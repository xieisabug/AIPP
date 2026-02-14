use crate::api::assistant_api::{
    get_assistant_field_value, get_assistant_mcp_servers_with_tools, MCPServerWithTools, MCPToolInfo,
};
use crate::db::mcp_db::MCPDatabase;
use crate::errors::AppError;
use std::collections::HashMap;
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

pub async fn is_dynamic_mcp_loading_enabled_for_assistant(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
) -> bool {
    let global_enabled = if let Some(feature_state) = app_handle.try_state::<crate::FeatureConfigState>() {
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
    match get_assistant_field_value(
        app_handle.clone(),
        assistant_id,
        "dynamic_mcp_loading_enabled",
    ) {
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
    let server_catalog = db
        .list_server_capability_catalog()
        .map_err(|e| AppError::DatabaseError(format!("Failed to load MCP server catalog: {}", e)))?;
    let tool_catalog = db
        .list_tool_catalog(None)
        .map_err(|e| AppError::DatabaseError(format!("Failed to load MCP tool catalog: {}", e)))?;

    let mut result = Vec::new();
    for server in &all_servers {
        let is_dynamic_builtin = server.command.as_deref() == Some("aipp:dynamic_mcp");
        if !server.is_enabled && !is_dynamic_builtin {
            continue;
        }
        if !is_dynamic_builtin {
            continue;
        }
        let tools = db
            .get_mcp_server_tools(server.id)
            .map_err(|e| AppError::DatabaseError(format!("Failed to load MCP server tools: {}", e)))?
            .into_iter()
            .filter(|tool| tool.tool_name == "load_mcp_server" || tool.tool_name == "load_mcp_tool")
            .map(|tool| MCPToolInfo {
                id: tool.id,
                name: tool.tool_name,
                description: tool.tool_description.unwrap_or_default(),
                is_enabled: true,
                is_auto_run: tool.is_auto_run,
                parameters: tool.parameters.unwrap_or_else(|| "{}".to_string()),
            })
            .collect::<Vec<_>>();
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
        .filter(|tool| tool.summary_generated_at.is_some() && tool.server_enabled && tool.tool_enabled)
        .map(|tool| ((tool.server_id, tool.tool_name.clone()), tool.summary))
        .collect();
    let servers_with_tools = db
        .get_mcp_servers_with_tools_by_ids(&summarized_server_ids)
        .map_err(|e| AppError::DatabaseError(format!("Failed to load summarized MCP tools: {}", e)))?;
    for (server, tools) in servers_with_tools {
        if !server.is_enabled || server.command.as_deref() == Some("aipp:dynamic_mcp") {
            continue;
        }
        let tools: Vec<MCPToolInfo> = tools
            .into_iter()
            .filter(|tool| tool.is_enabled)
            .filter_map(|tool| {
                summary_map
                    .get(&(server.id, tool.tool_name.clone()))
                    .map(|summary| MCPToolInfo {
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

            // 1. 先加入助手“已启用”的服务器（基础集合的一部分）
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
        let mcp_dynamic_prompt: &str = r#"
# MCP 动态加载规范（实验）

作为 AI 助手，你可以使用 MCP 工具来执行任务。请严格遵守以下规则：

## 使用原则
1. 你只能调用系统明确提供的 MCP 工具，不得虚构或调用未提及的工具
2. 仅在有助于完成任务时调用工具；能靠自身知识完成时不调用
3. 信任工具的返回结果（除非工具明确报错、超时或返回无效数据）
4. 一次只调用一个工具；如需多个步骤，请分多轮依次调用
5. 工具调用必须放在本条消息的最后

## 输出格式
当需要调用 MCP 工具时，请使用以下 XML 格式，注意不需要代码块包裹：

<mcp_tool_call>
  <server_name>工具集名称</server_name>
  <tool_name>工具名称</tool_name>
  <parameters>{"parameter1":"value1"}</parameters>
</mcp_tool_call>

## 动态加载流程
1. 先查看工具集摘要，再决定要深入哪个工具集；
2. 使用 `load_mcp_server` 获取目标工具集下的工具列表与工具摘要；
3. 使用 `load_mcp_tool` 将目标工具加载到当前会话；
4. 当调用某工具失败提示“未加载”时，先调用 `load_mcp_tool` 再重试；
5. 仅在工具已加载后再直接调用该工具。

## 重要注意事项
- 参数必须是有效的 JSON 格式
- 如果工具不需要参数，parameters 标签内应该为空对象 {}
- 不得伪造工具响应或猜测未返回的数据
"#;

        let mut load_tools_info = String::from("\n## 必备加载工具（始终可用）\n\n");
        load_tools_info.push_str("### 工具集: MCP 动态加载工具\n\n");
        let mut has_load_tools = false;
        for server_details in &mcp_info.enabled_servers {
            if server_details.command.as_deref() != Some("aipp:dynamic_mcp") {
                continue;
            }
            for tool in &server_details.tools {
                let description = if tool.description.trim().is_empty() {
                    "暂无描述"
                } else {
                    tool.description.as_str()
                };
                let parameters = if tool.parameters.trim().is_empty() {
                    "{}"
                } else {
                    tool.parameters.as_str()
                };
                load_tools_info.push_str(&format!("**{}** \n", tool.name));
                load_tools_info.push_str(&format!(" - description: {}\n", description));
                load_tools_info.push_str(&format!(" - parameters: {}\n\n", parameters));
                has_load_tools = true;
            }
        }
        if !has_load_tools {
            load_tools_info.push_str(
                "**load_mcp_server** \n - description: 根据关键词加载工具集的工具目录摘要\n - parameters: {\"type\":\"object\",\"properties\":{\"name\":{\"type\":\"string\",\"description\":\"要检索的工具集名称或关键词\"}},\"required\":[\"name\"]}\n\n",
            );
            load_tools_info.push_str(
                "**load_mcp_tool** \n - description: 按关键词加载 MCP 工具到当前会话，加载后后续轮次可直接调用\n - parameters: {\"type\":\"object\",\"properties\":{\"names\":{\"type\":\"array\",\"items\":{\"type\":\"string\"},\"description\":\"需要加载的工具关键词列表，可一次传入多个\"},\"server_name\":{\"type\":\"string\",\"description\":\"可选。限定在指定工具集（参数名为 server_name）下搜索工具\"}},\"required\":[\"names\"]}\n\n",
            );
        }
        load_tools_info.push_str(
            r#"### 使用范例
        
加入需要使用Agent相关的工具，先加载对应的server：
<mcp_tool_call>
  <server_name>MCP 动态加载工具</server_name>
  <tool_name>load_mcp_server</tool_name>
  <parameters>{"name":"Agent"}</parameters>
</mcp_tool_call>

再获取对应的工具详情：
<mcp_tool_call>
  <server_name>MCP 动态加载工具</server_name>
  <tool_name>load_mcp_tool</tool_name>
  <parameters>{"names":["load_skills"], "server_name":"Agent"}</parameters>
</mcp_tool_call>
            "#
        );

        let mut tools_info = String::from("\n## MCP 工具集目录摘要\n\n");
        let mut has_toolset = false;
        for server_details in &mcp_info.enabled_servers {
            if server_details.command.as_deref() == Some("aipp:dynamic_mcp") {
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
# MCP (Model Context Protocol) 工具使用规范

作为 AI 助手，你可以使用以下 MCP 工具来执行各种任务。请严格遵守以下规则：

## 使用原则
1. 你只能调用系统明确提供的 MCP 工具，不得虚构或调用未提及的工具
2. 仅在有助于完成任务时调用工具；能靠自身知识完成时不调用
3. 信任工具的返回结果（除非工具明确报错、超时或返回无效数据）
4. 一次只调用一个工具；如需多个步骤，请分多轮依次调用
5. 工具调用必须放在本条消息的最后

## 输出格式
当需要调用 MCP 工具时，请使用以下 XML 格式，注意不需要代码块包裹：

<mcp_tool_call>
  <server_name>服务器名称</server_name>
  <tool_name>工具名称</tool_name>
  <parameters>{"parameter1":"value1"}</parameters>
</mcp_tool_call>

## 重要注意事项
- 参数必须是有效的 JSON 格式
- 如果工具不需要参数，parameters 标签内应该为空对象 {}
- 不得伪造工具响应或猜测未返回的数据
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
