#![recursion_limit = "256"]

mod api;
mod artifacts;
mod db;
mod errors;
mod mcp;
mod plugin;
mod scheduler;
mod skills;
mod state;
mod template_engine;
mod utils;
mod window;

use crate::api::ai_api::{
    ask_ai, cancel_ai, get_activity_focus, regenerate_ai, regenerate_conversation_title,
    tool_result_continue_ask_ai,
};
use crate::api::assistant_api::{
    add_assistant, bulk_update_assistant_mcp_tools, copy_assistant, delete_assistant,
    export_assistant, get_assistant, get_assistant_field_value,
    get_assistant_mcp_servers_with_tools, get_assistants, import_assistant, save_assistant,
    update_assistant_mcp_config, update_assistant_mcp_tool_config,
    update_assistant_model_config_value,
};
use crate::api::attachment_api::{add_attachment, open_attachment_with_default_app};
use crate::api::conversation_api::{
    create_conversation_with_messages, create_message, delete_conversation, fork_conversation,
    get_conversation_with_messages, list_conversations, update_assistant_message,
    search_conversations, update_conversation, update_message_content,
};
use crate::api::copilot_api::{poll_github_copilot_token, start_github_copilot_device_flow};
#[cfg(desktop)]
use crate::api::copilot_lsp::{
    check_copilot_status, get_copilot_lsp_status, get_copilot_oauth_token_from_config,
    sign_in_confirm, sign_in_initiate, sign_out_copilot, stop_copilot_lsp, CopilotLspState,
};
use crate::api::highlight_api::{highlight_code, list_syntect_themes};
use crate::api::llm_api::{
    add_llm_model, add_llm_provider, delete_llm_model, delete_llm_provider, export_llm_provider,
    fetch_model_list, get_filtered_models_for_select, get_filtered_providers, get_llm_models,
    get_llm_provider_config, get_llm_providers, get_models_for_select, import_llm_provider,
    preview_model_list, update_llm_provider, update_llm_provider_config, update_selected_models,
};
use crate::api::operation_api::{confirm_acp_permission, confirm_operation_permission};
use crate::api::scheduled_task_api::{
    create_scheduled_task, delete_scheduled_task, list_scheduled_task_logs, list_scheduled_tasks,
    list_scheduled_task_runs, run_scheduled_task_now, update_scheduled_task,
};
use crate::api::ai::acp::AcpPermissionState;
use crate::api::skill_api::{
    bulk_update_assistant_skills, cleanup_orphaned_skill_configs, delete_skill, fetch_official_skills,
    get_assistant_skills, get_enabled_assistant_skills, get_skill, get_skill_content,
    get_skill_sources, get_skills_directory, install_official_skill, open_skill_parent_folder,
    open_skills_folder, open_source_url, remove_assistant_skill, scan_skills, skill_exists,
    toggle_assistant_skill, update_assistant_skill_config,
};
use crate::api::sub_task_api::{
    cancel_sub_task_execution, cancel_sub_task_execution_for_ui, create_sub_task_execution,
    delete_sub_task_definition, get_sub_task_definition, get_sub_task_execution_detail,
    get_sub_task_execution_detail_for_ui, get_sub_task_mcp_calls_for_ui, list_sub_task_definitions,
    list_sub_task_executions, register_sub_task_definition, run_sub_task_sync,
    run_sub_task_with_mcp_loop, sub_task_regist, update_sub_task_definition,
};
use crate::api::system_api::{
    copy_image_to_clipboard, get_all_feature_config, get_autostart_state, get_bang_list,
    get_selected_text_api, open_data_folder, open_image, resume_global_shortcut,
    save_feature_config, set_autostart, set_shortcut_recording, suspend_global_shortcut,
};
use crate::api::token_statistics_api::{get_conversation_token_stats, get_message_token_stats};
use crate::api::updater_api::{
    check_update, check_update_with_proxy, download_and_install_update,
    download_and_install_update_with_proxy, get_app_version,
};
use crate::artifacts::artifacts_db::ArtifactsDatabase;
use crate::artifacts::collection_api::{
    delete_artifact_collection, generate_artifact_metadata, get_artifact_by_id,
    get_artifacts_collection, get_artifacts_for_completion, get_artifacts_statistics,
    open_artifact_window, save_artifact_to_collection, search_artifacts_collection,
    update_artifact_collection,
};
use crate::artifacts::env_installer::{
    check_acp_library, check_bun_update, check_bun_update_with_proxy, check_bun_version,
    check_uv_update, check_uv_update_with_proxy, check_uv_version, get_python_info, install_acp_library,
    install_bun, install_python3, install_uv, update_bun, update_bun_with_proxy, update_uv,
    update_uv_with_proxy,
};
use crate::artifacts::preview_router::{
    confirm_environment_install, preview_react_component, restore_artifact_preview,
    retry_preview_after_install, run_artifacts,
};
use crate::artifacts::react_preview::{
    close_react_preview, create_react_preview, create_react_preview_for_artifact,
};
use crate::artifacts::vue_preview::{
    close_vue_preview, create_vue_preview, create_vue_preview_for_artifact,
};
use crate::artifacts::{
    react_runner::{close_react_artifact, run_react_artifact},
    vue_runner::{close_vue_artifact, run_vue_artifact},
};
use crate::db::assistant_db::AssistantDatabase;
use crate::db::llm_db::LLMDatabase;
use crate::db::mcp_db::MCPDatabase;
use crate::db::scheduled_task_db::ScheduledTaskDatabase;
use crate::db::sub_task_db::SubTaskDatabase;
use crate::db::system_db::SystemDatabase;
use crate::mcp::builtin_mcp::{
    add_or_update_aipp_builtin_server, execute_aipp_builtin_tool, init_builtin_mcp_servers,
    list_aipp_builtin_templates, OperationState,
};
use crate::mcp::execution_api::{
    continue_with_error, create_mcp_tool_call, execute_mcp_tool_call, get_mcp_tool_call,
    get_mcp_tool_calls_by_conversation, send_mcp_tool_results, stop_mcp_tool_call,
};
use crate::mcp::registry_api::{
    add_mcp_server,
    build_mcp_prompt,
    check_disable_agent_mcp,
    check_disable_assistant_agent_mcp,
    check_disable_assistant_operation_mcp,
    check_disable_operation_mcp,
    // Skills 与操作 MCP 联动校验 API
    check_operation_mcp_for_skills,
    delete_mcp_server,
    disable_agent_mcp_with_skills,
    disable_assistant_agent_mcp_with_skills,
    disable_assistant_operation_mcp_with_skills,
    disable_operation_mcp_with_skills,
    enable_operation_mcp_and_skill,
    enable_operation_mcp_and_skills,
    get_mcp_provider,
    get_mcp_server,
    get_mcp_server_prompts,
    get_mcp_server_resources,
    get_mcp_server_tools,
    get_mcp_servers,
    refresh_mcp_server_capabilities,
    test_mcp_connection,
    toggle_mcp_server,
    update_mcp_server,
    update_mcp_server_prompt,
    update_mcp_server_tool,
};
use crate::window::{
    awaken_aipp, create_ask_window, create_chat_ui_window_hidden, create_config_window_hidden,
    create_schedule_window_hidden, ensure_hidden_search_window, handle_open_ask_window,
    open_artifact_collections_window, open_artifact_preview_window, open_chat_ui_window,
    open_chat_ui_window_inner, open_config_window, open_config_window_inner, open_plugin_window,
    open_schedule_window,
};
use db::conversation_db::ConversationDatabase;
use db::database_upgrade;
use db::plugin_db::PluginDatabase;
use db::system_db::FeatureConfig;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use get_selected_text::get_selected_text;
use serde::{Deserialize, Serialize};
use state::message_token::MessageTokenManager;
use state::activity_state::ConversationActivityManager;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::path::BaseDirectory;
use tauri::Emitter;
#[cfg(desktop)]
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconEvent},
    Manager, RunEvent,
};
#[cfg(mobile)]
use tauri::{Manager, RunEvent};
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, info, warn};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

struct AppState {
    selected_text: TokioMutex<String>,
    recording_shortcut: TokioMutex<bool>,
}

#[derive(Clone)]
struct AcpSessionState {
    sessions: Arc<TokioMutex<HashMap<i64, crate::api::ai::acp::AcpSessionHandle>>>,
}

impl AcpSessionState {
    fn new() -> Self {
        Self {
            sessions: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }
}

#[derive(Clone)]
struct FeatureConfigState {
    configs: Arc<TokioMutex<Vec<FeatureConfig>>>,
    config_feature_map: Arc<TokioMutex<HashMap<String, HashMap<String, FeatureConfig>>>>,
}

#[derive(Clone)]
struct NameCacheState {
    assistant_names: Arc<TokioMutex<HashMap<i64, String>>>,
    model_names: Arc<TokioMutex<HashMap<i64, String>>>,
}

#[derive(Serialize, Deserialize)]
struct Config {
    selected_text: String,
}

#[cfg(target_os = "macos")]
fn query_accessibility_permissions() -> bool {
    let trusted = macos_accessibility_client::accessibility::application_is_trusted();
    if trusted {
        print!("Application is totally trusted!");
    } else {
        print!("Application isn't trusted :(");
        // let trusted = macos_accessibility_client::accessibility::application_is_trusted_with_prompt();
        // return trusted;
    }
    trusted
}

#[cfg(not(target_os = "macos"))]
fn query_accessibility_permissions() -> bool {
    return true;
}

#[tauri::command]
async fn get_selected() -> Result<String, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        // First try native selected-text crate
        let mut result = get_selected_text().unwrap_or_default();

        // Fallback on macOS: simulate Cmd+C and read from clipboard, then restore clipboard
        #[cfg(target_os = "macos")]
        if result.is_empty() {
            if let Some(fallback) = copy_selection_via_clipboard_fallback() {
                result = fallback;
            }
        }
        debug!(?result, "initialization result");
        Ok(result)
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        Ok(String::new())
    }
}

#[tauri::command]
async fn save_config(state: tauri::State<'_, AppState>, config: Config) -> Result<(), String> {
    let mut selected_text = state.selected_text.lock().await;
    *selected_text = config.selected_text;
    Ok(())
}

#[tauri::command]
async fn get_config(state: tauri::State<'_, AppState>) -> Result<Config, String> {
    let selected_text = state.selected_text.lock().await;
    Ok(Config { selected_text: selected_text.clone() })
}

#[cfg(target_os = "macos")]
fn read_clipboard_text() -> Option<String> {
    use std::process::{Command, Stdio};
    let output = Command::new("pbpaste").stdout(Stdio::piped()).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "macos")]
fn write_clipboard_text(text: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    if let Ok(mut child) = Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

#[cfg(target_os = "macos")]
fn copy_selection_via_clipboard_fallback() -> Option<String> {
    use std::thread;
    use std::time::Duration;
    let t_total = std::time::Instant::now();

    // Save current clipboard content
    let t_prev = std::time::Instant::now();
    let previous = read_clipboard_text().unwrap_or_default();
    let prev_ms = t_prev.elapsed().as_millis();

    // Ask the frontmost app to copy selection
    let t_apple = std::time::Instant::now();
    if let Err(e) = crate::artifacts::applescript::run_applescript(
        "tell application \"System Events\" to keystroke \"c\" using {command down}",
    ) {
        debug!(error=%format!("{:?}", e), "AppleScript copy failed");
    }

    // Wait a bit for clipboard to update
    thread::sleep(Duration::from_millis(180));
    let apple_ms = t_apple.elapsed().as_millis();

    // Read new clipboard
    let t_new = std::time::Instant::now();
    let new_clip = read_clipboard_text().unwrap_or_default();
    let new_ms = t_new.elapsed().as_millis();

    // Restore previous clipboard (best effort)
    let t_restore = std::time::Instant::now();
    write_clipboard_text(&previous);
    let restore_ms = t_restore.elapsed().as_millis();

    let total_ms = t_total.elapsed().as_millis();
    info!(total_ms=%total_ms, read_prev_ms=%prev_ms, apple_copy_ms=%apple_ms, read_new_ms=%new_ms, restore_ms=%restore_ms, "Clipboard fallback timings");

    if new_clip.is_empty() || new_clip == previous {
        None
    } else {
        Some(new_clip)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 初始化 tracing 日志 (RUST_LOG 环境变量可覆盖)
    // dev 构建默认 debug，release 构建默认 info
    if std::env::var("RUST_LOG").is_err() {
        let default_log = if cfg!(debug_assertions) {
            "debug,Aipp=debug,aipp=debug,rmcp=debug"
        } else {
            "info,Aipp=info,aipp=info,rmcp=warn"
        };
        std::env::set_var("RUST_LOG", default_log);
    }
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .with_line_number(true)
        .with_thread_ids(false)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["com.xieisabug.aipp"]),
        ))
        .setup(|app| {
            let app_handle = app.handle();

            // 系统托盘菜单和图标初始化
            #[cfg(desktop)]
            {
                let ask_item = MenuItemBuilder::with_id("ask", "Ask").build(app)?;
                let chat_item = MenuItemBuilder::with_id("chat", "Chat").build(app)?;
                let config_item = MenuItemBuilder::with_id("config", "配置").build(app)?;
                let separator = PredefinedMenuItem::separator(app)?;
                let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;
                let tray_menu = MenuBuilder::new(app)
                    .items(&[&ask_item, &chat_item, &config_item, &separator, &quit_item])
                    .build()?;

                let tray = app.tray_by_id("aipp").unwrap();
                tray.set_menu(Some(tray_menu))?;

                // 左键点击直接打开 Ask 窗口
                let app_handle_for_click = app_handle.clone();
                tray.on_tray_icon_event(move |_app, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        handle_open_ask_window(&app_handle_for_click);
                    }
                });

                // 右键菜单事件处理
                tray.on_menu_event(move |app, event| match event.id().as_ref() {
                    "ask" => {
                        handle_open_ask_window(app);
                    }
                    "chat" => {
                        if let Some(chat_window) = app.get_webview_window("chat_ui") {
                            open_chat_ui_window_inner(app, &chat_window);
                        } else {
                            crate::window::create_chat_ui_window(app);
                        }
                    }
                    "config" => {
                        if let Some(config_window) = app.get_webview_window("config") {
                            open_config_window_inner(app, &config_window);
                        } else {
                            crate::window::create_config_window(app);
                        }
                    }
                    "quit" => {
                        std::process::exit(0);
                    }
                    _ => {}
                });
                let _ = tray.set_show_menu_on_left_click(false);
            }

            let resource_path = app.path().resolve(
                "artifacts/templates/react/PreviewReactWindow.tsx",
                BaseDirectory::Resource,
            )?;
            debug!(?resource_path, "resource path");

            let system_db = SystemDatabase::new(&app_handle)?;
            let llm_db = LLMDatabase::new(&app_handle)?;
            let assistant_db = AssistantDatabase::new(&app_handle)?;
            let conversation_db = ConversationDatabase::new(&app_handle)?;
            let plugin_db = PluginDatabase::new(&app_handle)?;
            let mcp_db = MCPDatabase::new(&app_handle)?;
            let sub_task_db = SubTaskDatabase::new(&app_handle)?;
            let scheduled_task_db = ScheduledTaskDatabase::new(&app_handle)?;
            let artifacts_db = ArtifactsDatabase::new(&app_handle)?;
            let skill_db = db::skill_db::SkillDatabase::new(&app_handle)?;

            system_db.create_tables()?;
            llm_db.create_tables()?;
            assistant_db.create_tables()?;
            conversation_db.create_tables()?;
            plugin_db.create_tables()?;
            mcp_db.create_tables()?;
            sub_task_db.create_tables()?;
            scheduled_task_db.create_tables()?;
            artifacts_db.create_tables()?;
            skill_db.create_tables()?;

            // Migration: Remove old Claude Code agents/rules skill configs
            if let Err(e) = skill_db.migrate_claude_code_skills() {
                warn!(error = %e, "Failed to migrate Claude Code skills");
            }

            let _ = database_upgrade(&app_handle, system_db, llm_db, assistant_db, conversation_db);

            // 初始化内置工具集（搜索、操作），如果不存在则自动创建
            if let Err(e) = init_builtin_mcp_servers(&app_handle) {
                warn!(error = %e, "Failed to initialize builtin MCP servers");
            }

            app.manage(initialize_state(&app_handle));
            app.manage(initialize_name_cache_state(&app_handle));

            // 初始化并启动定时任务调度器
            let scheduler_state = scheduler::SchedulerState::new();
            app.manage(scheduler_state.clone());
            scheduler::start_scheduler(app_handle.clone(), scheduler_state);

            // 注册全局快捷键（必须在 state 初始化之后）
            #[cfg(desktop)]
            {
                register_global_shortcuts(&app_handle);
            }

            if app.get_webview_window("main").is_none() {
                // 移动端直接启动 chat_ui 窗口
                #[cfg(mobile)]
                {
                    create_chat_ui_window(&app_handle);
                }
                #[cfg(desktop)]
                {
                    create_chat_ui_window_hidden(&app_handle);
                    create_config_window_hidden(&app_handle);
                    create_schedule_window_hidden(&app_handle);
                    create_ask_window(&app_handle);
                }
            }

            Ok(())
        })
        .manage(AppState {
            selected_text: TokioMutex::new(String::new()),
            recording_shortcut: TokioMutex::new(false),
        })
        .manage(AcpSessionState::new())
        .manage(MessageTokenManager::new())
        .manage(ConversationActivityManager::new())
        .manage(OperationState::new())
        .manage(AcpPermissionState::new());
    #[cfg(desktop)]
    let app = app.manage(CopilotLspState::default());
    let app = app
        .invoke_handler(tauri::generate_handler![
            ask_ai,
            tool_result_continue_ask_ai,
            regenerate_ai,
            get_activity_focus,
            regenerate_conversation_title,
            generate_artifact_metadata,
            cancel_ai,
            get_selected,
            open_config_window,
            open_chat_ui_window,
            open_plugin_window,
            open_schedule_window,
            open_artifact_preview_window,
            save_config,
            get_config,
            get_all_feature_config,
            save_feature_config,
            open_data_folder,
            get_llm_providers,
            get_filtered_providers,
            update_llm_provider,
            add_llm_provider,
            delete_llm_provider,
            get_llm_provider_config,
            update_llm_provider_config,
            get_llm_models,
            fetch_model_list,
            preview_model_list,
            update_selected_models,
            get_models_for_select,
            get_filtered_models_for_select,
            add_llm_model,
            delete_llm_model,
            export_llm_provider,
            import_llm_provider,
            add_attachment,
            open_attachment_with_default_app,
            get_assistants,
            get_assistant,
            get_assistant_field_value,
            save_assistant,
            add_assistant,
            delete_assistant,
            copy_assistant,
            export_assistant,
            import_assistant,
            list_conversations,
            search_conversations,
            get_conversation_with_messages,
            create_conversation_with_messages,
            delete_conversation,
            fork_conversation,
            update_conversation,
            update_message_content,
            run_artifacts,
            restore_artifact_preview,
            save_artifact_to_collection,
            get_artifacts_collection,
            get_artifact_by_id,
            search_artifacts_collection,
            update_artifact_collection,
            delete_artifact_collection,
            open_artifact_window,
            open_artifact_collections_window,
            get_artifacts_statistics,
            get_artifacts_for_completion,
            get_bang_list,
            get_selected_text_api,
            set_shortcut_recording,
            suspend_global_shortcut,
            resume_global_shortcut,
            copy_image_to_clipboard,
            open_image,
            check_bun_version,
            check_uv_version,
            install_bun,
            install_uv,
            check_bun_update,
            check_bun_update_with_proxy,
            check_uv_update,
            check_uv_update_with_proxy,
            update_bun,
            update_bun_with_proxy,
            update_uv,
            update_uv_with_proxy,
            get_python_info,
            install_python3,
            check_acp_library,
            install_acp_library,
            preview_react_component,
            create_react_preview,
            create_react_preview_for_artifact,
            close_react_preview,
            create_vue_preview,
            create_vue_preview_for_artifact,
            close_vue_preview,
            run_react_artifact,
            close_react_artifact,
            run_vue_artifact,
            close_vue_artifact,
            confirm_environment_install,
            retry_preview_after_install,
            get_mcp_servers,
            get_mcp_server,
            get_mcp_provider,
            build_mcp_prompt,
            create_message,
            update_assistant_message,
            add_mcp_server,
            update_mcp_server,
            delete_mcp_server,
            toggle_mcp_server,
            get_mcp_server_tools,
            update_mcp_server_tool,
            get_mcp_server_resources,
            get_mcp_server_prompts,
            update_mcp_server_prompt,
            test_mcp_connection,
            refresh_mcp_server_capabilities,
            // Skills 与操作 MCP 联动校验 API
            check_operation_mcp_for_skills,
            enable_operation_mcp_and_skill,
            enable_operation_mcp_and_skills,
            check_disable_operation_mcp,
            disable_operation_mcp_with_skills,
            check_disable_agent_mcp,
            disable_agent_mcp_with_skills,
            check_disable_assistant_operation_mcp,
            disable_assistant_operation_mcp_with_skills,
            check_disable_assistant_agent_mcp,
            disable_assistant_agent_mcp_with_skills,
            get_assistant_mcp_servers_with_tools,
            update_assistant_mcp_config,
            update_assistant_mcp_tool_config,
            bulk_update_assistant_mcp_tools,
            update_assistant_model_config_value,
            start_github_copilot_device_flow,
            poll_github_copilot_token,
            // Copilot LSP commands
            stop_copilot_lsp,
            check_copilot_status,
            sign_in_initiate,
            sign_in_confirm,
            sign_out_copilot,
            get_copilot_lsp_status,
            get_copilot_oauth_token_from_config,
            create_mcp_tool_call,
            execute_mcp_tool_call,
            get_mcp_tool_call,
            get_mcp_tool_calls_by_conversation,
            stop_mcp_tool_call,
            continue_with_error,
            send_mcp_tool_results,
            list_aipp_builtin_templates,
            add_or_update_aipp_builtin_server,
            execute_aipp_builtin_tool,
            confirm_operation_permission,
            confirm_acp_permission,
            register_sub_task_definition,
            run_sub_task_sync,
            run_sub_task_with_mcp_loop,
            sub_task_regist,
            list_sub_task_definitions,
            get_sub_task_definition,
            update_sub_task_definition,
            delete_sub_task_definition,
            create_sub_task_execution,
            list_sub_task_executions,
            get_sub_task_execution_detail,
            get_sub_task_execution_detail_for_ui,
            cancel_sub_task_execution,
            get_sub_task_mcp_calls_for_ui,
            cancel_sub_task_execution_for_ui,
            highlight_code,
            ensure_hidden_search_window,
            list_syntect_themes,
            // Skill commands
            scan_skills,
            get_skill_sources,
            get_skill_content,
            get_skill,
            skill_exists,
            get_assistant_skills,
            get_enabled_assistant_skills,
            update_assistant_skill_config,
            toggle_assistant_skill,
            remove_assistant_skill,
            bulk_update_assistant_skills,
            cleanup_orphaned_skill_configs,
            open_skills_folder,
            open_skill_parent_folder,
            get_skills_directory,
            fetch_official_skills,
            install_official_skill,
            open_source_url,
            delete_skill,
            // Token statistics commands
            get_conversation_token_stats,
            get_message_token_stats,
            // Autostart commands
            get_autostart_state,
            set_autostart,
            // Updater commands
            check_update,
            check_update_with_proxy,
            download_and_install_update,
            download_and_install_update_with_proxy,
            get_app_version,
            list_scheduled_tasks,
            create_scheduled_task,
            update_scheduled_task,
            delete_scheduled_task,
            run_scheduled_task_now,
            list_scheduled_task_logs,
            list_scheduled_task_runs,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(|_app_handle, e| match e {
        RunEvent::ExitRequested { api, .. } => {
            api.prevent_exit();
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            awaken_aipp(_app_handle);
        }
        _ => {}
    });
}

fn initialize_state(app_handle: &tauri::AppHandle) -> FeatureConfigState {
    let db = SystemDatabase::new(app_handle).expect("Failed to connect to database");
    let configs = db.get_all_feature_config().expect("Failed to load feature configs");
    let mut configs_map = HashMap::new();
    for config in configs.clone().into_iter() {
        let feature_code = config.feature_code.clone();
        let key = config.key.clone();
        configs_map
            .entry(feature_code.clone())
            .or_insert(HashMap::new())
            .insert(key.clone(), config);
    }
    FeatureConfigState {
        configs: Arc::new(TokioMutex::new(configs)),
        config_feature_map: Arc::new(TokioMutex::new(configs_map)),
    }
}

fn initialize_name_cache_state(app_handle: &tauri::AppHandle) -> NameCacheState {
    let assistant_db = AssistantDatabase::new(app_handle).expect("Failed to connect to database");
    let assistants = assistant_db.get_assistants().expect("Failed to load assistants");
    let mut assistant_names = HashMap::new();
    for assistant in assistants.clone().into_iter() {
        assistant_names.insert(assistant.id, assistant.name.clone());
    }

    let llm_db = LLMDatabase::new(app_handle).expect("Failed to connect to database");
    let models = llm_db.get_models_for_select().expect("Failed to load models");
    let mut model_names = HashMap::new();
    for model in models.clone().into_iter() {
        model_names.insert(model.2, model.0);
    }

    NameCacheState {
        assistant_names: Arc::new(TokioMutex::new(assistant_names)),
        model_names: Arc::new(TokioMutex::new(model_names)),
    }
}

#[cfg(desktop)]
pub(crate) fn register_global_shortcuts(app_handle: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    info!("开始注册全局快捷键...");

    // 先安装插件（只安装一次即可）。若已安装会返回错误，忽略即可。
    let _ = app_handle.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|_app, _shortcut, event| {
                // 仅在按键释放时触发
                if event.state() == ShortcutState::Released {
                    let t_event = std::time::Instant::now();
                    info!("Global shortcut released: start handling");
                    // 如果正在录入快捷键，忽略全局事件
                    if let Some(state) = _app.try_state::<AppState>() {
                        if *state.recording_shortcut.blocking_lock() {
                            debug!("正在录入快捷键，忽略全局快捷键事件");
                            return;
                        }
                    }

                    // macOS：使用“先复制，再延迟聚焦，再后台读取剪贴板”的策略，绕过慢速 crate
                    #[cfg(target_os = "macos")]
                    {
                        use std::thread;
                        use std::time::Duration;

                        // 1) 读取当前剪贴板
                        let t_prev = std::time::Instant::now();
                        let previous = read_clipboard_text().unwrap_or_default();
                        let prev_ms = t_prev.elapsed().as_millis();

                        // 2) 立刻向前台应用发送 Cmd+C（尽量不等我们窗口抢焦点）
                        let t_apple = std::time::Instant::now();
                        if let Err(e) = crate::artifacts::applescript::run_applescript(
                            "tell application \"System Events\" to keystroke \"c\" using {command down}"
                        ) {
                            debug!(error=%format!("{:?}", e), "AppleScript copy dispatch failed");
                        }
                        let apple_ms = t_apple.elapsed().as_millis();

                        // 3) 稍等 25ms 再打开/聚焦 Ask 窗口，给前台应用时间处理复制
                        thread::sleep(Duration::from_millis(25));
                        let t_open = std::time::Instant::now();
                        handle_open_ask_window(_app);
                        info!(elapsed_ms=%t_open.elapsed().as_millis(), "Ask window opened/focused (mac, delayed)");

                        // 4) 后台线程：等待 150~180ms，读取剪贴板，恢复之前内容，若变更则发事件
                        let app_handle = _app.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            use std::thread;
                            use std::time::Duration;
                            let t_worker = std::time::Instant::now();
                            thread::sleep(Duration::from_millis(160));
                            let t_new = std::time::Instant::now();
                            let new_clip = read_clipboard_text().unwrap_or_default();
                            let new_ms = t_new.elapsed().as_millis();
                            let t_restore = std::time::Instant::now();
                            write_clipboard_text(&previous);
                            let restore_ms = t_restore.elapsed().as_millis();
                            info!(prev_ms=%prev_ms, apple_ms=%apple_ms, read_new_ms=%new_ms, restore_ms=%restore_ms, worker_total_ms=%t_worker.elapsed().as_millis(), "Copy-first worker timings (mac)");

                            if !new_clip.is_empty() && new_clip != previous {
                                let _ = app_handle.emit("get_selected_text_event", new_clip.clone());
                                if let Some(state) = app_handle.try_state::<AppState>() {
                                    *state.selected_text.blocking_lock() = new_clip;
                                }
                            } else {
                                debug!("Copy-first worker: no new selection or same as previous");
                            }
                        });

                        let dt_total = t_event.elapsed().as_millis();
                        info!(elapsed_ms=%dt_total, "Global shortcut (mac) initial path done");
                        return;
                    }

                    // 其他平台：保留原有快速获取 + 异步回退
                    #[cfg(not(target_os = "macos"))]
                    {
                        // 先尝试快速获取（不阻塞 UI）
                        let mut immediate: Option<String> = None;
                        let t_sel = std::time::Instant::now();
                        match get_selected_text() {
                            Ok(t) if !t.is_empty() => {
                                let dt = t_sel.elapsed().as_millis();
                                info!(elapsed_ms=%dt, len=t.len(), "Fast selected-text attempt succeeded");
                                immediate = Some(t)
                            }
                            Ok(_) => {
                                let dt = t_sel.elapsed().as_millis();
                                info!(elapsed_ms=%dt, "Fast selected-text attempt empty");
                            }
                            Err(e) => {
                                let dt = t_sel.elapsed().as_millis();
                                warn!(elapsed_ms=%dt, error=%e.to_string(), "Fast selected-text attempt failed");
                            }
                        }

                        // 立即打开 Ask 窗口，提升唤醒速度
                        let t_open = std::time::Instant::now();
                        handle_open_ask_window(_app);
                        let dt_open = t_open.elapsed().as_millis();
                        info!(elapsed_ms=%dt_open, "Ask window opened/focused (handle_open_ask_window)");

                        // 如果快速获取到了，立刻发给前端并缓存
                        if let Some(text) = immediate {
                            if !text.is_empty() {
                                let _ = _app.emit("get_selected_text_event", text.clone());
                                if let Some(state) = _app.try_state::<AppState>() {
                                    *state.selected_text.blocking_lock() = text;
                                }
                                let dt_total = t_event.elapsed().as_millis();
                                info!(elapsed_ms=%dt_total, "Global shortcut handling done (no fallback needed)");
                                return;
                            }
                        }

                        let dt_total = t_event.elapsed().as_millis();
                        info!(elapsed_ms=%dt_total, "Global shortcut handling finished initial path");
                    }
                }
            })
            .build(),
    );

    // 根据配置计算需要注册的快捷键字符串（global-hotkey 解析格式）
    let (shortcut_str, from_fallback) = {
        let state = app_handle.state::<FeatureConfigState>();
        let config_feature_map = state.config_feature_map.blocking_lock();
        if let Some(shortcuts_cfg) = config_feature_map.get("shortcuts") {
            if let Some(sc) = shortcuts_cfg.get("shortcut") {
                (sc.value.clone(), false)
            } else {
                // 兼容旧字段：modifier_key + Space
                let modifier = shortcuts_cfg
                    .get("modifier_key")
                    .map(|c| c.value.clone())
                    .unwrap_or_else(|| {
                        #[cfg(target_os = "macos")]
                        {
                            "option".to_string()
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            "alt".to_string()
                        }
                    });
                let mk = modifier.to_lowercase();
                let mod_token = if mk == "ctrl" || mk == "control" {
                    "Ctrl"
                } else if mk == "shift" {
                    "Shift"
                } else if mk == "cmd" || mk == "command" || mk == "super" {
                    #[cfg(target_os = "macos")]
                    {
                        "Command"
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        "Super"
                    }
                } else if mk == "option" || mk == "alt" {
                    "Alt"
                } else {
                    #[cfg(target_os = "macos")]
                    {
                        "Option"
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        "Alt"
                    }
                };
                (format!("{}+Space", mod_token), true)
            }
        } else {
            // 默认值
            #[cfg(target_os = "macos")]
            let s = "Option+Space".to_string();
            #[cfg(not(target_os = "macos"))]
            let s = "Alt+Space".to_string();
            (s, true)
        }
    };

    // 先清空旧注册，再注册新快捷键
    if let Err(e) = app_handle.global_shortcut().unregister_all() {
        debug!(error=%e, "卸载旧全局快捷键失败或未注册，继续");
    }

    match app_handle.global_shortcut().register(shortcut_str.as_str()) {
        Ok(_) => {
            if from_fallback {
                info!("✓ 成功注册全局快捷键(回退): {}", shortcut_str);
            } else {
                info!("✓ 成功注册全局快捷键: {}", shortcut_str);
            }
        }
        Err(e) => {
            warn!(error=%e, shortcut=%shortcut_str, "无法注册全局快捷键 (可能格式无效或被占用)");
        }
    }
}

#[cfg(desktop)]
pub(crate) async fn reconfigure_global_shortcuts_async(app_handle: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    info!("开始重新注册全局快捷键(异步)...");

    // 计算当前配置的快捷键字符串（异步锁避免阻塞 runtime）
    let shortcut_str = {
        let state = app_handle.state::<FeatureConfigState>();
        let config_feature_map = state.config_feature_map.lock().await;
        if let Some(shortcuts_cfg) = config_feature_map.get("shortcuts") {
            if let Some(sc) = shortcuts_cfg.get("shortcut") {
                sc.value.clone()
            } else {
                // 回退基于旧字段 modifier_key
                let modifier = shortcuts_cfg
                    .get("modifier_key")
                    .map(|c| c.value.clone())
                    .unwrap_or_else(|| {
                        #[cfg(target_os = "macos")]
                        {
                            "option".to_string()
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            "alt".to_string()
                        }
                    });
                let mk = modifier.to_lowercase();
                let mod_token = if mk == "ctrl" || mk == "control" {
                    "Ctrl"
                } else if mk == "shift" {
                    "Shift"
                } else if mk == "cmd" || mk == "command" || mk == "super" {
                    #[cfg(target_os = "macos")]
                    {
                        "Command"
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        "Super"
                    }
                } else if mk == "option" || mk == "alt" {
                    "Alt"
                } else {
                    #[cfg(target_os = "macos")]
                    {
                        "Option"
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        "Alt"
                    }
                };
                format!("{}+Space", mod_token)
            }
        } else {
            // 默认值
            #[cfg(target_os = "macos")]
            let s = "Option+Space".to_string();
            #[cfg(not(target_os = "macos"))]
            let s = "Alt+Space".to_string();
            s
        }
    };

    // 重新注册
    if let Err(e) = app_handle.global_shortcut().unregister_all() {
        debug!(error=%e, "卸载旧全局快捷键失败或未注册，继续");
    }
    match app_handle.global_shortcut().register(shortcut_str.as_str()) {
        Ok(_) => info!("✓ 成功注册全局快捷键: {}", shortcut_str),
        Err(e) => {
            warn!(error=%e, shortcut=%shortcut_str, "无法注册全局快捷键 (可能格式无效或被占用)")
        }
    }
}
