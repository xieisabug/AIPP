use async_trait::async_trait;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::db::mcp_db::{MCPDatabase, MCPServer, MCPServerTool};
use crate::mcp::builtin_mcp::superadmin::registry::{ActionHandler, ActionRegistry};
use crate::mcp::builtin_mcp::superadmin::types::*;

struct McpListHandler;
struct McpGetHandler;
struct McpToggleHandler;
struct McpAddHandler;
struct McpUpdateHandler;

fn parse_env_var_keys(raw: Option<&str>) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(raw) = raw {
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, _)) = line.split_once('=') {
                let key = key.trim();
                if !key.is_empty() {
                    keys.push(key.to_string());
                }
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn parse_header_keys(raw: Option<&str>) -> Vec<String> {
    let mut keys = match raw {
        Some(raw) => match serde_json::from_str::<Value>(raw) {
            Ok(Value::Object(map)) => map.keys().cloned().collect(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    keys.sort();
    keys.dedup();
    keys
}

fn json_from_str_or_text(raw: Option<&str>) -> Value {
    match raw {
        Some(raw) => serde_json::from_str::<Value>(raw).unwrap_or_else(|_| json!(raw)),
        None => Value::Null,
    }
}

fn tool_to_json(tool: &MCPServerTool) -> Value {
    json!({
        "id": tool.id,
        "server_id": tool.server_id,
        "tool_name": tool.tool_name,
        "tool_description": tool.tool_description,
        "is_enabled": tool.is_enabled,
        "is_auto_run": tool.is_auto_run,
        "parameters": json_from_str_or_text(tool.parameters.as_deref()),
    })
}

fn server_overview_json(server: &MCPServer, tools: &[MCPServerTool]) -> Value {
    let environment_variable_keys = parse_env_var_keys(server.environment_variables.as_deref());
    let header_keys = parse_header_keys(server.headers.as_deref());
    let enabled_tool_count = tools.iter().filter(|tool| tool.is_enabled).count();

    json!({
        "id": server.id,
        "name": server.name,
        "description": server.description,
        "transport_type": server.transport_type,
        "command": server.command,
        "url": server.url,
        "timeout": server.timeout,
        "is_long_running": server.is_long_running,
        "is_enabled": server.is_enabled,
        "is_builtin": server.is_builtin,
        "is_deletable": server.is_deletable,
        "proxy_enabled": server.proxy_enabled,
        "created_time": server.created_time,
        "has_environment_variables": server.environment_variables.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "environment_variable_keys": environment_variable_keys,
        "has_headers": server.headers.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "header_keys": header_keys,
        "tool_count": tools.len(),
        "enabled_tool_count": enabled_tool_count,
    })
}

fn server_detail_json(server: &MCPServer, tools: &[MCPServerTool]) -> Value {
    let mut detail = server_overview_json(server, tools);
    let tool_items: Vec<Value> = tools.iter().map(tool_to_json).collect();
    detail["tools"] = json!(tool_items);
    detail
}

fn server_snapshot(server: &MCPServer) -> Value {
    json!({
        "_type": "mcp.server",
        "server_id": server.id,
        "name": server.name,
        "description": server.description,
        "transport_type": server.transport_type,
        "command": server.command,
        "environment_variables": server.environment_variables,
        "headers": server.headers,
        "url": server.url,
        "timeout": server.timeout,
        "is_long_running": server.is_long_running,
        "is_enabled": server.is_enabled,
        "is_builtin": server.is_builtin,
        "is_deletable": server.is_deletable,
        "proxy_enabled": server.proxy_enabled,
    })
}

fn string_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| format!("Missing required parameter: {key}"))
}

fn optional_nullable_string_arg(args: &Value, key: &str) -> Result<Option<Option<String>>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(value)) => Ok(Some(Some(value.clone()))),
        Some(_) => Err(format!("Invalid parameter: {key} must be string or null")),
    }
}

fn optional_bool_arg(args: &Value, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("Invalid parameter: {key} must be boolean")),
    }
}

fn optional_i32_arg(args: &Value, key: &str) -> Result<Option<Option<i32>>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::Number(value)) => {
            let parsed = value
                .as_i64()
                .ok_or_else(|| format!("Invalid parameter: {key} must be an integer"))?;
            let timeout = i32::try_from(parsed)
                .map_err(|_| format!("Invalid parameter: {key} out of range"))?;
            Ok(Some(Some(timeout)))
        }
        Some(_) => Err(format!("Invalid parameter: {key} must be integer or null")),
    }
}

fn server_id_arg(args: &Value) -> Result<i64, String> {
    args.get("server_id")
        .and_then(|value| value.as_i64())
        .ok_or("Missing required parameter: server_id".to_string())
}

#[async_trait]
impl ActionHandler for McpListHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        _args: Value,
        _dry_run: bool,
    ) -> Result<Value, String> {
        let db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let servers = db.get_mcp_servers().map_err(|e| e.to_string())?;
        let server_ids: Vec<i64> = servers.iter().map(|server| server.id).collect();
        let servers_with_tools =
            db.get_mcp_servers_with_tools_by_ids(&server_ids).map_err(|e| e.to_string())?;

        let items: Vec<Value> = servers_with_tools
            .iter()
            .map(|(server, tools)| server_overview_json(server, tools))
            .collect();

        Ok(json!({ "servers": items, "count": items.len() }))
    }
}

#[async_trait]
impl ActionHandler for McpGetHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        _dry_run: bool,
    ) -> Result<Value, String> {
        let server_id = server_id_arg(&args)?;
        let db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let server = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;
        let tools = db.get_mcp_server_tools(server_id).map_err(|e| e.to_string())?;

        Ok(server_detail_json(&server, &tools))
    }
}

#[async_trait]
impl ActionHandler for McpToggleHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        dry_run: bool,
    ) -> Result<Value, String> {
        let server_id = server_id_arg(&args)?;
        let is_enabled = args
            .get("is_enabled")
            .and_then(|value| value.as_bool())
            .ok_or("Missing required parameter: is_enabled")?;

        let db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let server = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "server_id": server_id,
                "current_enabled": server.is_enabled,
                "target_enabled": is_enabled,
            }));
        }

        db.toggle_mcp_server(server_id, is_enabled).map_err(|e| e.to_string())?;
        db.rebuild_dynamic_mcp_catalog().map_err(|e| e.to_string())?;

        Ok(json!({
            "server_id": server_id,
            "is_enabled": is_enabled,
        }))
    }

    async fn snapshot_before(&self, app_handle: &AppHandle, args: &Value) -> Option<Value> {
        let server_id = args.get("server_id")?.as_i64()?;
        let db = MCPDatabase::new(app_handle).ok()?;
        let server = db.get_mcp_server(server_id).ok()?;
        Some(json!({
            "_type": "mcp.toggle",
            "server_id": server_id,
            "is_enabled": server.is_enabled,
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        snapshot: &Value,
        _original_args: &Value,
    ) -> Result<Value, String> {
        let server_id = snapshot
            .get("server_id")
            .and_then(|value| value.as_i64())
            .ok_or("Missing server_id in snapshot")?;
        let is_enabled = snapshot
            .get("is_enabled")
            .and_then(|value| value.as_bool())
            .ok_or("Missing is_enabled in snapshot")?;

        let db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
        db.toggle_mcp_server(server_id, is_enabled).map_err(|e| e.to_string())?;
        db.rebuild_dynamic_mcp_catalog().map_err(|e| e.to_string())?;

        Ok(json!({
            "undone": true,
            "server_id": server_id,
            "restored_enabled": is_enabled,
        }))
    }
}

#[async_trait]
impl ActionHandler for McpAddHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        dry_run: bool,
    ) -> Result<Value, String> {
        let name = string_arg(&args, "name")?;
        let transport_type = string_arg(&args, "transport_type")?;
        let description = optional_nullable_string_arg(&args, "description")?.flatten();
        let command = optional_nullable_string_arg(&args, "command")?.flatten();
        let environment_variables =
            optional_nullable_string_arg(&args, "environment_variables")?.flatten();
        let headers = optional_nullable_string_arg(&args, "headers")?.flatten();
        let url = optional_nullable_string_arg(&args, "url")?.flatten();
        let timeout = optional_i32_arg(&args, "timeout")?.flatten();
        let is_long_running = optional_bool_arg(&args, "is_long_running")?.unwrap_or(false);
        let is_enabled = optional_bool_arg(&args, "is_enabled")?.unwrap_or(true);
        let is_builtin = optional_bool_arg(&args, "is_builtin")?.unwrap_or(false);
        let proxy_enabled = optional_bool_arg(&args, "proxy_enabled")?.unwrap_or(false);

        let db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let existing = db.get_mcp_servers().map_err(|e| e.to_string())?;
        if existing.iter().any(|server| server.name == name) {
            return Err(format!("MCP server with name '{name}' already exists"));
        }

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "would_create": {
                    "name": name,
                    "description": description,
                    "transport_type": transport_type,
                    "command": command,
                    "url": url,
                    "timeout": timeout,
                    "is_long_running": is_long_running,
                    "is_enabled": is_enabled,
                    "is_builtin": is_builtin,
                    "proxy_enabled": proxy_enabled,
                    "environment_variable_keys": parse_env_var_keys(environment_variables.as_deref()),
                    "header_keys": parse_header_keys(headers.as_deref()),
                }
            }));
        }

        let server_id = db
            .upsert_mcp_server_with_builtin(
                &name,
                description.as_deref(),
                &transport_type,
                command.as_deref(),
                environment_variables.as_deref(),
                headers.as_deref(),
                url.as_deref(),
                timeout,
                is_long_running,
                is_enabled,
                is_builtin,
                true,
                proxy_enabled,
            )
            .map_err(|e| e.to_string())?;
        db.rebuild_dynamic_mcp_catalog().map_err(|e| e.to_string())?;

        let server = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;
        let tools = db.get_mcp_server_tools(server_id).map_err(|e| e.to_string())?;

        Ok(json!({
            "server_id": server_id,
            "server": server_detail_json(&server, &tools),
        }))
    }

    async fn snapshot_before(&self, _app_handle: &AppHandle, args: &Value) -> Option<Value> {
        let name = args.get("name")?.as_str()?;
        Some(json!({
            "_type": "mcp.add",
            "name": name,
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        _snapshot: &Value,
        original_args: &Value,
    ) -> Result<Value, String> {
        let name = original_args
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or("Missing name in original_args")?;

        let db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let server = db
            .get_mcp_servers()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|server| server.name == name)
            .ok_or_else(|| format!("MCP server not found for undo: {name}"))?;

        db.delete_mcp_server(server.id).map_err(|e| e.to_string())?;
        db.rebuild_dynamic_mcp_catalog().map_err(|e| e.to_string())?;

        Ok(json!({
            "undone": true,
            "deleted_server_id": server.id,
            "name": name,
        }))
    }
}

#[async_trait]
impl ActionHandler for McpUpdateHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        dry_run: bool,
    ) -> Result<Value, String> {
        let server_id = server_id_arg(&args)?;
        let db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let server = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;

        let mut updated_fields = Vec::new();

        let mut name = server.name.clone();
        if let Some(next_name) = optional_nullable_string_arg(&args, "name")? {
            name = next_name.ok_or("Invalid parameter: name cannot be null")?;
            updated_fields.push("name");
        }

        let mut description =
            if server.description.is_empty() { None } else { Some(server.description.clone()) };
        if let Some(next_description) = optional_nullable_string_arg(&args, "description")? {
            description = next_description;
            updated_fields.push("description");
        }

        let mut transport_type = server.transport_type.clone();
        if let Some(next_transport_type) = optional_nullable_string_arg(&args, "transport_type")? {
            transport_type =
                next_transport_type.ok_or("Invalid parameter: transport_type cannot be null")?;
            updated_fields.push("transport_type");
        }

        let mut command = server.command.clone();
        if let Some(next_command) = optional_nullable_string_arg(&args, "command")? {
            command = next_command;
            updated_fields.push("command");
        }

        let mut environment_variables = server.environment_variables.clone();
        if let Some(next_environment_variables) =
            optional_nullable_string_arg(&args, "environment_variables")?
        {
            environment_variables = next_environment_variables;
            updated_fields.push("environment_variables");
        }

        let mut headers = server.headers.clone();
        if let Some(next_headers) = optional_nullable_string_arg(&args, "headers")? {
            headers = next_headers;
            updated_fields.push("headers");
        }

        let mut url = server.url.clone();
        if let Some(next_url) = optional_nullable_string_arg(&args, "url")? {
            url = next_url;
            updated_fields.push("url");
        }

        let mut timeout = server.timeout;
        if let Some(next_timeout) = optional_i32_arg(&args, "timeout")? {
            timeout = next_timeout;
            updated_fields.push("timeout");
        }

        let mut is_long_running = server.is_long_running;
        if let Some(next_is_long_running) = optional_bool_arg(&args, "is_long_running")? {
            is_long_running = next_is_long_running;
            updated_fields.push("is_long_running");
        }

        let mut is_enabled = server.is_enabled;
        if let Some(next_is_enabled) = optional_bool_arg(&args, "is_enabled")? {
            is_enabled = next_is_enabled;
            updated_fields.push("is_enabled");
        }

        let mut is_builtin = server.is_builtin;
        if let Some(next_is_builtin) = optional_bool_arg(&args, "is_builtin")? {
            is_builtin = next_is_builtin;
            updated_fields.push("is_builtin");
        }

        let mut proxy_enabled = server.proxy_enabled;
        if let Some(next_proxy_enabled) = optional_bool_arg(&args, "proxy_enabled")? {
            proxy_enabled = next_proxy_enabled;
            updated_fields.push("proxy_enabled");
        }

        let duplicate_name = db
            .get_mcp_servers()
            .map_err(|e| e.to_string())?
            .into_iter()
            .any(|candidate| candidate.id != server_id && candidate.name == name);
        if duplicate_name {
            return Err(format!("Another MCP server already uses name '{name}'"));
        }

        if dry_run {
            let preview = MCPServer {
                id: server.id,
                name,
                description: description.clone().unwrap_or_default(),
                transport_type,
                command: command.clone(),
                environment_variables: environment_variables.clone(),
                headers: headers.clone(),
                url: url.clone(),
                timeout,
                is_long_running,
                is_enabled,
                is_builtin,
                is_deletable: server.is_deletable,
                proxy_enabled,
                created_time: server.created_time.clone(),
            };
            let tools = db.get_mcp_server_tools(server_id).map_err(|e| e.to_string())?;
            return Ok(json!({
                "dry_run": true,
                "server_id": server_id,
                "updated_fields": updated_fields,
                "would_update_to": server_detail_json(&preview, &tools),
            }));
        }

        db.update_mcp_server_with_builtin(
            server_id,
            &name,
            description.as_deref(),
            &transport_type,
            command.as_deref(),
            environment_variables.as_deref(),
            headers.as_deref(),
            url.as_deref(),
            timeout,
            is_long_running,
            is_enabled,
            is_builtin,
            proxy_enabled,
        )
        .map_err(|e| e.to_string())?;
        db.rebuild_dynamic_mcp_catalog().map_err(|e| e.to_string())?;

        let updated = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;
        let tools = db.get_mcp_server_tools(server_id).map_err(|e| e.to_string())?;

        Ok(json!({
            "server_id": server_id,
            "updated_fields": updated_fields,
            "server": server_detail_json(&updated, &tools),
        }))
    }

    async fn snapshot_before(&self, app_handle: &AppHandle, args: &Value) -> Option<Value> {
        let server_id = args.get("server_id")?.as_i64()?;
        let db = MCPDatabase::new(app_handle).ok()?;
        let server = db.get_mcp_server(server_id).ok()?;
        Some(server_snapshot(&server))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        snapshot: &Value,
        _original_args: &Value,
    ) -> Result<Value, String> {
        let server_id = snapshot
            .get("server_id")
            .and_then(|value| value.as_i64())
            .ok_or("Missing server_id in snapshot")?;
        let name = snapshot
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or("Missing name in snapshot")?;
        let transport_type = snapshot
            .get("transport_type")
            .and_then(|value| value.as_str())
            .ok_or("Missing transport_type in snapshot")?;
        let description = snapshot
            .get("description")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty());
        let command = snapshot.get("command").and_then(|value| value.as_str());
        let environment_variables =
            snapshot.get("environment_variables").and_then(|value| value.as_str());
        let headers = snapshot.get("headers").and_then(|value| value.as_str());
        let url = snapshot.get("url").and_then(|value| value.as_str());
        let timeout = snapshot
            .get("timeout")
            .and_then(|value| value.as_i64())
            .and_then(|value| i32::try_from(value).ok());
        let is_long_running = snapshot
            .get("is_long_running")
            .and_then(|value| value.as_bool())
            .ok_or("Missing is_long_running in snapshot")?;
        let is_enabled = snapshot
            .get("is_enabled")
            .and_then(|value| value.as_bool())
            .ok_or("Missing is_enabled in snapshot")?;
        let is_builtin = snapshot
            .get("is_builtin")
            .and_then(|value| value.as_bool())
            .ok_or("Missing is_builtin in snapshot")?;
        let proxy_enabled = snapshot
            .get("proxy_enabled")
            .and_then(|value| value.as_bool())
            .ok_or("Missing proxy_enabled in snapshot")?;

        let db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
        db.update_mcp_server_with_builtin(
            server_id,
            name,
            description,
            transport_type,
            command,
            environment_variables,
            headers,
            url,
            timeout,
            is_long_running,
            is_enabled,
            is_builtin,
            proxy_enabled,
        )
        .map_err(|e| e.to_string())?;
        db.rebuild_dynamic_mcp_catalog().map_err(|e| e.to_string())?;

        Ok(json!({
            "undone": true,
            "server_id": server_id,
            "restored": "mcp server configuration",
        }))
    }
}

pub fn register(registry: &mut ActionRegistry) {
    registry.register(
        ActionMeta {
            action_id: "mcp.list".into(),
            domain: "mcp".into(),
            summary: "列出 MCP 服务".into(),
            description: "返回所有 MCP 服务及其启用状态、工具数量和安全摘要。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["mcp".into(), "read".into(), "list".into()],
            args_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "servers": { "type": "array" },
                    "count": { "type": "integer" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(McpListHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "mcp.get".into(),
            domain: "mcp".into(),
            summary: "获取 MCP 服务详情".into(),
            description: "获取单个 MCP 服务详情，包含工具列表和脱敏后的环境/请求头摘要。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["mcp".into(), "read".into(), "detail".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "server_id": { "type": "integer", "description": "MCP 服务 ID" }
                },
                "required": ["server_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "name": { "type": "string" },
                    "tools": { "type": "array" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(McpGetHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "mcp.toggle".into(),
            domain: "mcp".into(),
            summary: "切换 MCP 服务启用状态".into(),
            description: "启用或禁用指定的 MCP 服务。".into(),
            risk_level: RiskLevel::LOW,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["mcp".into(), "write".into(), "toggle".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "server_id": { "type": "integer", "description": "MCP 服务 ID" },
                    "is_enabled": { "type": "boolean", "description": "目标启用状态" }
                },
                "required": ["server_id", "is_enabled"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "server_id": { "type": "integer" },
                    "is_enabled": { "type": "boolean" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可再次调用 mcp.toggle 恢复原状态".into()),
        },
        Box::new(McpToggleHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "mcp.add".into(),
            domain: "mcp".into(),
            summary: "新增 MCP 服务".into(),
            description: "新增一个 MCP 服务配置，并重建动态 MCP 目录。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["mcp".into(), "write".into(), "create".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "MCP 服务名称" },
                    "description": { "type": ["string", "null"], "description": "描述（可选）" },
                    "transport_type": { "type": "string", "description": "传输类型，如 stdio/http" },
                    "command": { "type": ["string", "null"], "description": "stdio 命令（可选）" },
                    "environment_variables": { "type": ["string", "null"], "description": "环境变量文本（可选）" },
                    "headers": { "type": ["string", "null"], "description": "请求头 JSON 字符串（可选）" },
                    "url": { "type": ["string", "null"], "description": "HTTP URL（可选）" },
                    "timeout": { "type": ["integer", "null"], "description": "超时时间毫秒（可选）" },
                    "is_long_running": { "type": "boolean", "description": "是否为长连接服务（可选）" },
                    "is_enabled": { "type": "boolean", "description": "是否启用（可选，默认 true）" },
                    "is_builtin": { "type": "boolean", "description": "是否标记为内置（可选）" },
                    "proxy_enabled": { "type": "boolean", "description": "是否启用全局代理（可选）" }
                },
                "required": ["name", "transport_type"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "server_id": { "type": "integer" },
                    "server": { "type": "object" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可通过 superadmin_undo 撤销新增。".into()),
        },
        Box::new(McpAddHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "mcp.update".into(),
            domain: "mcp".into(),
            summary: "更新 MCP 服务".into(),
            description: "更新指定 MCP 服务的配置，未提供的字段保持原值。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["mcp".into(), "write".into(), "update".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "server_id": { "type": "integer", "description": "MCP 服务 ID" },
                    "name": { "type": ["string", "null"], "description": "新名称（可选）" },
                    "description": { "type": ["string", "null"], "description": "新描述（可选）" },
                    "transport_type": { "type": ["string", "null"], "description": "新传输类型（可选）" },
                    "command": { "type": ["string", "null"], "description": "新命令（可选）" },
                    "environment_variables": { "type": ["string", "null"], "description": "新环境变量文本（可选）" },
                    "headers": { "type": ["string", "null"], "description": "新请求头 JSON（可选）" },
                    "url": { "type": ["string", "null"], "description": "新 URL（可选）" },
                    "timeout": { "type": ["integer", "null"], "description": "新超时毫秒（可选）" },
                    "is_long_running": { "type": "boolean", "description": "是否长连接（可选）" },
                    "is_enabled": { "type": "boolean", "description": "是否启用（可选）" },
                    "is_builtin": { "type": "boolean", "description": "是否标记为内置（可选）" },
                    "proxy_enabled": { "type": "boolean", "description": "是否启用代理（可选）" }
                },
                "required": ["server_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "server_id": { "type": "integer" },
                    "updated_fields": { "type": "array" },
                    "server": { "type": "object" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可通过 superadmin_undo 恢复更新前配置。".into()),
        },
        Box::new(McpUpdateHandler),
    );
}
