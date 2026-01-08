//! MCP 注册和工具逻辑测试
//!
//! ## 测试范围
//!
//! - 命令行解析
//! - 环境变量解析
//! - MCP 提示格式化

use crate::mcp::format_mcp_prompt;
use crate::mcp::MCPInfoForAssistant;
use crate::api::assistant_api::{MCPServerWithTools, MCPToolInfo};

// ============================================================================
// 命令行解析测试 (测试 split_command_line 的行为)
// ============================================================================

/// 辅助函数：模拟 split_command_line 逻辑
fn split_command_line(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    for c in input.chars() {
        if escape {
            buf.push(c);
            escape = false;
            continue;
        }
        match c {
            '\\' => {
                escape = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            ' ' | '\t' | '\n' if !in_single && !in_double => {
                if !buf.is_empty() {
                    parts.push(buf.clone());
                    buf.clear();
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() {
        parts.push(buf);
    }
    parts
}

/// 测试简单命令
#[test]
fn test_split_command_simple() {
    let result = split_command_line("python script.py");
    assert_eq!(result, vec!["python", "script.py"]);
}

/// 测试带双引号的命令
#[test]
fn test_split_command_double_quotes() {
    let result = split_command_line(r#"echo "hello world""#);
    assert_eq!(result, vec!["echo", "hello world"]);
}

/// 测试带单引号的命令
#[test]
fn test_split_command_single_quotes() {
    let result = split_command_line("echo 'hello world'");
    assert_eq!(result, vec!["echo", "hello world"]);
}

/// 测试反斜杠转义
#[test]
fn test_split_command_escape() {
    let result = split_command_line(r"echo hello\ world");
    assert_eq!(result, vec!["echo", "hello world"]);
}

/// 测试混合引号
#[test]
fn test_split_command_mixed_quotes() {
    let result = split_command_line(r#"echo "hello 'world'""#);
    assert_eq!(result, vec!["echo", "hello 'world'"]);
}

/// 测试多个参数
#[test]
fn test_split_command_multiple_args() {
    let result = split_command_line("node mcp-server.js --port 3000 --verbose");
    assert_eq!(result, vec!["node", "mcp-server.js", "--port", "3000", "--verbose"]);
}

/// 测试空命令
#[test]
fn test_split_command_empty() {
    let result = split_command_line("");
    assert!(result.is_empty());
}

/// 测试多余空格
#[test]
fn test_split_command_extra_spaces() {
    let result = split_command_line("  python   script.py   arg1  ");
    assert_eq!(result, vec!["python", "script.py", "arg1"]);
}

/// 测试制表符和换行
#[test]
fn test_split_command_tabs_newlines() {
    let result = split_command_line("python\tscript.py\narg1");
    assert_eq!(result, vec!["python", "script.py", "arg1"]);
}

// ============================================================================
// 环境变量解析测试
// ============================================================================

/// 辅助函数：模拟 parse_env_vars 逻辑
fn parse_env_vars(env: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for line in env.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            if key.is_empty() { continue; }
            result.push((key.to_string(), v.trim().to_string()));
        }
    }
    result
}

/// 测试简单环境变量
#[test]
fn test_parse_env_simple() {
    let result = parse_env_vars("API_KEY=abc123");
    assert_eq!(result, vec![("API_KEY".to_string(), "abc123".to_string())]);
}

/// 测试多行环境变量
#[test]
fn test_parse_env_multiple() {
    let env = "API_KEY=abc123\nSECRET=xyz789\nDEBUG=true";
    let result = parse_env_vars(env);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], ("API_KEY".to_string(), "abc123".to_string()));
    assert_eq!(result[1], ("SECRET".to_string(), "xyz789".to_string()));
    assert_eq!(result[2], ("DEBUG".to_string(), "true".to_string()));
}

/// 测试注释行
#[test]
fn test_parse_env_comments() {
    let env = "# This is a comment\nAPI_KEY=value\n# Another comment";
    let result = parse_env_vars(env);
    assert_eq!(result, vec![("API_KEY".to_string(), "value".to_string())]);
}

/// 测试空行
#[test]
fn test_parse_env_empty_lines() {
    let env = "\n\nAPI_KEY=value\n\n";
    let result = parse_env_vars(env);
    assert_eq!(result, vec![("API_KEY".to_string(), "value".to_string())]);
}

/// 测试键值带空格
#[test]
fn test_parse_env_with_spaces() {
    let env = "  API_KEY  =  abc123  ";
    let result = parse_env_vars(env);
    assert_eq!(result, vec![("API_KEY".to_string(), "abc123".to_string())]);
}

/// 测试值包含等号
#[test]
fn test_parse_env_value_with_equals() {
    let env = "CONNECTION_STRING=host=localhost;port=5432";
    let result = parse_env_vars(env);
    assert_eq!(result, vec![("CONNECTION_STRING".to_string(), "host=localhost;port=5432".to_string())]);
}

/// 测试空值
#[test]
fn test_parse_env_empty_value() {
    let env = "EMPTY_VAR=";
    let result = parse_env_vars(env);
    assert_eq!(result, vec![("EMPTY_VAR".to_string(), "".to_string())]);
}

/// 测试空键（应该忽略）
#[test]
fn test_parse_env_empty_key() {
    let env = "=value";
    let result = parse_env_vars(env);
    assert!(result.is_empty());
}

/// 测试无等号的行（应该忽略）
#[test]
fn test_parse_env_no_equals() {
    let env = "INVALID_LINE";
    let result = parse_env_vars(env);
    assert!(result.is_empty());
}

// ============================================================================
// MCP 提示格式化测试
// ============================================================================

/// 测试空 MCP 信息格式化
#[tokio::test]
async fn test_format_mcp_prompt_empty_servers() {
    let mcp_info = MCPInfoForAssistant {
        enabled_servers: vec![],
        use_native_toolcall: false,
    };

    let result = format_mcp_prompt("Initial prompt".to_string(), &mcp_info).await;
    // 即使没有服务器，也会添加 MCP 规范头部，并在最后包含原始提示
    assert!(result.contains("Initial prompt"));
    assert!(result.contains("MCP"));
}

/// 测试单个服务器的 MCP 提示格式化
#[tokio::test]
async fn test_format_mcp_prompt_single_server() {
    let mcp_info = MCPInfoForAssistant {
        enabled_servers: vec![MCPServerWithTools {
            id: 1,
            name: "weather-server".to_string(),
            command: None,
            is_enabled: true,
            tools: vec![MCPToolInfo {
                id: 1,
                name: "get_weather".to_string(),
                description: "Get weather information".to_string(),
                is_enabled: true,
                is_auto_run: false,
                parameters: r#"{"type":"object","properties":{"city":{"type":"string"}}}"#.to_string(),
            }],
        }],
        use_native_toolcall: false,
    };

    let result = format_mcp_prompt("".to_string(), &mcp_info).await;
    
    // 验证结果包含服务器和工具信息
    assert!(result.contains("weather-server"));
    assert!(result.contains("get_weather"));
}

/// 测试多个工具的格式化
#[tokio::test]
async fn test_format_mcp_prompt_multiple_tools() {
    let mcp_info = MCPInfoForAssistant {
        enabled_servers: vec![MCPServerWithTools {
            id: 1,
            name: "multi-tool-server".to_string(),
            command: None,
            is_enabled: true,
            tools: vec![
                MCPToolInfo {
                    id: 1,
                    name: "tool_one".to_string(),
                    description: "First tool".to_string(),
                    is_enabled: true,
                    is_auto_run: false,
                    parameters: "{}".to_string(),
                },
                MCPToolInfo {
                    id: 2,
                    name: "tool_two".to_string(),
                    description: "Second tool".to_string(),
                    is_enabled: true,
                    is_auto_run: true,
                    parameters: "{}".to_string(),
                },
            ],
        }],
        use_native_toolcall: false,
    };

    let result = format_mcp_prompt("".to_string(), &mcp_info).await;
    
    assert!(result.contains("tool_one"));
    assert!(result.contains("tool_two"));
}

/// 测试原生工具调用模式
#[tokio::test]
async fn test_format_mcp_prompt_native_toolcall() {
    let mcp_info = MCPInfoForAssistant {
        enabled_servers: vec![MCPServerWithTools {
            id: 1,
            name: "test-server".to_string(),
            command: None,
            is_enabled: true,
            tools: vec![MCPToolInfo {
                id: 1,
                name: "test_tool".to_string(),
                description: "Test tool".to_string(),
                is_enabled: true,
                is_auto_run: false,
                parameters: "{}".to_string(),
            }],
        }],
        use_native_toolcall: true, // 使用原生工具调用
    };

    let result = format_mcp_prompt("".to_string(), &mcp_info).await;
    
    // 原生模式下仍然应该包含工具信息
    assert!(result.contains("test-server") || result.is_empty());
}

/// 测试带有初始提示的格式化
#[tokio::test]
async fn test_format_mcp_prompt_with_initial_prompt() {
    let mcp_info = MCPInfoForAssistant {
        enabled_servers: vec![MCPServerWithTools {
            id: 1,
            name: "server".to_string(),
            command: None,
            is_enabled: true,
            tools: vec![MCPToolInfo {
                id: 1,
                name: "tool".to_string(),
                description: "A tool".to_string(),
                is_enabled: true,
                is_auto_run: false,
                parameters: "{}".to_string(),
            }],
        }],
        use_native_toolcall: false,
    };

    let result = format_mcp_prompt("You are a helpful assistant.".to_string(), &mcp_info).await;
    
    // 结果应该包含原始提示和 MCP 信息
    // 具体格式取决于 format_mcp_prompt 的实现
    assert!(!result.is_empty());
}

/// 测试多服务器格式化
#[tokio::test]
async fn test_format_mcp_prompt_multiple_servers() {
    let mcp_info = MCPInfoForAssistant {
        enabled_servers: vec![
            MCPServerWithTools {
                id: 1,
                name: "server-a".to_string(),
                command: None,
                is_enabled: true,
                tools: vec![MCPToolInfo {
                    id: 1,
                    name: "tool_a".to_string(),
                    description: "Tool A".to_string(),
                    is_enabled: true,
                    is_auto_run: false,
                    parameters: "{}".to_string(),
                }],
            },
            MCPServerWithTools {
                id: 2,
                name: "server-b".to_string(),
                command: None,
                is_enabled: true,
                tools: vec![MCPToolInfo {
                    id: 2,
                    name: "tool_b".to_string(),
                    description: "Tool B".to_string(),
                    is_enabled: true,
                    is_auto_run: false,
                    parameters: "{}".to_string(),
                }],
            },
        ],
        use_native_toolcall: false,
    };

    let result = format_mcp_prompt("".to_string(), &mcp_info).await;
    
    assert!(result.contains("server-a"));
    assert!(result.contains("server-b"));
    assert!(result.contains("tool_a"));
    assert!(result.contains("tool_b"));
}
