//! Scoped rollout controls: disabling a capability prevents new work, not reads,
//! cancellation, settlement or receipt delivery. Existing records are preserved.
use tauri::AppHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButlerCapability {
    TaskDispatch,
    PeerCollaboration,
    WorkContinuation,
}

impl ButlerCapability {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::TaskDispatch => "butler_task_dispatch_enabled",
            Self::PeerCollaboration => "butler_peer_collaboration_enabled",
            Self::WorkContinuation => "butler_work_continuation_enabled",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::TaskDispatch => "新建独立任务",
            Self::PeerCollaboration => "发起成员协作",
            Self::WorkContinuation => "继续执行已有工作",
        }
    }
}

// Old installations retain existing behavior. An explicit malformed value is
// fail-closed, rather than turning a disabled capability back on accidentally.
pub(crate) fn configured_enabled(value: Option<&str>) -> bool {
    value.map(|value| matches!(value.trim(), "true" | "1")).unwrap_or(true)
}

pub(crate) async fn enabled(app: &AppHandle, capability: ButlerCapability) -> bool {
    let value = super::butler_api::get_experimental_config_value(app, capability.key()).await;
    configured_enabled(value.as_deref())
}

pub(crate) async fn ensure_enabled(app: &AppHandle, capability: ButlerCapability) -> Result<(), String> {
    if enabled(app, capability).await {
        Ok(())
    } else {
        Err(format!("管家能力“{}”已停用；未启动新执行。可在管家设置中重新开启，已有记录、停止操作和结果查看仍可用。", capability.label()))
    }
}

#[cfg(test)]
#[path = "butler_capabilities_tests.rs"]
mod tests;
