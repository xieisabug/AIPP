//! 主动整理统一无人值守安全门禁。
//!
//! 同时服务三条执行路径：
//! 1. 普通 LLM 无头 Agentic Loop（`UnattendedToolGate`）；
//! 2. 原生 Agent 经 `acp_mcp_bridge` 调用 AIPP MCP 工具；
//! 3. 原生 Agent 自身内部工具/命令权限请求（`native_agents.rs` / `codex_app_server.rs`）。
//!
//! 规则：
//! - 硬红线（`rm`/删除类、需人工批准的 SuperAdmin 动作、在线问答工具、外发搜索含敏感词）直接拒绝；
//! - 主动整理专用工具与免审核内置工具直接放行；
//! - 其余操作（含文件写入、命令执行、外部 MCP）一律走 `tool_review` 自动安全审核；
//! - 审核未通过、超时或未配置审核模型时绝不挂起等人，直接返回拒绝并引导 AI 改用 `proactive_propose`。
use crate::api::ai::acp::{AcpPermissionOptionPayload, AcpPermissionRequestEvent};
use crate::api::ai::unattended_loop::UnattendedToolGate;
use crate::db::mcp_db::MCPDatabase;
use crate::mcp::tool_review::{self, UnattendedReview};
use crate::scheduler::gate::{self as sched_gate, Admission};

use super::{delta, tools};

fn refuse_with_proposal_hint(reason: &str) -> Admission {
    Admission::Refuse(format!(
        "主动整理安全审核未放行（{reason}）。后台整理不会挂起等待人工审批；如果此事确有必要执行，请改用 proactive_propose 生成建议卡片交给用户确认。"
    ))
}

/// 主动整理工具调用前置硬约束。
pub(crate) fn proactive_tool_restriction(
    is_builtin: bool,
    command: Option<&str>,
    tool: &str,
    parameters: &str,
) -> Option<String> {
    if is_builtin && command == Some("aipp:agent") && tools::is_proactive_tool(tool) {
        return None;
    }
    if let Some(reason) = sched_gate::human_only_reason(command.filter(|_| is_builtin), tool, parameters) {
        return Some(reason);
    }
    if is_builtin && command == Some("aipp:search") && delta::sensitive(parameters) {
        return Some("外发搜索或抓取参数包含疑似敏感信息，禁止发送到外部服务".to_string());
    }
    if is_builtin && command == Some("aipp:agent") && tool == "schedule_task" {
        let args: serde_json::Value = serde_json::from_str(parameters).unwrap_or_default();
        if !matches!(args["action"].as_str(), Some("list" | "get" | "runs")) {
            return Some("主动整理不能直接创建、修改或删除定时任务，请改用 proactive_propose 建议用户确认".to_string());
        }
    }
    None
}

/// 审核一条已入库的 `mcp_tool_call`。
pub async fn admit_call(app: &tauri::AppHandle, conversation_id: i64, call_id: i64) -> Admission {
    let resolved = MCPDatabase::new(app)
        .map_err(|e| e.to_string())
        .and_then(|db| {
            let call = db.get_mcp_tool_call(call_id).map_err(|e| e.to_string())?;
            let server = db.get_mcp_server(call.server_id).map_err(|e| e.to_string())?;
            Ok((call, server))
        });
    let (call, server) = match resolved {
        Ok(resolved) => resolved,
        Err(error) => return Admission::Refuse(format!("读取工具调用失败，未执行：{error}")),
    };
    let command = server.command.as_deref().filter(|_| server.is_builtin);
    if server.is_builtin && command == Some("aipp:agent") && tools::is_proactive_tool(&call.tool_name) {
        return Admission::Run("主动整理专用工具".to_string());
    }
    if let Some(reason) = proactive_tool_restriction(server.is_builtin, command, &call.tool_name, &call.parameters) {
        return refuse_with_proposal_hint(&reason);
    }
    match tool_review::review_unattended(app, conversation_id, call_id).await {
        UnattendedReview::Exempt => Admission::Run("免审核工具".to_string()),
        UnattendedReview::Safe(reason) => Admission::Run(format!("自动安全审核通过：{reason}")),
        UnattendedReview::NotSafe(reason) => refuse_with_proposal_hint(&reason),
    }
}

/// 供 `acp_mcp_bridge` Proxy 调用：为原生 Agent 的 MCP 工具调用登记记录并执行同一安全门禁。
pub(crate) async fn admit_proxy_tool(
    app: &tauri::AppHandle,
    conversation_id: i64,
    server: &crate::db::mcp_db::MCPServer,
    tool_name: &str,
    parameters: &str,
) -> Result<Option<i64>, String> {
    let command = server.command.as_deref().filter(|_| server.is_builtin);
    if server.is_builtin && command == Some("aipp:agent") && tools::is_proactive_tool(tool_name) {
        return Ok(None);
    }
    if let Some(reason) = proactive_tool_restriction(server.is_builtin, command, tool_name, parameters) {
        return Err(match refuse_with_proposal_hint(&reason) {
            Admission::Refuse(msg) => msg,
            Admission::Run(_) => reason,
        });
    }
    let record = crate::mcp::execution_api::create_mcp_tool_call_with_llm_id(
        app.clone(),
        conversation_id,
        None,
        server.name.clone(),
        tool_name.to_string(),
        parameters.to_string(),
        None,
        None,
    )
    .await?;
    match admit_call(app, conversation_id, record.id).await {
        Admission::Run(_) => Ok(Some(record.id)),
        Admission::Refuse(reason) => {
            if let Ok(db) = MCPDatabase::new(app) {
                let _ = db.update_mcp_tool_call_status(record.id, "failed", None, Some(&reason));
            }
            Err(reason)
        }
    }
}

/// 供原生 Agent 内部权限请求（ACP / Claude Code / Codex / NativeAgents）在主动整理会话中使用：
/// 审核通过返回一次性允许 option_id；不通过或包含硬红线直接返回 `Err(reject_option_id, reason)`，绝不挂起弹窗。
pub(crate) fn select_reject_option(options: &[AcpPermissionOptionPayload]) -> Option<String> {
    options
        .iter()
        .find(|o| o.kind == "reject_once" && !o.option_id.is_empty())
        .or_else(|| options.iter().find(|o| o.kind == "reject_always" && !o.option_id.is_empty()))
        .map(|o| o.option_id.clone())
}

pub(crate) async fn review_native_permission(
    app: &tauri::AppHandle,
    conversation_id: i64,
    turn: i64,
    agent: &'static str,
    event: &AcpPermissionRequestEvent,
    working_directory: &str,
) -> Result<String, (Option<String>, String)> {
    let title = event.title.as_deref().unwrap_or("未命名操作");
    let params = event.parameters.as_deref().unwrap_or_default();
    if crate::mcp::builtin_mcp::contains_manual_review_command_keyword(params)
        || crate::mcp::builtin_mcp::contains_manual_review_command_keyword(title)
    {
        return Err((select_reject_option(&event.options), "包含 rm/删除类高危命令，后台主动整理拒绝执行".into()));
    }
    let draft = tool_review::review_agent_permission(
        app,
        conversation_id,
        turn,
        agent,
        title,
        params,
        working_directory,
    )
    .await;
    match crate::api::ai::agent_driver::native_agents::review_outcome(&event.options, &draft.verdict, &draft.reason) {
        crate::api::ai::agent_driver::native_agents::ReviewOutcome::Allow(option_id) => Ok(option_id),
        crate::api::ai::agent_driver::native_agents::ReviewOutcome::Ask(reason) => {
            Err((select_reject_option(&event.options), reason))
        }
    }
}

pub struct ProactiveToolGate<'a> {
    pub app: &'a tauri::AppHandle,
    pub conversation_id: i64,
}

#[async_trait::async_trait]
impl UnattendedToolGate for ProactiveToolGate<'_> {
    async fn admit(&self, call_db_id: i64, _progress: &str) -> Admission {
        admit_call(self.app, self.conversation_id, call_db_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proactive_tools_are_exempt_and_dangerous_ops_are_blocked() {
        assert!(proactive_tool_restriction(true, Some("aipp:agent"), "proactive_track", "{}").is_none());
        assert!(proactive_tool_restriction(true, Some("aipp:agent"), "proactive_note", "{}").is_none());
        assert!(proactive_tool_restriction(
            true,
            Some("aipp:operation"),
            "execute_bash",
            r#"{"command":"rm -rf tmp"}"#
        )
        .is_some());
        assert!(proactive_tool_restriction(
            true,
            Some("aipp:ui_interaction"),
            "ask_user_question",
            "{}"
        )
        .is_some());
        assert!(proactive_tool_restriction(
            true,
            Some("aipp:search"),
            "search_http",
            r#"{"query":"sk-1234567890abcdef"}"#
        )
        .is_some());
        assert!(proactive_tool_restriction(
            true,
            Some("aipp:agent"),
            "schedule_task",
            r#"{"action":"list"}"#
        )
        .is_none());
        assert!(proactive_tool_restriction(
            true,
            Some("aipp:agent"),
            "schedule_task",
            r#"{"action":"delete","task_id":1}"#
        )
        .is_some());
    }

    #[test]
    fn select_reject_option_prefers_reject_once_over_reject_always() {
        let options = vec![
            AcpPermissionOptionPayload { option_id: "allow".into(), name: "Allow".into(), kind: "allow_once".into() },
            AcpPermissionOptionPayload { option_id: "deny_all".into(), name: "Deny always".into(), kind: "reject_always".into() },
            AcpPermissionOptionPayload { option_id: "deny_once".into(), name: "Deny".into(), kind: "reject_once".into() },
        ];
        assert_eq!(select_reject_option(&options).as_deref(), Some("deny_once"));
        assert_eq!(select_reject_option(&options[..2]).as_deref(), Some("deny_all"));
        assert_eq!(select_reject_option(&options[..1]), None);
    }
}
