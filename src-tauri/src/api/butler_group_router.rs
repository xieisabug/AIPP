//! 群聊发言调度：用户没有 @ 任何人时，判断由哪些成员回复。
//!
//! 只做判断，不回答问题。优先使用管家设置里的「小型辅助模型」，未配置时由总管家的模型判断。
//! 配置的模型不存在或调用失败时如实报错，不改用其他模型。
use std::collections::HashMap;

use tauri::{AppHandle, Manager};

use super::butler_group::RosterEntry;
use crate::db::butler_group::{Member, Message};

const RECENT_MESSAGES: usize = 12;
const RECENT_EXCERPT_CHARS: usize = 300;
const LATEST_CHARS: usize = 2_000;
const MAX_AUTO_RESPONDERS: usize = 3;

fn excerpt(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        flat
    } else {
        format!("{}…", flat.chars().take(limit).collect::<String>())
    }
}

fn speaker(message: &Message) -> String {
    if message.from_user() {
        "用户".into()
    } else if message.kind == "message" {
        message.sender_name.clone()
    } else {
        "〔系统〕".into()
    }
}

pub(super) struct Candidate<'a> {
    pub member: &'a Member,
    pub entry: Option<&'a RosterEntry>,
}

fn fallback_name(candidates: &[Candidate<'_>]) -> String {
    candidates.iter().find(|c| c.entry.is_some_and(|e| e.chief)).or(candidates.first())
        .map(|c| c.member.name.clone()).unwrap_or_default()
}

pub(super) fn build_prompt(group: &str, candidates: &[Candidate<'_>], recent: &[Message], latest: &Message) -> (String, String) {
    let system = format!("你是群聊里的发言调度，只决定接下来由哪些成员回复用户的最新消息，不回答问题本身。
规则：
1. 用户叫了某些成员的名字（或“你们俩”之类明确指向），就选这些成员。
2. 用户要求大家都发表意见（“大家”“你们都”“每个人”等），按成员列表顺序选出所有成员。
3. 用户在接着和某位成员说话（追问、回应对方刚才的问题），选那位成员。
4. 其他情况选最适合回答的一位成员；只有问题明显需要几种不同专长时才选多位，按合理的发言顺序排列，最多 {MAX_AUTO_RESPONDERS} 位。
5. 拿不准时选「{}」。
只输出 JSON，例如 {{\"responders\":[\"成员名\"]}}，名字必须与成员列表中的完全一致。", fallback_name(candidates));
    let members = candidates.iter().map(|c| {
        let description = c.entry.map(|e| excerpt(&e.description, 80)).filter(|d| !d.is_empty());
        match description {
            Some(description) => format!("- {}：{description}", c.member.name),
            None => format!("- {}", c.member.name),
        }
    }).collect::<Vec<_>>().join("\n");
    let history = recent.iter().filter(|m| m.seq < latest.seq).rev().take(RECENT_MESSAGES).collect::<Vec<_>>()
        .into_iter().rev().map(|m| format!("{}：{}", speaker(m), excerpt(&m.body, RECENT_EXCERPT_CHARS)))
        .collect::<Vec<_>>().join("\n");
    let quoted = latest.reply_to.and_then(|seq| recent.iter().find(|m| m.seq == seq))
        .map(|m| format!("（引用了 {}：「{}」）\n", speaker(m), excerpt(&m.body, 120))).unwrap_or_default();
    let user = format!("群聊：{group}\n成员：\n{members}\n\n最近的聊天：\n{}\n\n用户的最新消息：\n{quoted}{}",
        if history.is_empty() { "（无）".into() } else { history },
        latest.body.chars().take(LATEST_CHARS).collect::<String>());
    (system, user)
}

/// 解析调度模型的输出。名字按成员显示名匹配（容忍前导 @ 与空白），未知名字忽略。
pub(super) fn parse_responders(text: &str, candidates: &[Candidate<'_>]) -> Result<Vec<i64>, String> {
    let start = text.find('{');
    let end = text.rfind('}');
    let json = match (start, end) {
        (Some(start), Some(end)) if end > start => &text[start..=end],
        _ => return Err(format!("调度模型没有返回 JSON：{}", excerpt(text, 200))),
    };
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("调度模型返回的 JSON 无法解析（{e}）：{}", excerpt(text, 200)))?;
    let names = value.get("responders").and_then(|v| v.as_array())
        .ok_or_else(|| format!("调度模型的输出缺少 responders：{}", excerpt(text, 200)))?;
    let mut result = Vec::new();
    for name in names.iter().filter_map(|v| v.as_str()) {
        let wanted = name.trim().trim_start_matches(['@', '＠']).trim();
        let found = candidates.iter().find(|c| c.member.name == wanted)
            .or_else(|| candidates.iter().find(|c| c.member.name.to_lowercase() == wanted.to_lowercase()));
        if let Some(c) = found {
            if !result.contains(&c.member.id) {
                result.push(c.member.id);
            }
        }
    }
    if result.is_empty() {
        return Err(format!("调度模型选择的成员不在群里：{}", excerpt(text, 200)));
    }
    Ok(result)
}

fn parse_model(raw: &str) -> Option<(String, i64)> {
    let (code, provider) = raw.trim().split_once("%%")?;
    let code = code.trim();
    let provider = provider.trim().parse::<i64>().ok()?;
    (!code.is_empty()).then(|| (code.to_string(), provider))
}

async fn model(app: &AppHandle) -> Result<(String, i64, &'static str), String> {
    let helper = super::butler_api::get_experimental_config_value(app, super::butler_progress::HELPER_MODEL_KEY).await
        .filter(|value| !value.trim().is_empty());
    if let Some(raw) = helper {
        let (code, provider) = parse_model(&raw).ok_or_else(|| format!("小型辅助模型配置无效：{raw}；请在管家设置中重新选择"))?;
        return Ok((code, provider, "小型辅助模型"));
    }
    let selection = super::butler_api::get_butler_model_selection(app).await?;
    Ok((selection.model_code, selection.provider_id, "总管家模型"))
}

pub(super) async fn choose(app: &AppHandle, group_id: i64, group: &str, members: &[Member],
    roster: &HashMap<i64, RosterEntry>, recent: &[Message], latest: &Message) -> Result<Vec<i64>, String> {
    let candidates = members.iter().map(|member| Candidate { member, entry: roster.get(&member.assistant_id) }).collect::<Vec<_>>();
    let (code, provider, source) = model(app).await?;
    let config = app.state::<crate::FeatureConfigState>().config_feature_map.lock().await.clone();
    let app_db = app.clone();
    let lookup = code.clone();
    let detail = tokio::task::spawn_blocking(move || {
        crate::db::llm_db::LLMDatabase::new(&app_db).map_err(|e| e.to_string())?
            .get_llm_model_detail(&provider, &lookup).map_err(|e| e.to_string())
    }).await.map_err(|e| format!("读取{source}配置的任务失败：{e}"))?
        .map_err(|e| format!("{source}不可用（provider={provider}, model={code}）：{e}；请在管家设置中检查模型"))?;
    let proxy = super::ai::config::get_network_proxy_from_config(&config);
    let provider_config = super::copilot_token_manager::prepare_provider_configs(app, &detail.provider.api_type, &detail.configs, proxy.as_deref())
        .await.map_err(|e| format!("{source}的供应商配置无效（provider={provider}, model={code}）：{e}"))?;
    let client = super::genai_client::create_client_with_config(Some((app, group_id)), &provider_config, &detail.model.code,
        &detail.provider.api_type, Some(&detail.model.request_mode), proxy.as_deref(), false,
        Some(super::ai::config::get_request_timeout_from_config(&config)), false, &config)
        .map_err(|e| format!("创建{source}客户端失败（provider={provider}, model={code}）：{e}"))?;
    let (system, user) = build_prompt(group, &candidates, recent, latest);
    let request = super::ai::conversation::build_chat_request_from_messages(
        &[("system".to_string(), system, Vec::new()), ("user".to_string(), user, Vec::new())],
        super::ai::conversation::ToolCallStrategy::NonNative,
        None,
    ).chat_request;
    let response = client.exec_chat(&detail.model.code, request, None).await
        .map_err(|e| format!("{source}调用失败（provider={provider}, model={code}）：{e}"))?;
    let text = response.first_text().unwrap_or_default();
    parse_responders(text, &candidates)
}

#[cfg(test)]
#[path = "butler_group_router_tests.rs"]
mod tests;
