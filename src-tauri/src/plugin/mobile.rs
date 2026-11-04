use crate::api::plugin_api::ResolvedPluginManifest;

pub(crate) fn declaration_issue(value: &serde_json::Value) -> Option<String> {
    if value.pointer("/runtime/type").and_then(|v| v.as_str()).unwrap_or("js") != "js" {
        return Some("进程或 WASM 插件尚不支持此移动运行时".to_string());
    }
    if value.pointer("/mobile/supported").and_then(|v| v.as_bool()) != Some(true)
        || value.pointer("/mobile/apiVersion").and_then(|v| v.as_u64()) != Some(1) {
        return Some("兼容性未验证：插件未声明 mobile.supported=true 和 apiVersion=1".to_string());
    }
    if value.pointer("/contributions/hooks").and_then(|v| v.as_array()).is_some_and(|v| !v.is_empty())
        || value.pointer("/contributions/bangs").and_then(|v| v.as_array()).is_some_and(|v| !v.is_empty()) {
        return Some("移动首批插件仅支持 UI 与显式网络操作，hooks/bangs 依赖尚未核验".to_string());
    }
    let allowed = ["plugin.storage", "assistant.read", "assistant.config", "assistant.prompt.write", "conversation.read", "conversation.write", "data.read.conversation", "data.read.assistant", "data.read.llm", "network.comfyui", "network.image-generation", "markdown.register"];
    if value.get("permissions").is_some_and(|value| !value.is_array()) {
        return Some("移动插件 permissions 必须是权限名称数组".to_string());
    }
    if let Some(permissions) = value.get("permissions").and_then(|v| v.as_array()) {
        for permission in permissions {
            if !permission.as_str().is_some_and(|permission| allowed.contains(&permission)) {
                return Some(format!("移动 API 尚未核验权限 {}", permission));
            }
        }
    }
    None
}

pub(crate) fn compatibility_issue(manifest: &ResolvedPluginManifest) -> Option<String> {
    if manifest.runtime.runtime_type != "js" || manifest.plugin_type.iter().any(|kind| kind == "assistantType") {
        return Some("此设备暂不执行进程、WASM 或插件助手类型".to_string());
    }
    match std::fs::read(manifest.plugin_dir.join("plugin.json")) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => declaration_issue(&value),
            Err(error) => Some(format!("插件兼容声明读取失败: {error}")),
        },
        Err(error) => Some(format!("插件兼容声明不可用: {error}")),
    }
}

pub(crate) fn command_allowed(command: &str) -> bool {
    matches!(command, "get_platform" | "get_assistants" | "get_models_for_select"
        | "get_plugin_data" | "set_plugin_data" | "get_plugin_config" | "set_plugin_config"
        | "plugin_get_conversation_with_messages" | "plugin_get_assistant_detail"
        | "plugin_storage_query" | "plugin_storage_execute" | "plugin_storage_schema"
        | "plugin_comfyui_test_connection" | "plugin_comfyui_generate_and_attach" | "plugin_execute_image_task")
}

#[cfg(test)]
#[path = "mobile_tests.rs"]
mod tests;
