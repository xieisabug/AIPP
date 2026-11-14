use super::*;
use crate::mcp::builtin_mcp::operation::bash_ops::BashOperations;
use crate::mcp::builtin_mcp::operation::state::OperationState;
use crate::mcp::builtin_mcp::operation::types::{BashProcessStatus, ExecuteBashRequest, GetBashOutputRequest};
use serde_json::json;

const TOKEN: &str = "aipp-fake-token-7f3a91";

fn server(transport: &str, command: Option<&str>, url: Option<&str>) -> MCPServer {
    MCPServer {
        id: 1, name: "测试服务".into(), description: String::new(), transport_type: transport.into(),
        command: command.map(str::to_owned), environment_variables: None, headers: None, url: url.map(str::to_owned),
        timeout: None, is_long_running: false, is_enabled: true, is_builtin: false, is_deletable: true,
        proxy_enabled: false, created_time: String::new(),
    }
}

fn vault_server() -> MCPServer { server("stdio", Some(super::super::tool::COMMAND), None) }

#[test]
fn test_keys_found_anywhere_in_the_arguments() {
    let args = json!({
        "headers": {"Authorization": "Bearer <vault:github_token>"},
        "body": {"people": [{"id": "<vault:id_card>", "phone": "<vault:phone> / <vault:id_card>"}]},
        "count": 3,
    });
    let keys: Vec<String> = keys_in(&args).unwrap().into_iter().collect();
    assert_eq!(keys, vec!["github_token", "id_card", "phone"]);
    assert!(keys_in(&json!({"text": "no placeholders"})).unwrap().is_empty());
    // A typo is never sent literally.
    assert_eq!(keys_in(&json!({"x": "<vault:ID_Card>"})).unwrap_err().code, "invalid_placeholder");
    assert_eq!(keys_in(&json!({"x": "<vault:id_card"})).unwrap_err().code, "invalid_placeholder");
}

#[test]
fn test_substitution_keeps_json_valid() {
    let mut args = json!({"note": "a <vault:quote> b", "list": ["<vault:quote>"]});
    let values = BTreeMap::from([("quote".to_string(), "say \"hi\"\nnext".to_string())]);
    substitute_value(&mut args, &values);
    let text = args.to_string();
    let reparsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(reparsed["note"], "a say \"hi\"\nnext b");
    assert_eq!(reparsed["list"][0], "say \"hi\"\nnext");
}

#[test]
fn test_destination_for_each_kind_of_tool() {
    let http = destination(&vault_server(), super::super::tool::REQUEST_TOOL,
        &json!({"method": "post", "url": "https://kyfw.12306.cn/passenger/add?x=<vault:id_card>"})).unwrap();
    assert_eq!(http.kind, "site");
    assert_eq!(http.origin.as_deref(), Some("https://kyfw.12306.cn"));
    assert_eq!(http.label, "网页请求 POST https://kyfw.12306.cn/passenger/add");
    assert!(!http.insecure);
    let plain = destination(&vault_server(), super::super::tool::REQUEST_TOOL, &json!({"url": "http://192.168.1.2:8080/"})).unwrap();
    assert!(plain.insecure);
    // The host must be known when the user approves.
    let hidden = destination(&vault_server(), super::super::tool::REQUEST_TOOL, &json!({"url": "https://<vault:host>/x"}));
    assert_eq!(hidden.unwrap_err().code, "invalid_placeholder");
    assert!(destination(&vault_server(), super::super::tool::LIST_TOOL, &json!({})).is_err());

    let mcp = destination(&server("http", None, Some("https://mcp.example.com/mcp")), "create_issue", &json!({})).unwrap();
    assert_eq!((mcp.kind, mcp.origin.as_deref()), ("site", Some("https://mcp.example.com")));
    assert_eq!(mcp.label, "MCP「测试服务」· https://mcp.example.com");
    let stdio = destination(&server("stdio", Some("npx -y some-server"), None), "run", &json!({})).unwrap();
    assert_eq!((stdio.kind, stdio.label.as_str()), ("local", "本机程序「测试服务」"));
    let shell = destination(&server("stdio", Some("aipp:operation"), None), "execute_bash", &json!({})).unwrap();
    assert_eq!((shell.kind, shell.label.as_str()), ("local", "本机命令"));
    let file = destination(&server("stdio", Some("aipp:operation"), None), "write_file", &json!({})).unwrap();
    assert_eq!(file.label, "本机文件");
}

#[test]
fn test_shell_placeholders_become_environment_references() {
    let vars = BTreeMap::from([("gh".to_string(), env_var("gh"))]);
    let command = r#"curl -H "Authorization: Bearer <vault:gh>" https://api.github.com/user"#;
    assert_eq!(rewrite_command(command, false, &vars).unwrap(),
        r#"curl -H "Authorization: Bearer ${AIPP_VAULT_GH}" https://api.github.com/user"#);
    assert_eq!(rewrite_command(command, true, &vars).unwrap(),
        r#"curl -H "Authorization: Bearer ${env:AIPP_VAULT_GH}" https://api.github.com/user"#);
    assert_eq!(rewrite_command("export T=<vault:gh>", false, &vars).unwrap(), "export T=${AIPP_VAULT_GH}");
    // Single quotes never expand variables.
    assert!(rewrite_command("echo '<vault:gh>'", false, &vars).is_err());
    assert!(rewrite_command("echo 'it''s <vault:gh>'", true, &vars).is_err());
    // An escaped quote does not open a quoted section.
    assert!(rewrite_command(r"echo \'<vault:gh>", false, &vars).is_ok());
    assert!(rewrite_command("echo `'<vault:gh>", true, &vars).is_ok());
    assert!(rewrite_command(r#"echo "it's <vault:gh>""#, false, &vars).is_ok(), "a single quote inside double quotes is literal");
}

#[test]
fn test_value_typed_by_the_user_is_used_once_without_asking() {
    let item = "00000000-0000-4000-8000-0000000000aa";
    assert!(!has_once(Some(42), item));
    grant_once(42, item);
    assert!(has_once(Some(42), item));
    assert!(!has_once(Some(43), item), "only in the conversation it was typed for");
    assert!(!has_once(None, item));
    consume_once(Some(42), item);
    assert!(!has_once(Some(42), item));
}

#[tokio::test]
async fn test_no_placeholder_is_a_passthrough() {
    let prepared = Prepared::default();
    assert_eq!(prepared.parameters("{\"a\":1}"), "{\"a\":1}");
    assert_eq!(prepared.clean(format!("x {TOKEN}")), format!("x {TOKEN}"));
}

fn echo_command() -> &'static str {
    // Without an app handle the operation tool uses the platform default shell.
    if cfg!(target_os = "windows") { r#"Write-Output "value=<vault:gh>""# } else { r#"echo "value=<vault:gh>""# }
}

#[tokio::test]
async fn test_shell_receives_the_value_as_an_environment_variable() {
    let state = OperationState::new();
    let request = ExecuteBashRequest { command: echo_command().into(), description: None, timeout: Some(30_000), run_in_background: Some(false) };
    let response = with_shell(test_shell(&[("gh", TOKEN)]), BashOperations::execute_bash(None, &state, request)).await.unwrap();
    let output = response.output.unwrap();
    // The child saw the real value; execute_tool_by_transport redacts it before the model sees it.
    assert!(output.contains(&format!("value={TOKEN}")), "{output}");
}

#[tokio::test]
async fn test_background_output_is_redacted_as_it_arrives() {
    let state = OperationState::new();
    let request = ExecuteBashRequest { command: echo_command().into(), description: None, timeout: None, run_in_background: Some(true) };
    let response = with_shell(test_shell(&[("gh", TOKEN)]), BashOperations::execute_bash(None, &state, request)).await.unwrap();
    let bash_id = response.bash_id.unwrap();
    let mut collected = String::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let output = BashOperations::get_bash_output(&state, GetBashOutputRequest { bash_id: bash_id.clone(), filter: None }).await.unwrap();
        collected.push_str(&output.output);
        if output.status != BashProcessStatus::Running || std::time::Instant::now() > deadline { break; }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(collected.contains("value=[REDACTED]"), "{collected}");
    assert!(!collected.contains(TOKEN));
}
