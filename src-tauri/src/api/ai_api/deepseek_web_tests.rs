use super::*;
use genai::chat::{ChatMessage, ContentPart};
use serde_json::json;

fn effort_name(options: &ChatOptions) -> Option<&'static str> {
    options.reasoning_effort.as_ref().map(|effort| effort.variant_name())
}

#[test]
fn test_deepseek_web_thinking_follows_assistant_unless_native_tools_need_it() {
    // (助手配置的思考, 本轮是否带原生工具, 期望的思考, 温度是否被去掉)
    let cases = [
        (None, false, None, false),
        (Some(ReasoningEffort::Zero), false, Some("zero"), false),
        (Some(ReasoningEffort::Medium), false, Some("medium"), true),
        (None, true, Some("high"), true),
        (Some(ReasoningEffort::Zero), true, Some("high"), true),
        (Some(ReasoningEffort::Low), true, Some("low"), true),
    ];
    for (configured, native, expected, drops_temperature) in cases {
        let mut request = ChatRequest::from_user("联网");
        if native { request = request.with_tools(vec![Tool::new("record")]); }
        let mut options = ChatOptions::default().with_temperature(0.5);
        options.reasoning_effort = configured.clone();
        configure(true, &[], true, &mut request, &mut options);
        assert_eq!(options.deepseek_web, Some(true));
        assert_eq!(effort_name(&options), expected, "{configured:?} native={native}");
        assert_eq!(options.temperature.is_none(), drops_temperature, "{configured:?} native={native}");
    }
}

#[test]
fn test_deepseek_web_disabled_leaves_options_untouched() {
    let mut request = ChatRequest::from_user("联网").with_tools(vec![Tool::new("record")]);
    let mut options = ChatOptions::default().with_temperature(0.5);
    configure(false, &[], true, &mut request, &mut options);
    assert!(options.deepseek_web.is_none());
    assert!(options.reasoning_effort.is_none());
    assert_eq!(options.temperature, Some(0.5));
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
    configure(false, &[], true, &mut request, &mut options);
    assert_eq!(request.messages[0].content.custom_parts()[0].data["server_tool_calls"], calls);
    configure(true, &[], false, &mut request, &mut options);
    assert!(request.messages[0].content.custom_parts().is_empty());
    assert!(options.deepseek_web.is_none());
}
