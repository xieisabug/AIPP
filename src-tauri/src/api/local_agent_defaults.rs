//! 新建本地智能体提供商时，读取各智能体自己的默认配置（只读，不修改任何文件）。
//!
//! - `settings`：写入新提供商的配置。只放 AIPP 会显式传给智能体的项：不写的话 AIPP 用自己的
//!   默认值覆盖智能体配置（如 Codex 的审批策略/沙箱模式）。
//! - `info`：仅展示。AIPP 未设置这些项时智能体本来就按自身配置运行，写进提供商反而会把值固定住。
use crate::api::ai::codex_app_server::{CODEX_APPROVAL_POLICIES, CODEX_SANDBOX_MODES};
use serde::Serialize;
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Debug, Default, PartialEq)]
pub struct LocalAgentDefaults {
    /// 读取的配置文件（主目录缩写为 ~）；该智能体没有可读取的配置格式时为 None
    pub source: Option<String>,
    /// 配置文件存在且已读取
    pub found: bool,
    pub settings: BTreeMap<String, String>,
    pub info: Vec<DefaultInfo>,
    /// 无法采用的值、读取或解析失败的原因
    pub notes: Vec<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct DefaultInfo {
    pub label: String,
    pub value: String,
}

struct Locations {
    home: PathBuf,
    claude_config_dir: Option<PathBuf>,
}

impl Locations {
    fn display(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(rest) => format!("~/{}", rest.to_string_lossy().replace('\\', "/")),
            Err(_) => path.display().to_string(),
        }
    }
}

#[tauri::command]
pub async fn read_local_agent_defaults(api_type: String) -> Result<LocalAgentDefaults, String> {
    let home = dirs::home_dir().ok_or("无法确定用户主目录，不能读取智能体的默认配置")?;
    let locations = Locations {
        home,
        claude_config_dir: std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from),
    };
    let label = api_type.clone();
    tokio::task::spawn_blocking(move || read_defaults(&api_type, &locations))
        .await
        .map_err(|e| format!("读取本地智能体 {label} 默认配置的任务失败: {e}"))
}

fn read_defaults(api_type: &str, locations: &Locations) -> LocalAgentDefaults {
    let home = &locations.home;
    let candidates: Vec<PathBuf> = match api_type {
        // AIPP 启动 Codex 时把 CODEX_HOME 指向 ~/.codex（codex_app_server.rs）
        "codex_app_server" => vec![home.join(".codex").join("config.toml")],
        "claude_sdk" => vec![locations
            .claude_config_dir
            .clone()
            .unwrap_or_else(|| home.join(".claude"))
            .join("settings.json")],
        "kiro" => vec![home.join(".kiro").join("settings").join("cli.json")],
        "pi" => vec![home.join(".pi").join("agent").join("settings.json")],
        "opencode" => {
            let dir = home.join(".config").join("opencode");
            vec![dir.join("opencode.json"), dir.join("opencode.jsonc")]
        }
        _ => return LocalAgentDefaults::default(),
    };
    let path = candidates.iter().find(|p| p.exists()).unwrap_or(&candidates[0]).clone();
    let source = locations.display(&path);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => Some(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return LocalAgentDefaults {
                source: Some(source.clone()),
                notes: vec![format!("读取 {source} 失败：{e}")],
                ..Default::default()
            }
        }
    };
    parse_defaults(api_type, &source, content.as_deref())
}

/// 按智能体解析配置文件内容；`content` 为 None 表示文件不存在。
pub(crate) fn parse_defaults(api_type: &str, source: &str, content: Option<&str>) -> LocalAgentDefaults {
    let mut out = LocalAgentDefaults { source: Some(source.to_string()), ..Default::default() };
    let Some(content) = content else { return out };
    out.found = true;
    let parsed = match api_type {
        "codex_app_server" => toml::from_str::<toml::Value>(content)
            .map_err(|e| e.to_string())
            .and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string())),
        "opencode" => serde_json::from_str::<Json>(&strip_jsonc(content)).map_err(|e| e.to_string()),
        _ => serde_json::from_str::<Json>(content).map_err(|e| e.to_string()),
    };
    let root = match parsed {
        Ok(root) => root,
        Err(e) => {
            out.notes.push(format!("解析 {source} 失败：{e}"));
            return out;
        }
    };
    let info = &mut out.info;
    match api_type {
        "codex_app_server" => {
            // 顶层 profile 选中的 [profiles.<name>] 覆盖顶层同名键
            let profile = root.get("profile").and_then(Json::as_str).and_then(|name| root.get("profiles")?.get(name));
            let get = |key: &str| profile.and_then(|p| p.get(key)).or_else(|| root.get(key));
            push_info(info, "默认模型", get("model"));
            push_info(info, "推理强度", get("model_reasoning_effort"));
            if let Some(name) = root.get("profile").and_then(Json::as_str) {
                push_info(info, "使用的 profile", Some(&Json::String(name.into())));
            }
            for (key, setting, allowed, fallback) in [
                ("approval_policy", "codex_approval_policy", CODEX_APPROVAL_POLICIES, "on-request"),
                ("sandbox_mode", "codex_sandbox", CODEX_SANDBOX_MODES, "workspace-write"),
            ] {
                match get(key).and_then(Json::as_str) {
                    Some(value) if allowed.contains(&value) => {
                        out.settings.insert(setting.into(), value.into());
                    }
                    Some(value) => out
                        .notes
                        .push(format!("{key} = \"{value}\" 不在 AIPP 支持的取值内（{}），将使用 {fallback}", allowed.join(" / "))),
                    None => {}
                }
            }
        }
        "claude_sdk" => {
            push_info(info, "权限模式", root.get("permissions").and_then(|p| p.get("defaultMode")));
            push_info(info, "思考强度", root.get("effortLevel"));
            push_info(info, "默认模型", root.get("model"));
        }
        "kiro" => {
            // `kiro-cli settings` 以点号键平铺存储，也兼容嵌套写法
            let get = |key: &str| {
                root.get(key).or_else(|| key.split('.').try_fold(&root, |node, part| node.get(part)))
            };
            push_info(info, "默认 Agent", get("chat.defaultAgent"));
            push_info(info, "默认模型", get("chat.defaultModel"));
        }
        "pi" => {
            push_info(info, "默认 Provider", root.get("defaultProvider"));
            push_info(info, "默认模型", root.get("defaultModel"));
            push_info(info, "思考强度", root.get("defaultThinkingLevel"));
        }
        "opencode" => {
            push_info(info, "默认模型", root.get("model"));
            push_info(info, "小模型", root.get("small_model"));
        }
        _ => {}
    }
    out
}

fn push_info(info: &mut Vec<DefaultInfo>, label: &str, value: Option<&Json>) {
    if let Some(value) = value.and_then(scalar) {
        info.push(DefaultInfo { label: label.into(), value });
    }
}

fn scalar(value: &Json) -> Option<String> {
    match value {
        Json::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Json::Number(n) => Some(n.to_string()),
        Json::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// 去掉 JSONC 的注释与尾随逗号（字符串内的内容保持不变）。
pub(crate) fn strip_jsonc(input: &str) -> String {
    let without_comments = {
        let chars: Vec<char> = input.chars().collect();
        let mut out = String::with_capacity(input.len());
        let (mut i, mut in_string) = (0, false);
        while i < chars.len() {
            let c = chars[i];
            if in_string {
                out.push(c);
                if c == '\\' && i + 1 < chars.len() {
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                in_string = c != '"';
                i += 1;
                continue;
            }
            match (c, chars.get(i + 1)) {
                ('"', _) => {
                    in_string = true;
                    out.push(c);
                    i += 1;
                }
                ('/', Some('/')) => {
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                }
                ('/', Some('*')) => {
                    i += 2;
                    while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                        i += 1;
                    }
                    i += 2;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        out
    };
    let chars: Vec<char> = without_comments.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            in_string = c != '"';
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|ch| !ch.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

#[cfg(test)]
#[path = "local_agent_defaults_tests.rs"]
mod tests;
