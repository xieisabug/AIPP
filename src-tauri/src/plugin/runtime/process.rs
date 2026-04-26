use crate::api::plugin_api::ResolvedPluginManifest;
use crate::plugin::hook_bus::HookRuntimeResult;
use crate::plugin::runtime::verify_entry_checksum;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const PROCESS_PROTOCOL_JSONRPC_STDIO: &str = "jsonrpc-stdio";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessHookHostInfo {
    app_version: String,
    schema_version: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessJsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: String,
    method: &'static str,
    params: ProcessHookParams<'a>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessHookParams<'a> {
    hook: &'a str,
    plugin_code: &'a str,
    context: &'a Value,
    host: ProcessHookHostInfo,
}

#[derive(Debug, Deserialize)]
struct ProcessJsonRpcResponse {
    result: Option<HookRuntimeResult>,
    error: Option<ProcessJsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct ProcessJsonRpcError {
    code: Option<i64>,
    message: String,
}

pub(crate) async fn execute_process_hook(
    manifest: &ResolvedPluginManifest,
    hook_name: &str,
    context: &Value,
) -> Result<HookRuntimeResult, String> {
    let protocol = manifest
        .runtime
        .protocol
        .as_deref()
        .unwrap_or(PROCESS_PROTOCOL_JSONRPC_STDIO);
    if protocol != PROCESS_PROTOCOL_JSONRPC_STDIO {
        return Err(format!(
            "Unsupported process plugin protocol '{}' for plugin {}",
            protocol, manifest.code
        ));
    }

    let entry_path = manifest.plugin_dir.join(&manifest.runtime.entry);
    verify_entry_checksum(&entry_path, manifest.runtime.checksum.as_deref())?;

    let request = ProcessJsonRpcRequest {
        jsonrpc: "2.0",
        id: format!("{}:{}", manifest.code, hook_name),
        method: "hook.handle",
        params: ProcessHookParams {
            hook: hook_name,
            plugin_code: &manifest.code,
            context,
            host: ProcessHookHostInfo {
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                schema_version: 1,
            },
        },
    };
    let request_json = serde_json::to_vec(&request)
        .map_err(|error| format!("Failed to serialize process hook input: {}", error))?;

    let mut child = Command::new(&entry_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            format!(
                "Failed to spawn process plugin '{}': {}",
                entry_path.display(),
                error
            )
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&request_json)
            .await
            .map_err(|error| format!("Failed to write process plugin stdin: {}", error))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|error| format!("Failed to finish process plugin stdin: {}", error))?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|error| format!("Failed to wait for process plugin: {}", error))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Process plugin '{}' exited with {}: {}",
            manifest.code,
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("Process plugin stdout was not UTF-8: {}", error))?;
    let response = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| format!("Process plugin '{}' returned empty stdout", manifest.code))?;

    let parsed = serde_json::from_str::<ProcessJsonRpcResponse>(response)
        .map_err(|error| format!("Process plugin returned invalid JSON-RPC response: {}", error))?;
    if let Some(error) = parsed.error {
        let code = error.code.map(|value| format!("{}: ", value)).unwrap_or_default();
        return Err(format!("Process plugin error {}{}", code, error.message));
    }
    parsed
        .result
        .ok_or_else(|| "Process plugin JSON-RPC response did not include result".to_string())
}
