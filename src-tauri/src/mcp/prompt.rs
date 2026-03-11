use crate::api::assistant_api::{
    get_assistant_field_value, get_assistant_mcp_servers_with_tools, MCPServerWithTools,
    MCPToolInfo,
};
use crate::db::mcp_db::MCPDatabase;
use crate::errors::AppError;
use crate::mcp::builtin_mcp::get_builtin_tools_for_command;
use std::collections::{HashMap, HashSet};
use tauri::Manager;
use tracing::{debug, instrument, trace, warn};

#[derive(Debug, Clone)]
pub struct MCPInfoForAssistant {
    pub enabled_servers: Vec<MCPServerWithTools>,
    pub use_native_toolcall: bool,
    pub dynamic_loading_enabled: bool,
}

fn parse_bool(value: &str, default_value: bool) -> bool {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return default_value;
    }
    !(value == "false" || value == "0" || value == "off")
}

fn is_agent_server(command: Option<&str>) -> bool {
    command == Some("aipp:agent")
}

pub async fn is_dynamic_mcp_loading_enabled_for_assistant(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
) -> bool {
    let global_enabled =
        if let Some(feature_state) = app_handle.try_state::<crate::FeatureConfigState>() {
            let config_map = feature_state.config_feature_map.lock().await;
            config_map
                .get("experimental")
                .and_then(|cfg| cfg.get("dynamic_mcp_loading_enabled"))
                .map(|cfg| parse_bool(&cfg.value, false))
                .unwrap_or(false)
        } else {
            false
        };
    if !global_enabled {
        return false;
    }
    match get_assistant_field_value(app_handle.clone(), assistant_id, "dynamic_mcp_loading_enabled")
    {
        Ok(v) => parse_bool(&v, true),
        Err(_) => true,
    }
}

fn collect_all_enabled_servers_for_dynamic_mode(
    app_handle: &tauri::AppHandle,
) -> Result<Vec<MCPServerWithTools>, AppError> {
    let db = MCPDatabase::new(app_handle)
        .map_err(|e| AppError::DatabaseError(format!("Failed to open MCP database: {}", e)))?;
    let all_servers = db
        .get_mcp_servers()
        .map_err(|e| AppError::DatabaseError(format!("Failed to load MCP servers: {}", e)))?;
    let server_catalog = db.list_server_capability_catalog().map_err(|e| {
        AppError::DatabaseError(format!("Failed to load MCP server catalog: {}", e))
    })?;
    let tool_catalog = db
        .list_tool_catalog(None)
        .map_err(|e| AppError::DatabaseError(format!("Failed to load MCP tool catalog: {}", e)))?;

    let mut result = Vec::new();
    let mut loader_server_ids = HashSet::new();
    for server in &all_servers {
        if !server.is_enabled {
            continue;
        }
        if !is_agent_server(server.command.as_deref()) {
            continue;
        }
        let tools = db
            .get_mcp_server_tools(server.id)
            .map_err(|e| {
                AppError::DatabaseError(format!("Failed to load MCP server tools: {}", e))
            })?
            .into_iter()
            .filter(|tool| tool.is_enabled)
            .map(|tool| MCPToolInfo {
                id: tool.id,
                name: tool.tool_name,
                description: tool.tool_description.unwrap_or_default(),
                is_enabled: tool.is_enabled,
                is_auto_run: tool.is_auto_run,
                parameters: tool.parameters.unwrap_or_else(|| "{}".to_string()),
            })
            .collect::<Vec<_>>();
        if tools.is_empty() {
            continue;
        }
        loader_server_ids.insert(server.id);
        result.push(MCPServerWithTools {
            id: server.id,
            name: server.name.clone(),
            summary: String::new(),
            command: server.command.clone(),
            is_enabled: true,
            tools,
        });
    }

    let server_summary_map: HashMap<i64, String> = server_catalog
        .iter()
        .filter(|server| server.summary_generated_at.is_some())
        .map(|server| (server.server_id, server.summary.clone()))
        .collect();
    let summarized_server_ids: Vec<i64> = server_summary_map.keys().copied().collect();
    if summarized_server_ids.is_empty() {
        return Ok(result);
    }

    let summary_map: HashMap<(i64, String), String> = tool_catalog
        .into_iter()
        .filter(|tool| {
            tool.summary_generated_at.is_some() && tool.server_enabled && tool.tool_enabled
        })
        .map(|tool| ((tool.server_id, tool.tool_name.clone()), tool.summary))
        .collect();
    let servers_with_tools =
        db.get_mcp_servers_with_tools_by_ids(&summarized_server_ids).map_err(|e| {
            AppError::DatabaseError(format!("Failed to load summarized MCP tools: {}", e))
        })?;
    for (server, tools) in servers_with_tools {
        if !server.is_enabled
            || server.command.as_deref() == Some("aipp:dynamic_mcp")
            || loader_server_ids.contains(&server.id)
        {
            continue;
        }
        let tools: Vec<MCPToolInfo> = tools
            .into_iter()
            .filter(|tool| tool.is_enabled)
            .filter_map(|tool| {
                summary_map.get(&(server.id, tool.tool_name.clone())).map(|summary| MCPToolInfo {
                    id: tool.id,
                    name: tool.tool_name,
                    description: summary.clone(),
                    is_enabled: true,
                    is_auto_run: tool.is_auto_run,
                    parameters: tool.parameters.unwrap_or_else(|| "{}".to_string()),
                })
            })
            .collect();

        if tools.is_empty() {
            continue;
        }

        result.push(MCPServerWithTools {
            id: server.id,
            name: server.name,
            summary: server_summary_map.get(&server.id).cloned().unwrap_or_default(),
            command: server.command,
            is_enabled: true,
            tools,
        });
    }

    Ok(result)
}

#[instrument(level = "debug", skip(app_handle, mcp_override_config, enabled_servers_filter), fields(assistant_id, has_override = mcp_override_config.is_some(), filter_servers = enabled_servers_filter.map(|v| v.len()).unwrap_or(0)))]
pub async fn collect_mcp_info_for_assistant(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
    mcp_override_config: Option<&crate::api::ai::types::McpOverrideConfig>,
    enabled_servers_filter: Option<&Vec<String>>, // 可选过滤：服务器名称或ID（字符串）
) -> Result<MCPInfoForAssistant, AppError> {
    let dynamic_loading_enabled =
        is_dynamic_mcp_loading_enabled_for_assistant(app_handle, assistant_id).await;
    let use_native_toolcall = match crate::api::assistant_api::get_assistant_field_value(
        app_handle.clone(),
        assistant_id,
        "use_native_toolcall",
    ) {
        Ok(value) => value == "true",
        Err(e) => {
            warn!(error = %e, assistant_id, "Failed to get native toolcall config, using default (false)");
            false
        }
    };

    // Apply override configuration for use_native_toolcall if provided
    let final_use_native_toolcall = mcp_override_config
        .and_then(|config| config.use_native_toolcall)
        .unwrap_or(use_native_toolcall);

    let all_servers = if dynamic_loading_enabled {
        collect_all_enabled_servers_for_dynamic_mode(app_handle)?
    } else {
        get_assistant_mcp_servers_with_tools(app_handle.clone(), assistant_id)
            .await
            .map_err(|e| AppError::DatabaseError(format!("Failed to get MCP servers: {}", e)))?
    };
    debug!(total_servers = all_servers.len(), "Loaded assistant MCP servers");

    // 根据传入的 enabled_servers_filter 选择服务器：
    // - 如果提供了 ID 列表，就按该列表精确选择对应服务器（忽略助手层面的启用状态）
    // - 如果未提供，则只保留助手配置里启用的服务器
    let enabled_servers: Vec<MCPServerWithTools> = if let Some(filters) = enabled_servers_filter {
        if !filters.is_empty() {
            use std::collections::HashSet;
            let filter_set: HashSet<&String> = filters.iter().collect();

            // 1. 先加入助手"已启用"的服务器（基础集合的一部分）
            let mut picked: Vec<MCPServerWithTools> = Vec::new();
            let mut existing_id_set: HashSet<i64> = HashSet::new();
            for server in &all_servers {
                if server.is_enabled {
                    picked.push(server.clone());
                    existing_id_set.insert(server.id);
                }
            }

            // 2. 过滤列表里出现的（ID 或 名称匹配） -> 并集：即便未启用也加入
            for server in &all_servers {
                let id_str = server.id.to_string();
                if (filter_set.contains(&id_str) || filter_set.contains(&server.name))
                    && !existing_id_set.contains(&server.id)
                {
                    picked.push(server.clone());
                    existing_id_set.insert(server.id);
                }
            }

            // 3. filters 里额外的 ID（不在 all_servers 中）再批量查
            let mut extra_ids: Vec<i64> = Vec::new();
            for raw in filters.iter() {
                if let Ok(id_val) = raw.parse::<i64>() {
                    if !existing_id_set.contains(&id_val) {
                        extra_ids.push(id_val);
                    }
                }
            }

            if !extra_ids.is_empty() && !dynamic_loading_enabled {
                trace!(?extra_ids, "Fetching extra servers by id (filter not in assistant set)");
                if let Ok(db) = crate::db::mcp_db::MCPDatabase::new(app_handle) {
                    if let Ok(pairs) = db.get_mcp_servers_with_tools_by_ids(&extra_ids) {
                        for (srv, tools_raw) in pairs {
                            if existing_id_set.contains(&srv.id) {
                                continue;
                            }
                            // 只保留启用工具
                            let tools_converted: Vec<crate::api::assistant_api::MCPToolInfo> =
                                tools_raw
                                    .into_iter()
                                    .filter(|t| t.is_enabled)
                                    .map(|t| crate::api::assistant_api::MCPToolInfo {
                                        id: t.id,
                                        name: t.tool_name,
                                        description: t.tool_description.unwrap_or_default(),
                                        is_enabled: t.is_enabled,
                                        is_auto_run: t.is_auto_run,
                                        parameters: t
                                            .parameters
                                            .unwrap_or_else(|| "{}".to_string()),
                                    })
                                    .collect();
                            picked.push(MCPServerWithTools {
                                id: srv.id,
                                name: srv.name,
                                summary: String::new(),
                                command: srv.command.clone(),
                                is_enabled: srv.is_enabled,
                                tools: tools_converted,
                            });
                            existing_id_set.insert(srv.id);
                        }
                    }
                }
            }

            picked
        } else {
            // 空过滤列表 => 助手启用服务器
            all_servers.into_iter().filter(|s| s.is_enabled).collect()
        }
    } else {
        // 无过滤 => 助手启用服务器
        all_servers.into_iter().filter(|s| s.is_enabled).collect()
    };

    debug!(
        enabled_server_count = enabled_servers.len(),
        native_toolcall = final_use_native_toolcall,
        dynamic_loading_enabled,
        "Collected MCP info for assistant"
    );
    Ok(MCPInfoForAssistant {
        enabled_servers,
        use_native_toolcall: final_use_native_toolcall,
        dynamic_loading_enabled,
    })
}

#[instrument(level = "debug", skip(assistant_prompt_result, mcp_info), fields(server_count = mcp_info.enabled_servers.len()))]
pub async fn format_mcp_prompt(
    assistant_prompt_result: String,
    mcp_info: &MCPInfoForAssistant,
) -> String {
    format_mcp_prompt_with_filters(assistant_prompt_result, mcp_info, None, None).await
}

#[instrument(level = "trace", skip(assistant_prompt_result, mcp_info, enabled_servers, enabled_tools), fields(server_count = mcp_info.enabled_servers.len(), filter_servers = enabled_servers.map(|v| v.len()).unwrap_or(0)))]
pub async fn format_mcp_prompt_with_filters(
    assistant_prompt_result: String,
    mcp_info: &MCPInfoForAssistant,
    enabled_servers: Option<&Vec<String>>,
    enabled_tools: Option<&std::collections::HashMap<String, Vec<String>>>,
) -> String {
    if mcp_info.dynamic_loading_enabled {
        // 根据是否使用原生 toolcall 提供不同的 prompt
        let mcp_dynamic_prompt = if mcp_info.use_native_toolcall {
            r#"
# MCP 动态加载规范（严格遵守）

## 使用原则
1. 只能调用系统已加载的工具，禁止虚构工具名或参数
2. 仅在有助于完成任务时调用；能靠自身知识完成时不调用
3. 每条消息最多调用一个工具；如需多步骤，分多轮依次调用
4. **Agent 工具始终可用，无需加载；非 Agent 工具在调用前必须先通过 `load_mcp_tool` 加载**
5. 当用户明确要求使用某个 Skill / Agent 工作流时，应先调用 `load_skill` 读取该 Skill 的详细说明，再按说明执行

## 动态加载流程（必须按顺序执行）
1. **浏览目录**：查看下方"工具集目录摘要"，确定目标工具集
2. **查看工具列表**：调用 `load_mcp_server` 获取目标工具集的工具列表及摘要
3. **加载工具**：调用 `load_mcp_tool` 将目标工具加载到当前会话（推荐 `server::tool` 格式）
4. **调用工具**：工具加载成功后，在后续轮次直接调用该工具
5. 当工具调用失败提示"未加载"时，先 `load_mcp_tool` 再重试

⚠ 严禁跳过步骤直接调用未加载的工具——这样的调用会被系统忽略。
"#
        } else {
            r#"
# MCP 动态加载规范（严格遵守）

## 使用原则
1. 只能调用系统已加载的工具，禁止虚构工具名或参数
2. 仅在有助于完成任务时调用；能靠自身知识完成时不调用
3. 每条消息最多调用一个工具；如需多步骤，分多轮依次调用
4. 工具调用必须放在消息的最末尾，调用之后禁止再输出任何文字
5. **Agent 工具始终可用，无需加载；非 Agent 工具在调用前必须先通过 `load_mcp_tool` 加载**
6. 当用户明确要求使用某个 Skill / Agent 工作流时，应先调用 `load_skill` 读取该 Skill 的详细说明，再按说明执行

## 输出格式（强制）
调用工具时，必须使用下面的**裸 XML 格式**直接输出，禁止用 Markdown 代码块（```）包裹：

<mcp_tool_call>
  <server_name>工具集名称</server_name>
  <tool_name>工具名称</tool_name>
  <parameters>{"key":"value"}</parameters>
</mcp_tool_call>

### 格式要求
- `<mcp_tool_call>` 标签必须作为裸 XML 直接出现在消息中
- 参数必须是有效 JSON；无参数时写 `{}`
- 禁止伪造工具响应或猜测未返回的数据

### 错误示例（以下写法均无效，系统无法识别）
- ❌ 用代码块包裹：```xml <mcp_tool_call>...</mcp_tool_call> ```
- ❌ 工具调用后继续输出文字
- ❌ 调用未列出或未加载的工具名
- ❌ 使用原生的工具调用格式

## 动态加载流程（必须按顺序执行）
1. **浏览目录**：查看下方"工具集目录摘要"，确定目标工具集
2. **查看工具列表**：调用 `load_mcp_server` 获取目标工具集的工具列表及摘要
3. **加载工具**：调用 `load_mcp_tool` 将目标工具加载到当前会话（推荐 `server::tool` 格式）
4. **调用工具**：工具加载成功后，在后续轮次直接调用该工具
5. 当工具调用失败提示"未加载"时，先 `load_mcp_tool` 再重试

⚠ 严禁跳过步骤直接调用未加载的工具——这样的调用会被系统忽略。
"#
        };

        let mut load_tools_info = String::from("\n## Agent 工具（始终可用，无需加载）\n\n");
        load_tools_info.push_str("### 工具集: Agent\n\n");
        let mut has_load_tools = false;
        for server_details in &mcp_info.enabled_servers {
            if !is_agent_server(server_details.command.as_deref()) {
                continue;
            }
            for tool in &server_details.tools {
                let description = if tool.description.trim().is_empty() {
                    "暂无描述"
                } else {
                    tool.description.as_str()
                };
                let parameters =
                    if tool.parameters.trim().is_empty() { "{}" } else { tool.parameters.as_str() };
                load_tools_info.push_str(&format!("**{}** \n", tool.name));
                load_tools_info.push_str(&format!(" - description: {}\n", description));
                load_tools_info.push_str(&format!(" - parameters: {}\n\n", parameters));
                has_load_tools = true;
            }
        }
        if !has_load_tools {
            for tool in get_builtin_tools_for_command("aipp:agent") {
                load_tools_info.push_str(&format!("#### {} \n", tool.name));
                load_tools_info.push_str(&format!(" - description: {}\n", tool.description));
                load_tools_info.push_str(&format!(" - parameters: {}\n\n", tool.input_schema));
            }
        }
        // 只有非原生 toolcall 模式才显示 XML 范例
        if !mcp_info.use_native_toolcall {
            load_tools_info.push_str(
                r#"### 使用范例

加载某个 Skill 的详细说明：
<mcp_tool_call>
  <server_name>Agent</server_name>
  <tool_name>load_skill</tool_name>
  <parameters>{"command":"pdf","source_type":"AGENTS"}</parameters>
</mcp_tool_call>

查看 Search 工具集的工具列表：
<mcp_tool_call>
  <server_name>Agent</server_name>
  <tool_name>load_mcp_server</tool_name>
  <parameters>{"name":"Search"}</parameters>
</mcp_tool_call>

加载指定工具到会话（推荐 `server::tool` 格式）：
<mcp_tool_call>
  <server_name>Agent</server_name>
  <tool_name>load_mcp_tool</tool_name>
  <parameters>{"names":["Search::web_fetch"]}</parameters>
</mcp_tool_call>
"#,
            );
        }

        let mut tools_info = String::from("\n## MCP 工具集目录摘要\n\n");
        let mut has_toolset = false;
        for server_details in &mcp_info.enabled_servers {
            if server_details.command.as_deref() == Some("aipp:dynamic_mcp")
                || server_details.command.as_deref() == Some("aipp:agent")
            {
                continue;
            }
            if let Some(enabled_server_id) = enabled_servers {
                if !enabled_server_id.contains(&server_details.id.to_string()) {
                    continue;
                }
            }

            if let Some(enabled_tools_map) = enabled_tools {
                if !enabled_tools_map.contains_key(&server_details.name) {
                    continue;
                }
            }
            let summary = if server_details.summary.trim().is_empty() {
                "暂无摘要"
            } else {
                server_details.summary.as_str()
            };
            tools_info.push_str(&format!("- 工具集 `{}`：{}\n", server_details.name, summary));
            has_toolset = true;
        }
        if !has_toolset {
            tools_info.push_str("- 暂无可用工具集摘要，请先完成 MCP 工具集总结。\n");
        }

        return format!(
            "{}\n{}\n{}\n{}\n{}",
            "# 助手指令\n",
            assistant_prompt_result,
            mcp_dynamic_prompt,
            load_tools_info,
            tools_info
        );
    }

    let mcp_constraint_prompt: &str = r#"
# MCP 工具使用规范（严格遵守）

## 使用原则
1. 只能调用下方明确列出的工具，禁止虚构工具名或参数
2. 仅在有助于完成任务时调用；能靠自身知识完成时不调用
3. 每条消息最多调用一个工具；如需多步骤，分多轮依次调用
4. 工具调用必须放在消息的最末尾，调用之后禁止再输出任何文字

## 输出格式（强制）
调用工具时，必须使用下面的**裸 XML 格式**直接输出，禁止用 Markdown 代码块（```）包裹：

<mcp_tool_call>
  <server_name>服务器名称</server_name>
  <tool_name>工具名称</tool_name>
  <parameters>{"key":"value"}</parameters>
</mcp_tool_call>

### 格式要求
- `<mcp_tool_call>` 标签必须作为裸 XML 直接出现在消息中
- 参数必须是有效 JSON；无参数时写 `{}`
- 禁止伪造工具响应或猜测未返回的数据

### 错误示例（以下写法均无效，系统无法识别）
- ❌ 用代码块包裹：```xml <mcp_tool_call>...</mcp_tool_call> ```
- ❌ 工具调用后继续输出文字
- ❌ 一条消息中包含多个 `<mcp_tool_call>` 块
- ❌ 调用未列出的工具名
- ❌ 使用原生的工具调用格式
"#;

    let mut tools_info = String::from("\n## 可用的 MCP 工具\n\n");

    for server_details in &mcp_info.enabled_servers {
        // Check if this server is in the enabled servers list
        if let Some(enabled_server_id) = enabled_servers {
            if !enabled_server_id.contains(&server_details.id.to_string()) {
                continue;
            }
        }

        tools_info.push_str(&format!("### 服务器: {}\n", server_details.name));
        tools_info.push_str("\n#### 可用工具:\n\n");

        for tool in &server_details.tools {
            // Check if this tool is enabled for this server
            if let Some(enabled_tools_map) = enabled_tools {
                if let Some(allowed_tools) = enabled_tools_map.get(&server_details.name) {
                    if !allowed_tools.contains(&tool.name) {
                        continue;
                    }
                }
            }

            tools_info.push_str(&format!("**{}** \n", tool.name));
            tools_info.push_str(&format!(" - description: {}\n", tool.description));
            tools_info.push_str(&format!(" - parameters: {}\n", tool.parameters));
            tools_info.push_str("\n\n");
        }
        tools_info.push_str("\n---\n\n");
    }

    format!(
        "{}\n{}\n{}\n{}",
        "# 助手指令\n", assistant_prompt_result, mcp_constraint_prompt, tools_info
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::assistant_api::{MCPServerWithTools, MCPToolInfo};

    #[test]
    fn dynamic_mode_recognizes_load_skill_as_agent_loading_tool() {
        assert!(is_agent_server(Some("aipp:agent")));
        assert!(!is_agent_server(Some("aipp:search")));
        assert!(!is_agent_server(None));
    }

    #[tokio::test]
    async fn dynamic_prompt_fallback_introduces_load_skill() {
        let prompt = format_mcp_prompt_with_filters(
            "assistant prompt".to_string(),
            &MCPInfoForAssistant {
                enabled_servers: vec![MCPServerWithTools {
                    id: 1,
                    name: "Agent".to_string(),
                    summary: String::new(),
                    command: Some("aipp:agent".to_string()),
                    is_enabled: true,
                    tools: vec![],
                }],
                use_native_toolcall: false,
                dynamic_loading_enabled: true,
            },
            None,
            None,
        )
        .await;

        assert!(prompt.contains("load_skill"));
        assert!(prompt.contains("\"source_type\""));
        assert!(prompt.contains("todo_write"));
    }

    #[tokio::test]
    async fn dynamic_prompt_lists_load_skill_when_agent_tool_is_available() {
        let prompt = format_mcp_prompt_with_filters(
            "assistant prompt".to_string(),
            &MCPInfoForAssistant {
                enabled_servers: vec![MCPServerWithTools {
                    id: 1,
                    name: "Agent".to_string(),
                    summary: String::new(),
                    command: Some("aipp:agent".to_string()),
                    is_enabled: true,
                    tools: vec![MCPToolInfo {
                        id: 1,
                        name: "load_skill".to_string(),
                        description: "Load skill details".to_string(),
                        is_enabled: true,
                        is_auto_run: false,
                        parameters:
                            "{\"type\":\"object\",\"required\":[\"command\",\"source_type\"]}"
                                .to_string(),
                    }],
                }],
                use_native_toolcall: false,
                dynamic_loading_enabled: true,
            },
            None,
            None,
        )
        .await;

        assert!(prompt.contains("Load skill details"));
        assert!(prompt.contains("load_skill"));
    }
}
