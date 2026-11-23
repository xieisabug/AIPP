use super::*;

#[test]
fn test_mobile_plugin_requires_declaration_and_known_dependencies() {
    assert!(declaration_issue(&serde_json::json!({})).is_some());
    let mut manifest = serde_json::json!({"mobile":{"supported":true,"apiVersion":1},"runtime":{"type":"js"},"permissions":["network.image-generation"]});
    assert!(declaration_issue(&manifest).is_none());
    manifest["runtime"]["type"] = "viewer".into();
    assert!(declaration_issue(&manifest).is_none(), "iframe viewer runtime runs no host code");
    let declarative = serde_json::json!({"runtime":{"type":"declarative"},"contributions":{"avatars":["avatars/cat.json"]}});
    assert!(declaration_issue(&declarative).is_none(), "declarative avatar plugins require no JS runtime");
    let declarative_with_perm = serde_json::json!({"runtime":{"type":"declarative"},"permissions":["shell.execute"]});
    assert!(declaration_issue(&declarative_with_perm).is_some());
    manifest["runtime"]["type"] = "process".into();
    assert!(declaration_issue(&manifest).is_some());
    manifest["runtime"]["type"] = "js".into();
    manifest["permissions"] = serde_json::json!(["shell.execute"]);
    assert!(declaration_issue(&manifest).is_some());
}

#[test]
fn test_mobile_plugin_sdk_denies_desktop_and_arbitrary_commands() {
    assert!(command_allowed("plugin_comfyui_test_connection"));
    assert!(command_allowed("get_platform"));
    assert!(!command_allowed("execute_aipp_builtin_tool"));
    assert!(!command_allowed("execute_python"));
    assert!(!command_allowed("unknown_future_command"));
}
