//! MCP 工具检测和处理逻辑测试
//!
//! ## 测试范围
//!
//! - MCP 工具调用正则表达式匹配
//! - 参数解析
//! - 多个工具调用检测
//! - 边界条件

use regex::Regex;
use serde_json::{json, Value};

// ============================================================================
// MCP 工具调用正则表达式测试
// ============================================================================

/// 获取 MCP 工具调用正则表达式（与 detection.rs 中使用的相同）
fn get_mcp_tool_call_regex() -> Regex {
    Regex::new(r"<mcp_tool_call>\s*<server_name>([^<]*)</server_name>\s*<tool_name>([^<]*)</tool_name>\s*<parameters>([\s\S]*?)</parameters>\s*</mcp_tool_call>").unwrap()
}

/// 测试基本的 MCP 工具调用检测
#[test]
fn test_mcp_tool_call_basic_detection() {
    let regex = get_mcp_tool_call_regex();
    
    let content = r#"
<mcp_tool_call>
<server_name>weather-server</server_name>
<tool_name>get_weather</tool_name>
<parameters>{"city": "Beijing"}</parameters>
</mcp_tool_call>
"#;

    let captures = regex.captures(content);
    assert!(captures.is_some());
    
    let cap = captures.unwrap();
    assert_eq!(cap.get(1).unwrap().as_str(), "weather-server");
    assert_eq!(cap.get(2).unwrap().as_str(), "get_weather");
    assert_eq!(cap.get(3).unwrap().as_str(), r#"{"city": "Beijing"}"#);
}

/// 测试紧凑格式的工具调用
#[test]
fn test_mcp_tool_call_compact_format() {
    let regex = get_mcp_tool_call_regex();
    
    let content = r#"<mcp_tool_call><server_name>test</server_name><tool_name>do_thing</tool_name><parameters>{}</parameters></mcp_tool_call>"#;

    let captures = regex.captures(content);
    assert!(captures.is_some());
    
    let cap = captures.unwrap();
    assert_eq!(cap.get(1).unwrap().as_str(), "test");
    assert_eq!(cap.get(2).unwrap().as_str(), "do_thing");
    assert_eq!(cap.get(3).unwrap().as_str(), "{}");
}

/// 测试多行参数
#[test]
fn test_mcp_tool_call_multiline_parameters() {
    let regex = get_mcp_tool_call_regex();
    
    let content = r#"
<mcp_tool_call>
<server_name>code-server</server_name>
<tool_name>execute</tool_name>
<parameters>{
  "code": "print('hello')\nprint('world')",
  "language": "python"
}</parameters>
</mcp_tool_call>
"#;

    let captures = regex.captures(content);
    assert!(captures.is_some());
    
    let cap = captures.unwrap();
    assert_eq!(cap.get(1).unwrap().as_str(), "code-server");
    assert_eq!(cap.get(2).unwrap().as_str(), "execute");
    
    let params = cap.get(3).unwrap().as_str();
    assert!(params.contains("hello"));
    assert!(params.contains("world"));
}

/// 测试多个工具调用
#[test]
fn test_mcp_tool_call_multiple() {
    let regex = get_mcp_tool_call_regex();
    
    let content = r#"
Let me check the weather and then search for restaurants.

<mcp_tool_call>
<server_name>weather</server_name>
<tool_name>get_weather</tool_name>
<parameters>{"city": "Shanghai"}</parameters>
</mcp_tool_call>

Based on the weather, I'll search for outdoor restaurants.

<mcp_tool_call>
<server_name>search</server_name>
<tool_name>find_restaurants</tool_name>
<parameters>{"type": "outdoor", "city": "Shanghai"}</parameters>
</mcp_tool_call>
"#;

    let mut matches = vec![];
    for cap in regex.captures_iter(content) {
        matches.push((
            cap.get(1).unwrap().as_str().to_string(),
            cap.get(2).unwrap().as_str().to_string(),
        ));
    }

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0], ("weather".to_string(), "get_weather".to_string()));
    assert_eq!(matches[1], ("search".to_string(), "find_restaurants".to_string()));
}

/// 测试无工具调用的内容
#[test]
fn test_mcp_tool_call_no_match() {
    let regex = get_mcp_tool_call_regex();
    
    let contents = vec![
        "This is a normal message without any tool calls.",
        "<mcp_tool_call>incomplete tag",
        "<server_name>test</server_name>", // missing wrapper
        "", // empty
    ];

    for content in contents {
        let captures = regex.captures(content);
        assert!(captures.is_none(), "Should not match: {}", content);
    }
}

/// 测试带有特殊字符的服务器名和工具名
#[test]
fn test_mcp_tool_call_special_names() {
    let regex = get_mcp_tool_call_regex();
    
    let content = r#"
<mcp_tool_call>
<server_name>my-server_v2.0</server_name>
<tool_name>get_data-v1</tool_name>
<parameters>{"key": "value"}</parameters>
</mcp_tool_call>
"#;

    let captures = regex.captures(content);
    assert!(captures.is_some());
    
    let cap = captures.unwrap();
    assert_eq!(cap.get(1).unwrap().as_str(), "my-server_v2.0");
    assert_eq!(cap.get(2).unwrap().as_str(), "get_data-v1");
}

/// 测试空参数
#[test]
fn test_mcp_tool_call_empty_parameters() {
    let regex = get_mcp_tool_call_regex();
    
    let content = r#"
<mcp_tool_call>
<server_name>test</server_name>
<tool_name>no_params</tool_name>
<parameters></parameters>
</mcp_tool_call>
"#;

    let captures = regex.captures(content);
    assert!(captures.is_some());
    
    let cap = captures.unwrap();
    assert_eq!(cap.get(3).unwrap().as_str(), "");
}

/// 测试带有 XML 特殊字符的参数
#[test]
fn test_mcp_tool_call_xml_special_chars() {
    let regex = get_mcp_tool_call_regex();
    
    let content = r#"
<mcp_tool_call>
<server_name>test</server_name>
<tool_name>search</tool_name>
<parameters>{"query": "a &amp; b", "filter": "x &gt; 10"}</parameters>
</mcp_tool_call>
"#;

    let captures = regex.captures(content);
    assert!(captures.is_some());
    
    let cap = captures.unwrap();
    let params = cap.get(3).unwrap().as_str();
    assert!(params.contains("&amp;"));
    assert!(params.contains("&gt;"));
}

// ============================================================================
// JSON 参数解析测试
// ============================================================================

/// 测试 JSON 参数解析
#[test]
fn test_parse_json_parameters() {
    let params_str = r#"{"city": "Beijing", "units": "celsius"}"#;
    let parsed: Result<Value, _> = serde_json::from_str(params_str);
    
    assert!(parsed.is_ok());
    let value = parsed.unwrap();
    assert_eq!(value["city"], "Beijing");
    assert_eq!(value["units"], "celsius");
}

/// 测试复杂嵌套 JSON 参数
#[test]
fn test_parse_nested_json_parameters() {
    let params_str = r#"{
        "query": "test",
        "options": {
            "limit": 10,
            "filters": ["active", "verified"]
        }
    }"#;
    
    let parsed: Result<Value, _> = serde_json::from_str(params_str);
    assert!(parsed.is_ok());
    
    let value = parsed.unwrap();
    assert_eq!(value["query"], "test");
    assert_eq!(value["options"]["limit"], 10);
}

/// 测试无效 JSON 参数
#[test]
fn test_parse_invalid_json_parameters() {
    let invalid_params = vec![
        "not json at all",
        "{missing: quotes}",
        "{'single': 'quotes'}", // Python style
    ];
    
    for params in invalid_params {
        let parsed: Result<Value, _> = serde_json::from_str(params);
        assert!(parsed.is_err(), "Should fail to parse: {}", params);
    }
}

// ============================================================================
// MCP 工具调用状态测试
// ============================================================================

/// 测试工具调用状态枚举
#[test]
fn test_tool_call_status_transitions() {
    // 模拟状态转换: pending -> running -> success/failed
    let valid_transitions = vec![
        ("pending", "running"),
        ("running", "success"),
        ("running", "failed"),
        ("pending", "cancelled"),
    ];
    
    for (from, to) in valid_transitions {
        // 验证有效转换
        let is_valid = match (from, to) {
            ("pending", "running") => true,
            ("pending", "cancelled") => true,
            ("running", "success") => true,
            ("running", "failed") => true,
            _ => false,
        };
        assert!(is_valid, "Transition from {} to {} should be valid", from, to);
    }
    
    // 无效转换
    let invalid_transitions = vec![
        ("success", "running"),
        ("failed", "success"),
        ("cancelled", "running"),
    ];
    
    for (from, to) in invalid_transitions {
        let is_valid = match (from, to) {
            ("pending", "running") => true,
            ("pending", "cancelled") => true,
            ("running", "success") => true,
            ("running", "failed") => true,
            _ => false,
        };
        assert!(!is_valid, "Transition from {} to {} should be invalid", from, to);
    }
}

// ============================================================================
// MCP 内容替换测试
// ============================================================================

/// 测试工具调用标记替换
#[test]
fn test_mcp_tool_call_marker_replacement() {
    let original_content = r#"
Let me search for that.

<mcp_tool_call>
<server_name>search</server_name>
<tool_name>web_search</tool_name>
<parameters>{"query": "rust programming"}</parameters>
</mcp_tool_call>

I found the results.
"#;

    let regex = get_mcp_tool_call_regex();
    let call_id = 42;
    
    // 替换为注释标记
    let replacement = format!("<!-- MCP_TOOL_CALL:{} -->", call_id);
    let replaced = regex.replace_all(original_content, replacement.as_str());
    
    assert!(replaced.contains("<!-- MCP_TOOL_CALL:42 -->"));
    assert!(!replaced.contains("<mcp_tool_call>"));
    assert!(!replaced.contains("</mcp_tool_call>"));
}

/// 测试多个工具调用的替换
#[test]
fn test_mcp_multiple_tool_calls_replacement() {
    let content = r#"
<mcp_tool_call>
<server_name>a</server_name>
<tool_name>tool_a</tool_name>
<parameters>{}</parameters>
</mcp_tool_call>

<mcp_tool_call>
<server_name>b</server_name>
<tool_name>tool_b</tool_name>
<parameters>{}</parameters>
</mcp_tool_call>
"#;

    let regex = get_mcp_tool_call_regex();
    
    // 计数替换
    let mut count = 0;
    let replaced = regex.replace_all(content, |_: &regex::Captures| {
        count += 1;
        format!("<!-- REPLACED:{} -->", count)
    });
    
    assert_eq!(count, 2);
    assert!(replaced.contains("<!-- REPLACED:1 -->"));
    assert!(replaced.contains("<!-- REPLACED:2 -->"));
}

// ============================================================================
// 边界条件测试
// ============================================================================

/// 测试非常长的参数
#[test]
fn test_mcp_tool_call_very_long_parameters() {
    let regex = get_mcp_tool_call_regex();
    
    // 创建一个 10KB 的 JSON 参数
    let long_value = "x".repeat(10 * 1024);
    let params = format!(r#"{{"data": "{}"}}"#, long_value);
    
    let content = format!(r#"
<mcp_tool_call>
<server_name>test</server_name>
<tool_name>process</tool_name>
<parameters>{}</parameters>
</mcp_tool_call>
"#, params);

    let captures = regex.captures(&content);
    assert!(captures.is_some());
    
    let cap = captures.unwrap();
    let extracted_params = cap.get(3).unwrap().as_str();
    assert!(extracted_params.len() > 10 * 1024);
}

/// 测试带有 Unicode 的内容
#[test]
fn test_mcp_tool_call_unicode_content() {
    let regex = get_mcp_tool_call_regex();
    
    let content = r#"
<mcp_tool_call>
<server_name>翻译服务</server_name>
<tool_name>translate</tool_name>
<parameters>{"text": "Hello 世界", "to": "中文"}</parameters>
</mcp_tool_call>
"#;

    let captures = regex.captures(content);
    assert!(captures.is_some());
    
    let cap = captures.unwrap();
    assert_eq!(cap.get(1).unwrap().as_str(), "翻译服务");
    
    let params = cap.get(3).unwrap().as_str();
    assert!(params.contains("世界"));
}

/// 测试工具名称包含空格的情况（应该被捕获但可能无效）
#[test]
fn test_mcp_tool_call_name_with_spaces() {
    let regex = get_mcp_tool_call_regex();
    
    let content = r#"
<mcp_tool_call>
<server_name>  spaced server  </server_name>
<tool_name>  spaced tool  </tool_name>
<parameters>{}</parameters>
</mcp_tool_call>
"#;

    let captures = regex.captures(content);
    assert!(captures.is_some());
    
    let cap = captures.unwrap();
    // 正则会捕获包含空格的名称
    let server_name = cap.get(1).unwrap().as_str();
    let tool_name = cap.get(2).unwrap().as_str();
    
    // 应用程序代码应该使用 .trim() 处理
    assert_eq!(server_name.trim(), "spaced server");
    assert_eq!(tool_name.trim(), "spaced tool");
}
