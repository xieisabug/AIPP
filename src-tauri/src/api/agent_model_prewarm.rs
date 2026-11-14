//! 启动后在后台预读 Agent 助手的模型列表，让新建对话、助手设置第一次打开也直接命中
//! `agent_model_cache`，不用等本机 CLI 冷启动（Pi ~0.5s、Codex 0.25~5s、Cursor 2~4s）。
//!
//! 只预读探测时不会在智能体里留下痕迹的类型：Codex（不建 thread）、Pi（不落盘会话）、
//! Cursor（只发 HTTP）、OpenCode（拿到目录即返回）。通用 ACP、Kiro、Antigravity 的探测要
//! `session/new`，会出现在智能体自己的会话历史里，留到用户真正打开时再读（之后同样走缓存）。
//! 预读只占结果本身的内存（几 KB）；拉起的 CLI 拿到列表就退出，逐个执行避免启动时同时拉起多个。

use crate::api::assistant_api::{get_agent_model_options, get_assistants};
use crate::api::ai::codex_app_server::CODEX_APP_SERVER_API_TYPE;
use crate::db::{assistant_db::AssistantDatabase, llm_db::LLMDatabase};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// 等主窗口创建与首屏加载过去再预读
const PREWARM_DELAY: Duration = Duration::from_secs(5);
const AGENT_ASSISTANT_TYPE: i64 = 4;

/// 探测没有副作用、适合启动时预读的 Agent 类型
pub(crate) fn is_prewarm_safe_agent(api_type: &str) -> bool {
    matches!(api_type, CODEX_APP_SERVER_API_TYPE | "pi" | "cursor" | "opencode")
}

pub(crate) struct PrewarmCandidate {
    pub assistant_id: i64,
    pub provider_api_type: String,
    pub provider_enabled: bool,
}

/// 预读对象：提供商已启用、且探测无副作用的 Agent 助手
pub(crate) fn prewarm_targets(candidates: &[PrewarmCandidate]) -> Vec<i64> {
    candidates
        .iter()
        .filter(|candidate| candidate.provider_enabled && is_prewarm_safe_agent(&candidate.provider_api_type))
        .map(|candidate| candidate.assistant_id)
        .collect()
}

fn collect_candidates(app_handle: &tauri::AppHandle) -> Result<Vec<PrewarmCandidate>, String> {
    let assistant_db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let llm_db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let mut candidates = Vec::new();
    for assistant in get_assistants(app_handle.clone())? {
        if assistant.assistant_type != Some(AGENT_ASSISTANT_TYPE) {
            continue;
        }
        let models = assistant_db.get_assistant_model(assistant.id).map_err(|e| e.to_string())?;
        let Some(model) = models.first() else { continue };
        // 提供商缺失的助手本来就用不了，打开时由模型选择器报错，这里只是不预读
        match llm_db.get_llm_provider(model.provider_id) {
            Ok(provider) => candidates.push(PrewarmCandidate {
                assistant_id: assistant.id,
                provider_api_type: provider.api_type,
                provider_enabled: provider.is_enabled,
            }),
            Err(error) => debug!(assistant_id = assistant.id, error = %error, "Skipping agent model prewarm: provider unavailable"),
        }
    }
    Ok(candidates)
}

pub(crate) fn spawn(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(PREWARM_DELAY).await;
        let targets = match collect_candidates(&app_handle) {
            Ok(candidates) => prewarm_targets(&candidates),
            Err(error) => {
                warn!(error = %error, "Agent model prewarm skipped: cannot list Agent assistants");
                return;
            }
        };
        // 逐个执行：配置相同的助手第二次直接命中缓存
        for assistant_id in targets {
            let started = Instant::now();
            match get_agent_model_options(app_handle.clone(), assistant_id, None).await {
                Ok(options) => info!(
                    assistant_id,
                    models = options.len(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Agent model list prewarmed"
                ),
                Err(error) => warn!(
                    assistant_id,
                    error = %error,
                    "Agent model prewarm failed; the model picker probes again when opened"
                ),
            }
        }
    });
}

#[cfg(test)]
#[path = "agent_model_prewarm_tests.rs"]
mod tests;
