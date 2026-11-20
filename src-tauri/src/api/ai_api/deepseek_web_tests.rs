use super::*;
use genai::chat::{ChatMessage, ContentPart};
use serde_json::json;

#[test]
fn test_deepseek_web_thinking_depends_on_actual_tools() {
    for (mode, native, expected) in [(WebMode::Search, false, "zero"), (WebMode::Deep, false, "high"), (WebMode::Search, true, "high")] {
        let mut request = ChatRequest::from_user("联网");
        if native { request = request.with_tools(vec![Tool::new("record")]); }
        let mut options = ChatOptions::default().with_temperature(0.5);
        configure(mode, &[], true, &mut request, &mut options);
        assert_eq!(options.deepseek_web, Some(true));
        assert_eq!(options.reasoning_effort.unwrap().variant_name(), expected);
        assert_eq!(options.temperature.is_none(), expected == "high");
    }
}

#[test]
fn test_deepseek_web_filters_only_generated_search_catalogue() {
    let mut prompt = "# 助手指令\n\n请保留 search_web 示例\n## 可用的 MCP 工具\n\n### 服务器: 搜索\n\n#### 可用工具:\n\n**fetch_url** \n\n---\n\n### 服务器: 资料\n正文".to_owned();
    remove_search_catalogue(&mut prompt, &["搜索".into()]);
    assert!(prompt.contains("保留 search_web 示例"));
    assert!(prompt.contains("资料"));
    assert!(!prompt.contains("fetch_url"));
}

#[test]
fn test_deepseek_web_signed_history_survives_disabled_search_only_for_flash() {
    let calls = json!([{"calls":[{"signature":"signed"}]}]);
    let part = ContentPart::from_custom(json!({"type":"deepseek_web","server_tool_calls":calls}),
        Some(genai::ModelIden::new(genai::adapter::AdapterKind::DeepSeek, "deepseek-flash")));
    let mut request = ChatRequest::new(vec![ChatMessage::assistant(genai::chat::MessageContent::from_parts(vec![part]))]);
    let mut options = ChatOptions::default();
    configure(WebMode::Off, &[], true, &mut request, &mut options);
    assert_eq!(request.messages[0].content.custom_parts()[0].data["server_tool_calls"], calls);
    configure(WebMode::Deep, &[], false, &mut request, &mut options);
    assert!(request.messages[0].content.custom_parts().is_empty());
    assert!(options.deepseek_web.is_none());
}
