pub mod butler_control;
pub mod butler_group;
pub mod butler_group_runtime;
mod butler_group_router;
pub mod ai;
pub mod ai_api;
pub mod assistant_api;
pub mod assistant_summary_api;
pub mod attachment_api;
pub mod butler_api;
pub(crate) mod butler_reporting;
pub(crate) mod butler_capabilities;
pub mod butler_contact_activity;
pub mod butler_progress;
pub mod butler_peer;
pub mod butler_work;
pub mod conversation_api;
pub mod copilot_api;
#[cfg(desktop)]
pub mod copilot_lsp;
pub mod copilot_token_manager;
pub mod export_api;
pub mod genai_client;
#[cfg(test)]
pub(crate) mod live_test_client;
pub mod highlight_api;
pub mod llm_api;
pub mod mobile_capability;
pub mod mobile_recovery;
pub mod mobile_tasks;
pub(crate) mod magpie;
pub(crate) mod opencode_go;
pub mod operation_api;
pub mod plugin_api;
pub mod scheduled_task_api;
pub mod skill_api;
pub mod system_api;
pub mod todo_api;
pub mod token_statistics_api;
pub mod updater_api;

#[cfg(test)]
mod tests;

pub(crate) mod butler_assistant_tools;
