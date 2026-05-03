use crate::api::plugin_api::ResolvedPluginManifest;
use crate::plugin::hook_bus::HookRuntimeResult;
use crate::plugin::runtime::verify_entry_checksum;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::sync::OnceLock;
use wasmtime::{Engine, Instance, Memory, Module, Store};

const ABI_SCHEMA_VERSION: u32 = 1;
const MAX_WASM_IO_BYTES: usize = 1024 * 1024;

static WASM_ENGINE: OnceLock<Engine> = OnceLock::new();

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WasmHookHostInfo {
    app_version: String,
    schema_version: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WasmHookInput<'a> {
    hook: &'a str,
    plugin_code: &'a str,
    context: &'a Value,
    host: WasmHookHostInfo,
}

pub(crate) fn execute_wasm_hook(
    manifest: &ResolvedPluginManifest,
    hook_name: &str,
    context: &Value,
) -> Result<HookRuntimeResult, String> {
    let wasm_path = manifest.plugin_dir.join(&manifest.runtime.entry);
    verify_entry_checksum(&wasm_path, manifest.runtime.checksum.as_deref())?;
    let input = WasmHookInput {
        hook: hook_name,
        plugin_code: &manifest.code,
        context,
        host: WasmHookHostInfo {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            schema_version: ABI_SCHEMA_VERSION,
        },
    };
    execute_wasm_hook_from_path(&wasm_path, &input)
}

fn execute_wasm_hook_from_path(
    wasm_path: &Path,
    input: &WasmHookInput<'_>,
) -> Result<HookRuntimeResult, String> {
    let engine = WASM_ENGINE.get_or_init(Engine::default);
    let module = Module::from_file(engine, wasm_path)
        .map_err(|error| format!("Failed to load WASM plugin '{}': {}", wasm_path.display(), error))?;
    let mut store = Store::new(engine, ());
    let instance = Instance::new(&mut store, &module, &[])
        .map_err(|error| format!("Failed to instantiate WASM plugin '{}': {}", wasm_path.display(), error))?;

    if let Ok(init) = instance.get_typed_func::<(), i32>(&mut store, "aipp_plugin_init") {
        let code = init
            .call(&mut store, ())
            .map_err(|error| format!("WASM plugin init failed: {}", error))?;
        if code != 0 {
            return Err(format!("WASM plugin init returned non-zero status: {}", code));
        }
    }

    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| "WASM plugin must export memory".to_string())?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&mut store, "aipp_plugin_alloc")
        .map_err(|_| "WASM plugin must export aipp_plugin_alloc(len: i32) -> i32".to_string())?;
    let handle_hook = instance
        .get_typed_func::<(i32, i32), i64>(&mut store, "aipp_plugin_handle_hook")
        .map_err(|_| {
            "WASM plugin must export aipp_plugin_handle_hook(ptr: i32, len: i32) -> i64"
                .to_string()
        })?;
    let free = instance
        .get_typed_func::<(i32, i32), ()>(&mut store, "aipp_plugin_free")
        .map_err(|_| "WASM plugin must export aipp_plugin_free(ptr: i32, len: i32)".to_string())?;

    let input_bytes = serde_json::to_vec(input)
        .map_err(|error| format!("Failed to serialize WASM hook input: {}", error))?;
    if input_bytes.len() > MAX_WASM_IO_BYTES {
        return Err(format!("WASM hook input is too large: {} bytes", input_bytes.len()));
    }

    let input_ptr = alloc
        .call(&mut store, input_bytes.len() as i32)
        .map_err(|error| format!("WASM plugin input allocation failed: {}", error))?;
    write_memory(&memory, &mut store, input_ptr, &input_bytes)?;

    let packed_result = handle_hook
        .call(&mut store, (input_ptr, input_bytes.len() as i32))
        .map_err(|error| format!("WASM plugin hook call failed: {}", error));
    let free_input_result = free.call(&mut store, (input_ptr, input_bytes.len() as i32));
    if let Err(error) = free_input_result {
        return Err(format!("WASM plugin input free failed: {}", error));
    }

    let packed_result = packed_result?;
    let (result_ptr, result_len) = unpack_ptr_len(packed_result)?;
    let output_bytes = read_memory(&memory, &mut store, result_ptr, result_len)?;
    free.call(&mut store, (result_ptr, result_len as i32))
        .map_err(|error| format!("WASM plugin result free failed: {}", error))?;

    serde_json::from_slice::<HookRuntimeResult>(&output_bytes)
        .map_err(|error| format!("WASM plugin returned invalid hook result JSON: {}", error))
}

fn write_memory(
    memory: &Memory,
    store: &mut Store<()>,
    ptr: i32,
    bytes: &[u8],
) -> Result<(), String> {
    let ptr = validate_ptr(ptr)?;
    memory
        .write(store, ptr, bytes)
        .map_err(|error| format!("Failed to write WASM memory: {}", error))
}

fn read_memory(
    memory: &Memory,
    store: &mut Store<()>,
    ptr: i32,
    len: usize,
) -> Result<Vec<u8>, String> {
    let ptr = validate_ptr(ptr)?;
    if len > MAX_WASM_IO_BYTES {
        return Err(format!("WASM hook output is too large: {} bytes", len));
    }
    let mut bytes = vec![0u8; len];
    memory
        .read(store, ptr, &mut bytes)
        .map_err(|error| format!("Failed to read WASM memory: {}", error))?;
    Ok(bytes)
}

fn validate_ptr(ptr: i32) -> Result<usize, String> {
    if ptr < 0 {
        Err(format!("WASM plugin returned negative pointer: {}", ptr))
    } else {
        Ok(ptr as usize)
    }
}

fn unpack_ptr_len(packed: i64) -> Result<(i32, usize), String> {
    if packed < 0 {
        return Err(format!("WASM plugin returned negative packed pointer/length: {}", packed));
    }
    let raw = packed as u64;
    let ptr = (raw >> 32) as u32 as i32;
    let len = (raw & 0xffff_ffff) as u32 as usize;
    if len > MAX_WASM_IO_BYTES {
        return Err(format!("WASM hook output is too large: {} bytes", len));
    }
    Ok((ptr, len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn build_constant_output_plugin(output_json: &str) -> Vec<u8> {
        let ptr = 1024u64;
        let len = output_json.as_bytes().len() as u64;
        let packed = (ptr << 32) | len;
        let escaped = output_json.replace('\\', "\\\\").replace('"', "\\\"");
        let wat = format!(
            r#"
            (module
              (memory (export "memory") 1)
              (global $heap (mut i32) (i32.const 4096))
              (func (export "aipp_plugin_init") (result i32)
                i32.const 0)
              (func (export "aipp_plugin_alloc") (param $len i32) (result i32)
                (local $ptr i32)
                global.get $heap
                local.set $ptr
                global.get $heap
                local.get $len
                i32.add
                global.set $heap
                local.get $ptr)
              (func (export "aipp_plugin_free") (param i32) (param i32))
              (data (i32.const 1024) "{escaped}")
              (func (export "aipp_plugin_handle_hook") (param i32) (param i32) (result i64)
                i64.const {packed})
            )
            "#
        );
        wat::parse_str(wat).expect("test WAT should compile")
    }

    #[test]
    fn wasm_hook_runtime_reads_replace_result() {
        let output_json = r#"{"action":"replace","context":{"prompt":"hooked"},"metadata":{}}"#;
        let wasm = build_constant_output_plugin(output_json);
        let temp_dir = tempfile::tempdir().unwrap();
        let wasm_path = temp_dir.path().join("plugin.wasm");
        std::fs::write(&wasm_path, wasm).unwrap();

        let input = WasmHookInput {
            hook: "chat.beforeSend",
            plugin_code: "test-plugin",
            context: &json!({ "prompt": "original" }),
            host: WasmHookHostInfo {
                app_version: "test".to_string(),
                schema_version: ABI_SCHEMA_VERSION,
            },
        };

        let result = execute_wasm_hook_from_path(&wasm_path, &input).unwrap();
        assert_eq!(result.context.unwrap()["prompt"], "hooked");
    }
}
