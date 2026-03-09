use super::{Bang, BangType, TemplateEngine};
use crate::api::plugin_api::{
    get_enabled_plugin_manifests, PluginBangArgumentSource, PluginBangArgumentSpec,
    PluginBangArgumentValueType, PluginBangContribution, PluginBangExecutor,
    PluginBangServerDefinition, ResolvedPluginManifest,
};
use crate::db::mcp_db::{MCPDatabase, MCPServer};
use crate::mcp::execution_api::execute_tool_by_transport;
use futures::FutureExt;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use tauri::Manager;
use tracing::warn;

const DEFAULT_BUILTIN_COMMAND: &str = "aipp:operation";

#[derive(Debug, Clone)]
struct ParsedBangArguments {
    raw: String,
    positional: Vec<String>,
    named: HashMap<String, String>,
}

#[derive(Clone)]
struct RuntimeBangExecutor {
    server: MCPServer,
    tool_name: String,
    arguments: HashMap<String, PluginBangArgumentSpec>,
}

pub fn build_template_engine(app_handle: &tauri::AppHandle) -> Result<TemplateEngine, String> {
    let mut engine = TemplateEngine::new();
    let manifests = get_enabled_plugin_manifests(app_handle)?;

    for manifest in manifests {
        if manifest.contributions.bangs.is_empty() {
            continue;
        }
        if !has_permission(&manifest.permissions, "bang.register") {
            warn!(plugin = %manifest.code, "Skipping bang contributions because plugin lacks bang.register permission");
            continue;
        }
        register_plugin_bangs(&mut engine, app_handle, &manifest);
    }

    Ok(engine)
}

fn register_plugin_bangs(
    engine: &mut TemplateEngine,
    app_handle: &tauri::AppHandle,
    manifest: &ResolvedPluginManifest,
) {
    for bang in &manifest.contributions.bangs {
        match register_single_bang(engine, app_handle, manifest, bang) {
            Ok(()) => {}
            Err(error) => {
                warn!(
                    plugin = %manifest.code,
                    bang = %bang.name,
                    error = %error,
                    "Skipping invalid bang contribution"
                );
            }
        }
    }
}

fn register_single_bang(
    engine: &mut TemplateEngine,
    app_handle: &tauri::AppHandle,
    manifest: &ResolvedPluginManifest,
    contribution: &PluginBangContribution,
) -> Result<(), String> {
    let primary_name = normalize_bang_name(&contribution.name)?;
    let executor = resolve_runtime_executor(app_handle, manifest, contribution)?;
    let bang_type = parse_bang_type(contribution.bang_type.as_deref());
    let description = contribution
        .description
        .clone()
        .unwrap_or_else(|| format!("{} 提供的 bang", manifest.name));
    let primary_complete =
        derive_completion(&primary_name, None, contribution, &executor.arguments);

    add_runtime_bang(
        engine,
        app_handle,
        &primary_name,
        &primary_complete,
        &description,
        bang_type.clone(),
        executor.clone(),
    );

    for alias in &contribution.aliases {
        let alias_name = normalize_bang_name(alias)?;
        let alias_complete = derive_completion(
            &primary_name,
            Some(alias_name.as_str()),
            contribution,
            &executor.arguments,
        );
        add_runtime_bang(
            engine,
            app_handle,
            &alias_name,
            &alias_complete,
            &description,
            bang_type.clone(),
            executor.clone(),
        );
    }

    Ok(())
}

fn add_runtime_bang(
    engine: &mut TemplateEngine,
    app_handle: &tauri::AppHandle,
    name: &str,
    complete: &str,
    description: &str,
    bang_type: BangType,
    executor: RuntimeBangExecutor,
) {
    if engine.has_command(name) {
        warn!(bang = %name, "Skipping duplicated bang registration");
        return;
    }

    let app_handle = app_handle.clone();
    let executor = executor.clone();
    let bang_name = name.to_string();
    engine.register_bang(Bang::new(
        name,
        complete,
        description,
        bang_type,
        std::sync::Arc::new(move |_, raw_args, context| {
            let app_handle = app_handle.clone();
            let executor = executor.clone();
            let bang_name = bang_name.clone();
            async move {
                execute_runtime_bang(&app_handle, &bang_name, &executor, &raw_args, &context).await
            }
            .boxed()
        }),
    ));
}

async fn execute_runtime_bang(
    app_handle: &tauri::AppHandle,
    bang_name: &str,
    executor: &RuntimeBangExecutor,
    raw_args: &str,
    context: &HashMap<String, String>,
) -> String {
    let parameters = match build_tool_arguments(&executor.arguments, raw_args, context) {
        Ok(parameters) => parameters,
        Err(error) => {
            return format!("<!bang_error: bang '{}' 参数解析失败 - {}>", bang_name, error);
        }
    };

    let conversation_id =
        context.get("conversation_id").and_then(|value| value.parse::<i64>().ok());
    let feature_config_state = app_handle.state::<crate::FeatureConfigState>();
    match execute_tool_by_transport(
        app_handle,
        &feature_config_state,
        &executor.server,
        &executor.tool_name,
        &JsonValue::Object(parameters).to_string(),
        conversation_id,
        None,
    )
    .await
    {
        Ok(output) => stringify_tool_output(&output),
        Err(error) => format!("<!bang_error: bang '{}' 执行失败 - {}>", bang_name, error),
    }
}

fn resolve_runtime_executor(
    app_handle: &tauri::AppHandle,
    manifest: &ResolvedPluginManifest,
    contribution: &PluginBangContribution,
) -> Result<RuntimeBangExecutor, String> {
    match &contribution.executor {
        PluginBangExecutor::BuiltinTool { command, tool_name, arguments } => {
            Ok(RuntimeBangExecutor {
                server: build_builtin_server(command.as_deref().unwrap_or(DEFAULT_BUILTIN_COMMAND)),
                tool_name: tool_name.trim().to_string(),
                arguments: arguments.clone(),
            })
        }
        PluginBangExecutor::McpTool { server, tool_name, arguments } => Ok(RuntimeBangExecutor {
            server: resolve_mcp_server(app_handle, server)?,
            tool_name: tool_name.trim().to_string(),
            arguments: arguments.clone(),
        }),
        PluginBangExecutor::PluginMcpTool { server, tool_name, arguments } => {
            Ok(RuntimeBangExecutor {
                server: build_plugin_mcp_server(manifest, server)?,
                tool_name: tool_name.trim().to_string(),
                arguments: arguments.clone(),
            })
        }
    }
}

fn build_builtin_server(command: &str) -> MCPServer {
    MCPServer {
        id: synthetic_server_id(&format!("builtin:{}", command)),
        name: format!("Builtin {}", command),
        description: "Bang builtin executor".to_string(),
        transport_type: "stdio".to_string(),
        command: Some(command.to_string()),
        environment_variables: None,
        headers: None,
        url: None,
        timeout: None,
        is_long_running: false,
        is_enabled: true,
        is_builtin: true,
        is_deletable: false,
        proxy_enabled: false,
        created_time: String::new(),
    }
}

fn resolve_mcp_server(app_handle: &tauri::AppHandle, selector: &str) -> Result<MCPServer, String> {
    let normalized = selector.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("mcpTool.server 不能为空".to_string());
    }

    let db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let servers = db.get_mcp_servers().map_err(|e| e.to_string())?;
    servers
        .into_iter()
        .find(|server| {
            server.is_enabled
                && (server.name.eq_ignore_ascii_case(&normalized)
                    || server
                        .command
                        .as_deref()
                        .unwrap_or_default()
                        .eq_ignore_ascii_case(&normalized))
        })
        .ok_or_else(|| format!("未找到已启用的 MCP server: {}", selector))
}

fn build_plugin_mcp_server(
    manifest: &ResolvedPluginManifest,
    definition: &PluginBangServerDefinition,
) -> Result<MCPServer, String> {
    let plugin_dir = manifest.plugin_dir.to_string_lossy().to_string();
    let transport_type = definition.transport_type.trim().to_ascii_lowercase();
    if transport_type != "stdio" && transport_type != "http" {
        return Err(format!(
            "不支持的 pluginMcpTool transport_type: {}",
            definition.transport_type
        ));
    }

    let command =
        definition.command.as_ref().map(|value| replace_plugin_tokens(value, &plugin_dir));
    let url = definition.url.as_ref().map(|value| replace_plugin_tokens(value, &plugin_dir));
    if transport_type == "stdio" && command.as_deref().unwrap_or_default().trim().is_empty() {
        return Err("pluginMcpTool server.command 不能为空".to_string());
    }
    if transport_type == "http" && url.as_deref().unwrap_or_default().trim().is_empty() {
        return Err("pluginMcpTool server.url 不能为空".to_string());
    }

    let mut env_vars = definition.environment_variables.clone();
    env_vars.insert("AIPP_PLUGIN_DIR".to_string(), plugin_dir.clone());
    let environment_variables = if env_vars.is_empty() {
        None
    } else {
        Some(
            env_vars
                .into_iter()
                .map(|(key, value)| {
                    format!("{}={}", key, replace_plugin_tokens(&value, &plugin_dir))
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };

    let headers = if definition.headers.is_empty() {
        None
    } else {
        Some(
            serde_json::to_string(
                &definition
                    .headers
                    .iter()
                    .map(|(key, value)| (key.clone(), replace_plugin_tokens(value, &plugin_dir)))
                    .collect::<HashMap<_, _>>(),
            )
            .map_err(|e| e.to_string())?,
        )
    };

    Ok(MCPServer {
        id: synthetic_server_id(&format!("plugin:{}:{}", manifest.code, definition.name)),
        name: format!("{}::{}", manifest.code, definition.name),
        description: definition
            .description
            .clone()
            .unwrap_or_else(|| format!("{} 提供的 MCP server", manifest.name)),
        transport_type,
        command,
        environment_variables,
        headers,
        url,
        timeout: definition.timeout,
        is_long_running: definition.is_long_running,
        is_enabled: true,
        is_builtin: false,
        is_deletable: false,
        proxy_enabled: definition.proxy_enabled,
        created_time: String::new(),
    })
}

fn build_tool_arguments(
    definitions: &HashMap<String, PluginBangArgumentSpec>,
    raw_args: &str,
    context: &HashMap<String, String>,
) -> Result<JsonMap<String, JsonValue>, String> {
    let parsed_args = parse_bang_arguments(raw_args);
    if definitions.is_empty() {
        return parse_raw_json_arguments(&parsed_args.raw);
    }

    let mut result = JsonMap::new();
    for (parameter_name, spec) in definitions {
        if let Some(value) = resolve_argument_value(parameter_name, spec, &parsed_args, context)? {
            result.insert(parameter_name.clone(), value);
        }
    }
    Ok(result)
}

fn parse_raw_json_arguments(raw: &str) -> Result<JsonMap<String, JsonValue>, String> {
    if raw.trim().is_empty() {
        return Ok(JsonMap::new());
    }
    match serde_json::from_str::<JsonValue>(raw) {
        Ok(JsonValue::Object(map)) => Ok(map),
        Ok(_) => Err("bang 参数必须是 JSON 对象".to_string()),
        Err(error) => Err(format!("bang 参数不是合法 JSON 对象: {}", error)),
    }
}

fn resolve_argument_value(
    parameter_name: &str,
    spec: &PluginBangArgumentSpec,
    parsed_args: &ParsedBangArguments,
    context: &HashMap<String, String>,
) -> Result<Option<JsonValue>, String> {
    let raw_value = match spec.source {
        PluginBangArgumentSource::Raw => {
            if parsed_args.raw.is_empty() {
                None
            } else {
                Some(parsed_args.raw.clone())
            }
        }
        PluginBangArgumentSource::Arg => {
            parsed_args.positional.get(spec.index.unwrap_or(0)).cloned()
        }
        PluginBangArgumentSource::FirstArg => parsed_args.positional.first().cloned(),
        PluginBangArgumentSource::Named => {
            let key = spec.name.as_deref().unwrap_or(parameter_name).trim().to_ascii_lowercase();
            parsed_args.named.get(&key).cloned()
        }
        PluginBangArgumentSource::Context => {
            let key = spec.name.as_deref().unwrap_or(parameter_name).trim();
            context.get(key).cloned()
        }
        PluginBangArgumentSource::Const => None,
    };

    let value = if matches!(spec.source, PluginBangArgumentSource::Const) {
        spec.value.clone().or_else(|| spec.default.clone())
    } else if let Some(raw_value) = raw_value {
        Some(convert_argument_value(&raw_value, spec.value_type)?)
    } else if let Some(default_value) = spec.default.clone() {
        Some(default_value)
    } else if spec.required {
        return Err(format!("缺少必填参数: {}", parameter_name));
    } else {
        None
    };

    Ok(value)
}

fn convert_argument_value(
    raw_value: &str,
    value_type: Option<PluginBangArgumentValueType>,
) -> Result<JsonValue, String> {
    match value_type.unwrap_or(PluginBangArgumentValueType::String) {
        PluginBangArgumentValueType::String => Ok(JsonValue::String(unquote(raw_value))),
        PluginBangArgumentValueType::Number => {
            let value = unquote(raw_value)
                .trim()
                .parse::<f64>()
                .map_err(|e| format!("无法解析数字参数 '{}': {}", raw_value, e))?;
            let number = serde_json::Number::from_f64(value)
                .ok_or_else(|| format!("无法表示数字参数: {}", raw_value))?;
            Ok(JsonValue::Number(number))
        }
        PluginBangArgumentValueType::Boolean => {
            let normalized = unquote(raw_value).trim().to_ascii_lowercase();
            match normalized.as_str() {
                "true" | "1" | "yes" | "y" | "on" => Ok(JsonValue::Bool(true)),
                "false" | "0" | "no" | "n" | "off" => Ok(JsonValue::Bool(false)),
                _ => Err(format!("无法解析布尔参数 '{}': 仅支持 true/false", raw_value)),
            }
        }
        PluginBangArgumentValueType::Json => {
            serde_json::from_str::<JsonValue>(raw_value.trim()).map_err(|e| e.to_string())
        }
    }
}

fn parse_bang_arguments(raw_args: &str) -> ParsedBangArguments {
    let trimmed = raw_args.trim();
    let raw = if trimmed.starts_with('(') && trimmed.ends_with(')') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1].trim().to_string()
    } else {
        trimmed.to_string()
    };
    if raw.is_empty() {
        return ParsedBangArguments { raw, positional: Vec::new(), named: HashMap::new() };
    }

    let mut positional = Vec::new();
    let mut named = HashMap::new();
    for segment in split_top_level(&raw, ',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        if let Some((key, value)) = split_top_level_once(segment, '=') {
            let normalized_key = key.trim().to_ascii_lowercase();
            if normalized_key.is_empty() {
                positional.push(segment.to_string());
                continue;
            }
            named.insert(normalized_key, value.trim().to_string());
        } else {
            positional.push(segment.to_string());
        }
    }

    ParsedBangArguments { raw, positional, named }
}

fn split_top_level(input: &str, delimiter: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut escaped = false;
    let mut round_depth = 0;
    let mut square_depth = 0;
    let mut curly_depth = 0;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => {
                current.push(ch);
                escaped = true;
            }
            '\'' if !double_quoted => {
                single_quoted = !single_quoted;
                current.push(ch);
            }
            '"' if !single_quoted => {
                double_quoted = !double_quoted;
                current.push(ch);
            }
            '(' if !single_quoted && !double_quoted => {
                round_depth += 1;
                current.push(ch);
            }
            ')' if !single_quoted && !double_quoted && round_depth > 0 => {
                round_depth -= 1;
                current.push(ch);
            }
            '[' if !single_quoted && !double_quoted => {
                square_depth += 1;
                current.push(ch);
            }
            ']' if !single_quoted && !double_quoted && square_depth > 0 => {
                square_depth -= 1;
                current.push(ch);
            }
            '{' if !single_quoted && !double_quoted => {
                curly_depth += 1;
                current.push(ch);
            }
            '}' if !single_quoted && !double_quoted && curly_depth > 0 => {
                curly_depth -= 1;
                current.push(ch);
            }
            _ if ch == delimiter
                && !single_quoted
                && !double_quoted
                && round_depth == 0
                && square_depth == 0
                && curly_depth == 0 =>
            {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn split_top_level_once(input: &str, delimiter: char) -> Option<(String, String)> {
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut escaped = false;
    let mut round_depth = 0;
    let mut square_depth = 0;
    let mut curly_depth = 0;

    for (index, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => {
                escaped = true;
            }
            '\'' if !double_quoted => {
                single_quoted = !single_quoted;
            }
            '"' if !single_quoted => {
                double_quoted = !double_quoted;
            }
            '(' if !single_quoted && !double_quoted => round_depth += 1,
            ')' if !single_quoted && !double_quoted && round_depth > 0 => round_depth -= 1,
            '[' if !single_quoted && !double_quoted => square_depth += 1,
            ']' if !single_quoted && !double_quoted && square_depth > 0 => square_depth -= 1,
            '{' if !single_quoted && !double_quoted => curly_depth += 1,
            '}' if !single_quoted && !double_quoted && curly_depth > 0 => curly_depth -= 1,
            _ if ch == delimiter
                && !single_quoted
                && !double_quoted
                && round_depth == 0
                && square_depth == 0
                && curly_depth == 0 =>
            {
                return Some((
                    input[..index].to_string(),
                    input[index + ch.len_utf8()..].to_string(),
                ));
            }
            _ => {}
        }
    }

    None
}

fn stringify_tool_output(raw_output: &str) -> String {
    match serde_json::from_str::<JsonValue>(raw_output) {
        Ok(value) => flatten_tool_output(&value),
        Err(_) => raw_output.to_string(),
    }
}

fn flatten_tool_output(value: &JsonValue) -> String {
    match value {
        JsonValue::String(text) => text.clone(),
        JsonValue::Array(items) => {
            let texts = items.iter().filter_map(extract_text_part).collect::<Vec<_>>();
            if texts.is_empty() {
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            } else {
                texts.join("\n\n")
            }
        }
        JsonValue::Object(map) => {
            map.get("content").map(flatten_tool_output).unwrap_or_else(|| {
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            })
        }
        _ => value.to_string(),
    }
}

fn extract_text_part(value: &JsonValue) -> Option<String> {
    if let Some(text) = value.get("text").and_then(|item| item.as_str()) {
        return Some(text.to_string());
    }
    if let JsonValue::String(text) = value {
        return Some(text.clone());
    }
    None
}

fn normalize_bang_name(name: &str) -> Result<String, String> {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("bang name 不能为空".to_string());
    }
    if normalized.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        Ok(normalized)
    } else {
        Err(format!("非法 bang 名称 '{}': 仅支持字母、数字和下划线", name))
    }
}

fn derive_completion(
    primary_name: &str,
    alias_name: Option<&str>,
    contribution: &PluginBangContribution,
    argument_specs: &HashMap<String, PluginBangArgumentSpec>,
) -> String {
    let target_name = alias_name.unwrap_or(primary_name);
    let default_completion = if argument_specs.is_empty() {
        target_name.to_string()
    } else {
        format!("{}(|)", target_name)
    };

    let Some(template) = contribution.complete.as_deref().map(str::trim) else {
        return default_completion;
    };
    if template.is_empty() {
        return default_completion;
    }
    if alias_name.is_none() {
        return template.to_string();
    }
    if let Some(rest) = template.strip_prefix(primary_name) {
        return format!("{}{}", target_name, rest);
    }
    default_completion
}

fn parse_bang_type(raw_type: Option<&str>) -> BangType {
    match raw_type.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        Some("image") => BangType::Image,
        Some("audio") => BangType::Audio,
        _ => BangType::Text,
    }
}

fn replace_plugin_tokens(value: &str, plugin_dir: &str) -> String {
    value.replace("{plugin_dir}", plugin_dir)
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.chars().next().unwrap_or_default();
        let last = trimmed.chars().last().unwrap_or_default();
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

fn synthetic_server_id(seed: &str) -> i64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hasher);
    -((hasher.finish() & 0x3fff_ffff_ffff_ffff) as i64)
}

fn has_permission(permissions: &[String], expected: &str) -> bool {
    permissions.iter().any(|permission| permission == expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_and_named_arguments() {
        let parsed = parse_bang_arguments(r#"./src, recursive=true, pattern="*.rs""#);
        assert_eq!(parsed.raw, r#"./src, recursive=true, pattern="*.rs""#);
        assert_eq!(parsed.positional, vec!["./src"]);
        assert_eq!(parsed.named.get("recursive").map(String::as_str), Some("true"));
        assert_eq!(parsed.named.get("pattern").map(String::as_str), Some("\"*.rs\""));
    }

    #[test]
    fn splits_top_level_without_breaking_nested_values() {
        let parts =
            split_top_level(r#"path="/tmp,dev", json={"a": [1, 2]}, command=echo $(pwd)"#, ',');
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], r#"path="/tmp,dev""#);
        assert_eq!(parts[1], r#"json={"a": [1, 2]}"#);
        assert_eq!(parts[2], "command=echo $(pwd)");
    }
}
