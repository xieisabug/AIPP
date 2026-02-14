use crate::db::mcp_db::MCPDatabase;
use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
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
    pub default_timeout: Option<i32>, // 默认超时时间（毫秒）
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

fn builtin_templates() -> Vec<BuiltinTemplateInfo> {
    vec![
        // Agent 工具
        BuiltinTemplateInfo {
            id: "agent".into(),
            name: "Agent 工具".into(),
            description: "内置的 Agent 能力工具集，包含 Skill 加载等功能。当 AI 需要执行用户定义的技能时，可以通过此工具加载技能的详细指令。".into(),
            command: "aipp:agent".into(),
            transport_type: "stdio".into(),
            required_envs: vec![],
            default_timeout: Some(30000), // 30秒
        },
        // 搜索工具
        BuiltinTemplateInfo {
            id: "search".into(),
            name: "搜索工具".into(),
            description: "内置的网络搜索和网页访问工具，支持多种搜索引擎和浏览器，可以通过搜索引擎获取到相关信息，并且可以调用访问工具进一步获取页面的具体信息".into(),
            command: "aipp:search".into(),
            transport_type: "stdio".into(),
            default_timeout: Some(60000), // 60秒，搜索和抓取需要更多时间
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
        },
        // 操作工具
        BuiltinTemplateInfo {
            id: "operation".into(),
            name: "操作工具".into(),
            description: "内置的文件操作和命令执行工具，支持文件读写、目录列表、命令行执行等操作。可以帮助 AI 助手完成代码编辑、文件管理、脚本执行等任务。".into(),
            command: "aipp:operation".into(),
            transport_type: "stdio".into(),
            required_envs: vec![
                BuiltinTemplateEnvVar {
                    key: "ALLOWED_DIRECTORIES".into(),
                    label: "允许访问的目录".into(),
                    required: false,
                    tip: Some("允许操作的目录白名单，每行一个目录路径。如果为空，所有操作都需要用户确认授权。".into()),
                    field_type: "textarea".into(),
                    default_value: None,
                    placeholder: Some("/Users/username/projects\n/tmp".into()),
                    options: None,
                },
                BuiltinTemplateEnvVar {
                    key: "DEFAULT_SHELL".into(),
                    label: "默认 Shell".into(),
                    required: false,
                    tip: Some("执行命令时使用的 Shell，默认自动检测（macOS/Linux 使用 zsh/bash，Windows 使用 PowerShell）".into()),
                    field_type: "select".into(),
                    default_value: Some("auto".into()),
                    placeholder: None,
                    options: Some(vec![
                        EnvVarOption { label: "自动检测".into(), value: "auto".into() },
                        EnvVarOption { label: "Bash".into(), value: "bash".into() },
                        EnvVarOption { label: "Zsh".into(), value: "zsh".into() },
                        EnvVarOption { label: "PowerShell".into(), value: "powershell".into() },
                    ]),
                },
                BuiltinTemplateEnvVar {
                    key: "MAX_READ_LINES".into(),
                    label: "文件读取行数限制".into(),
                    required: false,
                    tip: Some("单次读取文件的最大行数，默认 2000 行".into()),
                    field_type: "number".into(),
                    default_value: Some("2000".into()),
                    placeholder: Some("2000".into()),
                    options: None,
                },
                BuiltinTemplateEnvVar {
                    key: "COMMAND_TIMEOUT_MS".into(),
                    label: "命令超时时间".into(),
                    required: false,
                    tip: Some("命令执行的默认超时时间（毫秒），默认 120000（2分钟），最大 600000（10分钟）".into()),
                    field_type: "number".into(),
                    default_value: Some("120000".into()),
                    placeholder: Some("120000".into()),
                    options: None,
                },
            ],
            default_timeout: Some(180000), // 3分钟，操作工具可能执行较长命令
        },
        BuiltinTemplateInfo {
            id: "dynamic_mcp".into(),
            name: "MCP 动态加载工具".into(),
            description: "为 MCP 动态加载场景提供按需加载工具能力。".into(),
            command: "aipp:dynamic_mcp".into(),
            transport_type: "stdio".into(),
            required_envs: vec![],
            default_timeout: Some(30000),
        },
    ]
}

pub fn get_builtin_tools_for_command(command: &str) -> Vec<BuiltinToolInfo> {
    match super::builtin_command_id(command).as_deref() {
        Some("agent") => vec![
            BuiltinToolInfo {
                name: "load_skill".into(),
                description: "Load a skill's detailed instructions (SKILL.md). Use this tool when you need to execute a user-defined skill. The skill's prompt will provide detailed instructions on how to complete the task.".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The skill name only (no arguments). E.g., 'pdf' or 'xlsx'. This should match the skill's display name or file name."
                        },
                        "source_type": {
                            "type": "string",
                            "description": "The source type of the skill. Available types: 'aipp' (AIPP Skills), 'claude_code_agents' (Claude Code Agents), 'claude_code_rules' (Claude Code Rules), 'codex' (Codex), or custom source types."
                        }
                    },
                    "required": ["command", "source_type"]
                }),
            },
            BuiltinToolInfo {
                name: "todo_write".into(),
                description: r#"Create and manage a structured task list for your current coding session. This helps you track progress, organize complex tasks, and demonstrate thoroughness to the user. It also helps the user understand the progress of the task and overall progress of their requests.

**When to Use This Tool:**
1. **Complex multi-step tasks** - When a task requires 3 or more distinct steps or actions
2. **Non-trivial and complex tasks** - Tasks that require careful planning or multiple operations
3. **User explicitly requests todo list** - When the user directly asks you to use the todo list
4. **User provides multiple tasks** - When users provide a list of things to be done (numbered or comma-separated)
5. **After receiving new instructions** - Immediately capture user requirements as todos
6. **When you start working on a task** - Mark it as in_progress BEFORE beginning work. **Ideally you should only have one todo as in_progress at a time**
7. **After completing a task** - Mark it as completed and add any new follow-up tasks discovered during implementation

**When NOT to Use This Tool:**
- There is only a single, straightforward task
- The task is trivial and tracking it provides no organizational benefit
- The task can be completed in less than 3 trivial steps
- The task is purely conversational or informational
- NOTE that you should **not use this tool if there is only one trivial task to do**. In this case you are better off just doing the task directly

**Task Management:**
- Update task status in real-time as you work
- Mark tasks complete **IMMEDIATELY** after finishing (**don't batch completions**)
- **Exactly ONE task must be in_progress at any time (not less, not more)**
- Complete current tasks before starting new ones
- Remove tasks that are no longer relevant from the list entirely

**Task Completion Requirements:**
- **ONLY** mark a task as completed when you have **FULLY** accomplished it
- If you encounter errors, blockers, or cannot finish, keep the task as in_progress
- When blocked, create a new task describing what needs to be resolved
- Never mark a task as completed if: Tests are failing, Implementation is partial, You encountered unresolved errors, You couldn't find necessary files or dependencies

**Task Breakdown:**
- Create specific, actionable items
- Break complex tasks into smaller, manageable steps
- Use clear, descriptive task names
- Always provide both forms: content: "Fix authentication bug", activeForm: "Fixing authentication bug"

**Critical Rule:**
- It is critical that you mark todos as completed **as soon as you are done** with a task. **Do not batch up multiple tasks before marking them as completed**"#.into(),
                input_schema: serde_json::json!({
                    "$schema": "http://json-schema.org/draft-07/schema#",
                    "type": "object",
                    "required": ["todos"],
                    "additionalProperties": false,
                    "properties": {
                        "todos": {
                            "type": "array",
                            "description": "The updated todo list",
                            "items": {
                                "type": "object",
                                "required": ["content", "status", "activeForm"],
                                "additionalProperties": false,
                                "properties": {
                                    "content": {
                                        "type": "string",
                                        "minLength": 1,
                                        "description": "Imperative form: what needs to be done (e.g., 'Fix authentication bug')"
                                    },
                                    "status": {
                                        "type": "string",
                                        "enum": ["pending", "in_progress", "completed"],
                                        "description": "Task status: pending (not started), in_progress (currently working on), completed (finished)"
                                    },
                                    "activeForm": {
                                        "type": "string",
                                        "minLength": 1,
                                        "description": "Present continuous form: what's being done (e.g., 'Fixing authentication bug')"
                                    }
                                }
                            }
                        }
                    }
                }),
            },
        ],
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
        Some("operation") => vec![
            BuiltinToolInfo {
                name: "read_file".into(),
                description: "读取文件内容。支持部分读取（通过 offset 和 limit 参数）。返回带行号的内容（类似 cat -n 格式）。必须使用绝对路径。".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "要读取的文件的绝对路径"
                        },
                        "offset": {
                            "type": "number",
                            "description": "开始读取的行号（1-indexed）。仅在文件过大无法一次读取时使用"
                        },
                        "limit": {
                            "type": "number",
                            "description": "要读取的行数。仅在文件过大无法一次读取时使用"
                        }
                    },
                    "required": ["file_path"]
                }),
            },
            BuiltinToolInfo {
                name: "write_file".into(),
                description: "创建新文件或完全覆盖现有文件。必须使用绝对路径。安全机制：覆盖现有文件前必须先使用 read_file 读取该文件。推荐使用 edit_file 进行文件修改，write_file 仅用于创建新文件。".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "要写入的文件的绝对路径"
                        },
                        "content": {
                            "type": "string",
                            "description": "要写入的完整文件内容"
                        }
                    },
                    "required": ["file_path", "content"]
                }),
            },
            BuiltinToolInfo {
                name: "edit_file".into(),
                description: "对文件进行精确的字符串替换。必须使用绝对路径。要求：1) 必须先用 read_file 读取文件；2) old_string 必须精确匹配文件中的文本（包括空格和缩进）；3) 默认情况下 old_string 必须在文件中唯一出现，否则需要设置 replace_all=true。".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "要编辑的文件的绝对路径"
                        },
                        "old_string": {
                            "type": "string",
                            "description": "要查找并替换的精确文本，必须包含足够的上下文以确保唯一匹配"
                        },
                        "new_string": {
                            "type": "string",
                            "description": "替换后的文本（必须与 old_string 不同）"
                        },
                        "replace_all": {
                            "type": "boolean",
                            "default": false,
                            "description": "是否替换所有匹配项（默认 false，仅替换第一个唯一匹配）"
                        }
                    },
                    "required": ["file_path", "old_string", "new_string"]
                }),
            },
            BuiltinToolInfo {
                name: "list_directory".into(),
                description: "列出目录内容。支持 glob 模式过滤和递归列出。返回文件名、路径、类型、大小和修改时间。结果按修改时间排序（最新的在前）。".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "要列出的目录的绝对路径"
                        },
                        "pattern": {
                            "type": "string",
                            "description": "可选的 glob 模式过滤，如 '*.js'、'**/*.ts'、'src/**/*.{ts,tsx}'"
                        },
                        "recursive": {
                            "type": "boolean",
                            "default": false,
                            "description": "是否递归列出子目录"
                        }
                    },
                    "required": ["path"]
                }),
            },
            BuiltinToolInfo {
                name: "execute_bash".into(),
                description: "执行 Shell 命令。根据操作系统自动选择 Shell（macOS/Linux: zsh/bash, Windows: PowerShell）。默认超时 2 分钟，最长 10 分钟。对于长时间运行的命令（如服务器、watch 模式），请设置 run_in_background=true，然后使用 get_bash_output 获取输出。".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "要执行的 Shell 命令"
                        },
                        "description": {
                            "type": "string",
                            "description": "命令的简短描述（5-10 个词），例如 'List files in current directory'"
                        },
                        "timeout": {
                            "type": "number",
                            "description": "超时时间（毫秒），默认 120000，最大 600000"
                        },
                        "run_in_background": {
                            "type": "boolean",
                            "default": false,
                            "description": "是否后台运行。后台运行会返回 bash_id，稍后使用 get_bash_output 获取输出"
                        }
                    },
                    "required": ["command"]
                }),
            },
            BuiltinToolInfo {
                name: "get_bash_output".into(),
                description: "获取后台运行的命令的输出。返回自上次调用以来的新增输出（增量输出）。可以使用正则表达式过滤输出，但过滤后的行将不再可用。".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "bash_id": {
                            "type": "string",
                            "description": "execute_bash 返回的后台任务 ID"
                        },
                        "filter": {
                            "type": "string",
                            "description": "可选的正则表达式，用于过滤输出行。注意：不匹配的行将被永久丢弃"
                        }
                    },
                    "required": ["bash_id"]
                }),
            },
        ],
        Some("dynamic_mcp") => vec![
            BuiltinToolInfo {
                name: "load_mcp_server".into(),
                description: "根据需求关键词检索 MCP 工具集目录，并返回对应工具集下的工具摘要。".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "要检索的工具集名称或关键词"
                        }
                    },
                    "required": ["name"]
                }),
            },
            BuiltinToolInfo {
                name: "load_mcp_tool".into(),
                description: "按关键词加载 MCP 工具到当前会话，并返回这些工具的完整定义（含 description 与 parameters schema）。".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "names": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "需要加载的工具关键词列表，可一次传入多个"
                        },
                        "server_name": {
                            "type": "string",
                            "description": "可选。限定在指定工具集（参数名为 server_name）下搜索工具"
                        }
                    },
                    "required": ["names"]
                }),
            },
        ],
        _ => vec![],
    }
}

/// 初始化所有内置工具集到数据库（如果不存在）
/// 此函数应在应用启动时调用，确保内置工具集始终存在
#[instrument(skip(app_handle))]
pub fn init_builtin_mcp_servers(app_handle: &AppHandle) -> Result<()> {
    use tracing::info;

    let db = MCPDatabase::new(app_handle).context("Create MCPDatabase failed")?;
    let templates = builtin_templates();

    for tpl in templates {
        // 检查是否已存在该内置工具集（通过 command 匹配）
        let exists = db.conn
            .prepare("SELECT id FROM mcp_server WHERE command = ? AND is_builtin = 1 AND is_deletable = 0")?
            .query_row([&tpl.command], |row| row.get::<_, i64>(0))
            .optional()?;

        if exists.is_none() {
            info!(template_id = %tpl.id, name = %tpl.name, "Initializing builtin MCP server");

            // 插入内置工具集（系统初始化的不可删除）
            let server_id = db
                .upsert_mcp_server_with_builtin(
                    &tpl.name,              // name
                    Some(&tpl.description), // description
                    &tpl.transport_type,    // transport_type
                    Some(&tpl.command),     // command
                    None,                   // environment_variables (用户可以后续配置)
                    None,                   // headers
                    None,                   // url (builtins use stdio)
                    Some(20000),            // timeout
                    false,                  // is_long_running
                    true,                   // is_enabled
                    true,                   // is_builtin
                    false,                  // is_deletable - 系统初始化的不可删除
                    false,                  // proxy_enabled - builtin 不使用代理
                )
                .context("Insert builtin server failed")?;

            // 注册工具
            for tool in get_builtin_tools_for_command(&tpl.command) {
                db.upsert_mcp_server_tool(
                    server_id,
                    &tool.name,
                    Some(&tool.description),
                    Some(&tool.input_schema.to_string()),
                )
                .with_context(|| format!("Insert server tool failed: {}", tool.name))?;
            }

            info!(template_id = %tpl.id, server_id = server_id, "Builtin MCP server initialized");
        }
    }

    Ok(())
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
    timeout: Option<i32>,
) -> Result<i64, String> {
    (|| -> Result<i64> {
        let templates = builtin_templates();
        let tpl = templates
            .into_iter()
            .find(|t| t.id == template_id)
            .with_context(|| format!("Unknown builtin template id: {}", template_id))?;

        // 使用传入的 timeout，如果没有传入则使用模板的默认值
        let final_timeout = timeout.or(tpl.default_timeout);

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
                final_timeout,                                     // timeout
                false,                                             // is_long_running
                true,                                              // is_enabled
                true,                                              // is_builtin
                true,  // is_deletable - 用户添加的可删除
                false, // proxy_enabled - builtin 不使用代理
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
    fn test_builtin_templates_agent_exists() {
        let templates = builtin_templates();
        let agent = templates.iter().find(|t| t.id == "agent");
        assert!(agent.is_some(), "Agent template should exist");
    }

    #[test]
    fn test_agent_template_has_required_fields() {
        let templates = builtin_templates();
        let agent = templates.iter().find(|t| t.id == "agent").unwrap();

        assert!(!agent.name.is_empty());
        assert!(!agent.description.is_empty());
        assert_eq!(agent.command, "aipp:agent");
        assert_eq!(agent.transport_type, "stdio");
        // Agent has no required env vars
        assert!(agent.required_envs.is_empty());
    }

    #[test]
    fn test_get_tools_for_agent_command() {
        let tools = get_builtin_tools_for_command("aipp:agent");
        assert_eq!(tools.len(), 1, "Agent command should have 1 tool");
    }

    #[test]
    fn test_agent_load_skill_tool_exists() {
        let tools = get_builtin_tools_for_command("aipp:agent");
        let load_skill = tools.iter().find(|t| t.name == "load_skill");
        assert!(load_skill.is_some(), "load_skill tool should exist");
    }

    #[test]
    fn test_agent_load_skill_tool_schema() {
        let tools = get_builtin_tools_for_command("aipp:agent");
        let load_skill = tools.iter().find(|t| t.name == "load_skill").unwrap();

        assert!(!load_skill.description.is_empty());

        let schema = &load_skill.input_schema;
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["command"].is_object());
        assert!(schema["properties"]["source_type"].is_object());

        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|r| r == "command"));
        assert!(required.iter().any(|r| r == "source_type"));
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

        assert!(
            !search.required_envs.is_empty(),
            "Search template should have environment variables"
        );
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
        let valid_types = ["text", "select", "boolean", "number", "textarea"];

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
