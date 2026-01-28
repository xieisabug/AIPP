pub mod ai;
pub mod ai_api;
pub mod assistant_api;
pub mod attachment_api;
pub mod conversation_api;
pub mod copilot_api;
#[cfg(desktop)]
pub mod copilot_lsp;
pub mod genai_client;
pub mod highlight_api;
pub mod llm_api;
pub mod operation_api;
pub mod scheduled_task_api;
pub mod skill_api;
pub mod sub_task_api;
pub mod system_api;
pub mod todo_api;
pub mod token_statistics_api;
pub mod updater_api;

#[cfg(test)]
mod tests;
