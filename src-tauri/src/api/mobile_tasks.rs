use serde::Serialize;
use tauri::Manager;
use crate::api::butler_api::{active_butler_main_conversation_id, list_butler_tasks, ButlerTaskListItem};
use crate::mcp::builtin_mcp::operation::state::PermissionRequestSnapshot;

#[derive(Serialize)]
pub struct MobileButlerOverview {
    pub main_conversation_id: Option<i64>,
    pub tasks: Vec<ButlerTaskListItem>,
    pub local_permissions: Vec<PermissionRequestSnapshot>,
}

/// Read-only: loading the mobile page must not create a conversation or reconcile remote work.
#[tauri::command]
pub async fn get_mobile_butler_overview(app_handle: tauri::AppHandle) -> Result<MobileButlerOverview, String> {
    let main_conversation_id = active_butler_main_conversation_id(&app_handle)?;
    let tasks = match main_conversation_id {
        Some(id) => list_butler_tasks(app_handle.clone(), id).await?,
        None => Vec::new(),
    };
    let state = app_handle.state::<crate::mcp::builtin_mcp::OperationState>();
    let mut local_permissions = Vec::new();
    for task in &tasks {
        local_permissions.extend(state.list_permission_requests_for_conversation(task.task_conversation_id).await);
    }
    Ok(MobileButlerOverview { main_conversation_id, tasks, local_permissions })
}

#[tauri::command]
pub async fn stop_mobile_feishu_runtime(app_handle: tauri::AppHandle) -> Result<crate::feishu::FeishuRuntimeStatus, String> {
    crate::feishu::stop_runtime(&app_handle).await
}
