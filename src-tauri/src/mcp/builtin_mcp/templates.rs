use crate::db::mcp_db::MCPDatabase;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tracing::{error, instrument};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinTemplateEnvVar {
    pub key: String,
    pub label: String,
    pub required: bool,
    pub tip: Option<String>,
    pub field_type: String, // "text", "select", "boolean", "number"
    pub default_value: Option<String>,
    pub placeholder: Option<String>,
    pub options: Option<Vec<EnvVarOption>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVarOption {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinTemplateInfo {
    pub id: String,             // unique key, e.g., "search"
    pub name: String,           // 显示名
    pub description: String,    // 描述
    pub command: String,        // aipp:search
    pub transport_type: String, // 固定 "stdio"
    pub required_envs: Vec<BuiltinTemplateEnvVar>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

fn builtin_templates() -> Vec<BuiltinTemplateInfo> {
    vec![BuiltinTemplateInfo {
        id: "search".into(),
        name: "搜索工具".into(),
        description: "内置的网络搜索和网页访问工具，支持多种搜索引擎和浏览器，可以通过搜索引擎获取到相关信息，并且可以调用访问工具进一步获取页面的具体信息".into(),
        command: "aipp:search".into(),
        transport_type: "stdio".into(),
        required_envs: vec![
            BuiltinTemplateEnvVar {
                key: "BROWSER_TYPE".into(),
                label: "浏览器类型".into(),
                required: false,
                tip: Some("搜索使用的浏览器类型，默认使用 Chrome，如果不可用则降级为 Edge".into()),
                field_type: "select".into(),
                default_value: Some("chrome".into()),
                placeholder: None,
                options: Some(vec![
                    EnvVarOption { label: "Chrome".into(), value: "chrome".into() },
                    EnvVarOption { label: "Edge".into(), value: "edge".into() },
                ]),
            },
            BuiltinTemplateEnvVar {
                key: "SEARCH_ENGINE".into(),
                label: "搜索引擎".into(),
                required: true,
                tip: Some("搜索使用的搜索引擎，默认使用 Google".into()),
                field_type: "select".into(),
                default_value: Some("google".into()),
                placeholder: None,
                options: Some(vec![
                    EnvVarOption { label: "Google".into(), value: "google".into() },
                    EnvVarOption { label: "Bing".into(), value: "bing".into() },
                    EnvVarOption { label: "DuckDuckGo".into(), value: "duckduckgo".into() },
                    EnvVarOption { label: "Kagi".into(), value: "kagi".into() },
                ]),
            },
            BuiltinTemplateEnvVar {
                key: "USER_DATA_DIR".into(),
                label: "浏览器数据目录".into(),
                required: false,
                tip: Some("使用的浏览器 profile 目录，用于共享登录状态和配置".into()),
                field_type: "text".into(),
                default_value: None,
                placeholder: Some("/path/to/browser/profile".into()),
                options: None,
            },
            BuiltinTemplateEnvVar {
                key: "PROXY_SERVER".into(),
                label: "代理服务器".into(),
                required: false,
                tip: Some("代理服务器地址，支持 HTTP 和 SOCKS5 协议".into()),
                field_type: "text".into(),
                default_value: None,
                placeholder: Some("http://proxy:port 或 socks5://proxy:port".into()),
                options: None,
            },
            BuiltinTemplateEnvVar {
                key: "HEADLESS".into(),
                label: "无头模式".into(),
                required: false,
                tip: Some("启用后浏览器在后台运行，关闭后会显示浏览器窗口（用于调试）".into()),
                field_type: "boolean".into(),
                default_value: Some("true".into()),
                placeholder: None,
                options: None,
            },
            BuiltinTemplateEnvVar {
                key: "KAGI_SESSION_URL".into(),
                label: "Kagi 会话链接".into(),
                required: false,
                tip: Some("仅在使用 Kagi 搜索引擎时需要填写。Kagi 是付费搜索引擎，需要提供带 token 的会话链接才能搜索。格式如：https://kagi.com/search?token=xxxxx。如果填写了此配置，搜索时将直接使用该链接拼接搜索参数，而不是模拟在首页输入搜索。注意：此配置仅对 Kagi 生效，其他搜索引擎请勿填写，否则可能导致搜索失败。".into()),
                field_type: "text".into(),
                default_value: None,
                placeholder: Some("https://kagi.com/search?token=xxxxx".into()),
                options: None,
            },
            BuiltinTemplateEnvVar {
                key: "WAIT_SELECTORS".into(),
                label: "等待元素选择器".into(),
                required: false,
                tip: Some("等待页面指定元素加载完成的 CSS 选择器，多个选择器用逗号分隔，程序已经对常用的搜索引擎进行了适配，如果发现适配出现问题，可以使用该属性进行覆盖".into()),
                field_type: "text".into(),
                default_value: None,
                placeholder: Some("#search-results, .content-area".into()),
                options: None,
            },
            BuiltinTemplateEnvVar {
                key: "WAIT_TIMEOUT_MS".into(),
                label: "等待超时时间".into(),
                required: false,
                tip: Some("等待页面元素加载的超时时间（毫秒）".into()),
                field_type: "number".into(),
                default_value: Some("15000".into()),
                placeholder: Some("15000".into()),
                options: None,
            },
        ],
    }]
}

pub fn get_builtin_tools_for_command(command: &str) -> Vec<BuiltinToolInfo> {
    match super::builtin_command_id(command).as_deref() {
        Some("search") => vec![
            BuiltinToolInfo {
                name: "search_web".into(),
                description: "搜索网络内容，当进行事实性验证、实事信息、研究特定主题等情况时使用最佳。当搜索结果没有可用时，可以尝试更改关键字进行搜索；当搜索结果的简介有限但判断该结果有可用性时，请进一步通过fetch_url工具获取到页面完整的信息。".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string", 
                            "description": "搜索查询关键词，支持中英文和各种搜索语法"
                        },
                        "result_type": {
                            "type": "string",
                            "enum": ["markdown", "items"],
                            "default": "markdown",
                            "description": "结果格式类型：\n- markdown: 将HTML转换为Markdown格式，便于阅读和处理\n- items: 返回结构化的搜索结果列表，包含标题、URL、摘要等字段"
                        }
                    },
                    "required": ["query"]
                }),
            },
            BuiltinToolInfo {
                name: "fetch_url".into(),
                description: "获取网页内容，支持多种结果格式。可以返回Markdown格式的网页内容。".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string", 
                            "description": "要获取内容的URL"
                        },
                        "result_type": {
                            "type": "string",
                            "enum": ["markdown"],
                            "default": "markdown",
                            "description": "结果格式类型：- markdown: 将HTML转换为Markdown格式，便于阅读和处理"
                        }
                    },
                    "required": ["url"]
                }),
            },
        ],
        _ => vec![],
    }
}

#[tauri::command]
#[instrument]
pub async fn list_aipp_builtin_templates() -> Result<Vec<BuiltinTemplateInfo>, String> {
    Ok(builtin_templates())
}

#[tauri::command]
#[instrument(skip(app_handle, envs), fields(template_id = %template_id))]
pub async fn add_or_update_aipp_builtin_server(
    app_handle: AppHandle,
    template_id: String,
    name: Option<String>,
    description: Option<String>,
    envs: Option<std::collections::HashMap<String, String>>,
) -> Result<i64, String> {
    (|| -> Result<i64> {
        let templates = builtin_templates();
        let tpl = templates
            .into_iter()
            .find(|t| t.id == template_id)
            .with_context(|| format!("Unknown builtin template id: {}", template_id))?;

        // envs to multiline string
        let env_str = envs.map(|m| {
            m.into_iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>().join("\n")
        });

        let db = MCPDatabase::new(&app_handle).context("Create MCPDatabase failed")?;
        let server_id = db
            .upsert_mcp_server_with_builtin(
                name.as_deref().unwrap_or(&tpl.name),              // name
                description.as_deref().or(Some(&tpl.description)), // description
                &tpl.transport_type,                               // transport_type
                Some(&tpl.command),                                // command
                env_str.as_deref(),                                // environment_variables
                None,                                              // headers
                None,                                              // url (builtins use stdio)
                Some(20000),                                       // timeout
                false,                                             // is_long_running
                true,                                              // is_enabled
                true,                                              // is_builtin
            )
            .context("Upsert builtin server failed")?;

        // 注册工具
        for tool in get_builtin_tools_for_command(&tpl.command) {
            db.upsert_mcp_server_tool(
                server_id,
                &tool.name,
                Some(&tool.description),
                Some(&tool.input_schema.to_string()),
            )
            .with_context(|| format!("Upsert server tool failed: {}", tool.name))?;
        }

        Ok(server_id)
    })()
    .map_err(|e| {
        error!(error = %e, "add_or_update_aipp_builtin_server failed");
        format!("{}", e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // builtin_templates Tests
    // ============================================

    #[test]
    fn test_builtin_templates_not_empty() {
        let templates = builtin_templates();
        assert!(!templates.is_empty(), "Should have at least one builtin template");
    }

    #[test]
    fn test_builtin_templates_search_exists() {
        let templates = builtin_templates();
        let search = templates.iter().find(|t| t.id == "search");
        assert!(search.is_some(), "Search template should exist");
    }

    #[test]
    fn test_search_template_has_required_fields() {
        let templates = builtin_templates();
        let search = templates.iter().find(|t| t.id == "search").unwrap();
        
        assert!(!search.name.is_empty());
        assert!(!search.description.is_empty());
        assert!(!search.command.is_empty());
        assert_eq!(search.transport_type, "stdio");
    }

    #[test]
    fn test_search_template_has_env_vars() {
        let templates = builtin_templates();
        let search = templates.iter().find(|t| t.id == "search").unwrap();
        
        assert!(!search.required_envs.is_empty(), "Search template should have environment variables");
    }

    #[test]
    fn test_search_template_browser_type_env() {
        let templates = builtin_templates();
        let search = templates.iter().find(|t| t.id == "search").unwrap();
        
        let browser_env = search.required_envs.iter().find(|e| e.key == "BROWSER_TYPE");
        assert!(browser_env.is_some(), "BROWSER_TYPE env should exist");
        
        let env = browser_env.unwrap();
        assert_eq!(env.field_type, "select");
        assert!(env.options.is_some());
        
        let options = env.options.as_ref().unwrap();
        assert!(options.iter().any(|o| o.value == "chrome"));
        assert!(options.iter().any(|o| o.value == "edge"));
    }

    #[test]
    fn test_search_template_search_engine_env() {
        let templates = builtin_templates();
        let search = templates.iter().find(|t| t.id == "search").unwrap();
        
        let engine_env = search.required_envs.iter().find(|e| e.key == "SEARCH_ENGINE");
        assert!(engine_env.is_some(), "SEARCH_ENGINE env should exist");
        
        let env = engine_env.unwrap();
        assert!(env.required, "SEARCH_ENGINE should be required");
        assert_eq!(env.default_value, Some("google".into()));
        
        let options = env.options.as_ref().unwrap();
        assert!(options.iter().any(|o| o.value == "google"));
        assert!(options.iter().any(|o| o.value == "bing"));
        assert!(options.iter().any(|o| o.value == "duckduckgo"));
        assert!(options.iter().any(|o| o.value == "kagi"));
    }

    #[test]
    fn test_search_template_headless_env() {
        let templates = builtin_templates();
        let search = templates.iter().find(|t| t.id == "search").unwrap();
        
        let headless_env = search.required_envs.iter().find(|e| e.key == "HEADLESS");
        assert!(headless_env.is_some(), "HEADLESS env should exist");
        
        let env = headless_env.unwrap();
        assert_eq!(env.field_type, "boolean");
        assert_eq!(env.default_value, Some("true".into()));
    }

    // ============================================
    // get_builtin_tools_for_command Tests
    // ============================================

    #[test]
    fn test_get_tools_for_search_command() {
        let tools = get_builtin_tools_for_command("aipp:search");
        assert_eq!(tools.len(), 2, "Search command should have 2 tools");
    }

    #[test]
    fn test_search_web_tool_exists() {
        let tools = get_builtin_tools_for_command("aipp:search");
        let search_web = tools.iter().find(|t| t.name == "search_web");
        assert!(search_web.is_some(), "search_web tool should exist");
    }

    #[test]
    fn test_fetch_url_tool_exists() {
        let tools = get_builtin_tools_for_command("aipp:search");
        let fetch_url = tools.iter().find(|t| t.name == "fetch_url");
        assert!(fetch_url.is_some(), "fetch_url tool should exist");
    }

    #[test]
    fn test_search_web_tool_schema() {
        let tools = get_builtin_tools_for_command("aipp:search");
        let search_web = tools.iter().find(|t| t.name == "search_web").unwrap();
        
        assert!(!search_web.description.is_empty());
        
        let schema = &search_web.input_schema;
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["query"].is_object());
        
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|r| r == "query"));
    }

    #[test]
    fn test_fetch_url_tool_schema() {
        let tools = get_builtin_tools_for_command("aipp:search");
        let fetch_url = tools.iter().find(|t| t.name == "fetch_url").unwrap();
        
        assert!(!fetch_url.description.is_empty());
        
        let schema = &fetch_url.input_schema;
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["url"].is_object());
        
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|r| r == "url"));
    }

    #[test]
    fn test_get_tools_for_unknown_command() {
        let tools = get_builtin_tools_for_command("unknown:command");
        assert!(tools.is_empty(), "Unknown command should return empty tools");
    }

    #[test]
    fn test_get_tools_for_empty_command() {
        let tools = get_builtin_tools_for_command("");
        assert!(tools.is_empty(), "Empty command should return empty tools");
    }

    // ============================================
    // BuiltinTemplateEnvVar Structure Tests
    // ============================================

    #[test]
    fn test_all_env_vars_have_valid_field_type() {
        let templates = builtin_templates();
        let valid_types = ["text", "select", "boolean", "number"];
        
        for template in templates {
            for env in template.required_envs {
                assert!(
                    valid_types.contains(&env.field_type.as_str()),
                    "Env var {} has invalid field_type: {}",
                    env.key,
                    env.field_type
                );
            }
        }
    }

    #[test]
    fn test_select_env_vars_have_options() {
        let templates = builtin_templates();
        
        for template in templates {
            for env in template.required_envs {
                if env.field_type == "select" {
                    assert!(
                        env.options.is_some() && !env.options.as_ref().unwrap().is_empty(),
                        "Select env var {} should have options",
                        env.key
                    );
                }
            }
        }
    }
}
