#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]
#![recursion_limit = "256"]

mod api;
mod artifacts;
mod db;
mod entity; // re-exported SeaORM entities
mod errors;
mod mcp;
mod plugin;
mod state;
mod template_engine;
mod utils;
mod window;

use crate::api::ai_api::{
    ask_ai, cancel_ai, regenerate_ai, regenerate_conversation_title, tool_result_continue_ask_ai,
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
    update_conversation, update_message_content,
};
use crate::api::highlight_api::{highlight_code, list_syntect_themes};
use crate::api::llm_api::{
    add_llm_model, add_llm_provider, delete_llm_model, delete_llm_provider, export_llm_provider,
    fetch_model_list, get_llm_models, get_llm_provider_config, get_llm_providers,
    get_models_for_select, import_llm_provider, preview_model_list, update_llm_provider,
    update_llm_provider_config, update_selected_models,
};
use crate::api::sub_task_api::{
    cancel_sub_task_execution, cancel_sub_task_execution_for_ui, create_sub_task_execution,
    delete_sub_task_definition, get_sub_task_definition, get_sub_task_execution_detail,
    get_sub_task_execution_detail_for_ui, get_sub_task_mcp_calls_for_ui, list_sub_task_definitions,
    list_sub_task_executions, register_sub_task_definition, run_sub_task_sync,
    run_sub_task_with_mcp_loop, sub_task_regist, update_sub_task_definition,
};
use crate::api::system_api::{
    get_all_feature_config, get_bang_list, get_data_storage_config, get_selected_text_api,
    open_data_folder, resume_global_shortcut, save_data_storage_config, save_feature_config,
    set_shortcut_recording, suspend_global_shortcut, test_remote_storage_connection,
    upload_local_data,
};
use crate::artifacts::artifacts_db::ArtifactsDatabase;
use crate::artifacts::collection_api::{
    delete_artifact_collection, generate_artifact_metadata, get_artifact_by_id,
    get_artifacts_collection, get_artifacts_for_completion, get_artifacts_statistics,
    open_artifact_window, save_artifact_to_collection, search_artifacts_collection,
    update_artifact_collection,
};
use crate::artifacts::env_installer::{
    check_bun_version, check_uv_version, install_bun, install_uv,
};
use crate::artifacts::preview_router::{
    confirm_environment_install, preview_react_component, retry_preview_after_install,
    run_artifacts,
};
use crate::artifacts::react_preview::{
    close_react_preview, create_react_preview, create_react_preview_for_artifact,
};
use crate::artifacts::vue_preview::{
    close_vue_preview, create_vue_preview, create_vue_preview_for_artifact,
};
// ==== Added foundational imports for state and utilities ====
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use serde::{Serialize, Deserialize};
use tracing::{debug, info, warn};
use tracing_subscriber::{FmtSubscriber, EnvFilter};
use tauri::{RunEvent, Manager, Emitter};
use get_selected_text::get_selected_text;

// Databases & upgrade helpers
use crate::db::{
    database_upgrade,
    system_db::SystemDatabase,
    llm_db::LLMDatabase,
    assistant_db::AssistantDatabase,
    conversation_db::ConversationDatabase,
    plugin_db::PluginDatabase,
    mcp_db::MCPDatabase,
    sub_task_db::SubTaskDatabase,
};

// Window helpers
use crate::window::{
    create_ask_window,
    handle_open_ask_window,
    awaken_aipp,
    open_config_window,
    open_chat_ui_window,
    open_plugin_window,
    open_plugin_store_window,
    open_artifact_preview_window,
    open_artifact_collections_window,
    ensure_hidden_search_window,
};
use crate::artifacts::react_runner::{run_react_artifact, close_react_artifact};
use crate::artifacts::vue_runner::{run_vue_artifact, close_vue_artifact};

// Message token manager
use crate::state::message_token::MessageTokenManager;

// MCP APIs
use crate::mcp::registry_api::{
    get_mcp_servers,
    get_mcp_server,
    get_mcp_provider,
    build_mcp_prompt,
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
};
use crate::mcp::execution_api::{
    create_mcp_tool_call,
    execute_mcp_tool_call,
    get_mcp_tool_call,
    get_mcp_tool_calls_by_conversation,
};
use crate::mcp::builtin_mcp::{
    list_aipp_builtin_templates,
    add_or_update_aipp_builtin_server,
    execute_aipp_builtin_tool,
};

// Menu & path utilities
use tauri::menu::{MenuItemBuilder, MenuBuilder};
use tauri::path::BaseDirectory;

// FeatureConfig type
use crate::db::system_db::FeatureConfig;

// SeaORM database connection
use sea_orm::DatabaseConnection;

// ===== Application state structs =====
pub struct AppState {
    pub selected_text: TokioMutex<String>,
    pub recording_shortcut: TokioMutex<bool>,
}

#[derive(Clone)]
pub struct DatabaseState {
    pub conn: Arc<DatabaseConnection>,
}

#[derive(Clone)]
pub struct DataStorageState {
    pub flat: Arc<TokioMutex<HashMap<String, String>>>,
}

#[derive(Clone)]
pub struct FeatureConfigState {
    pub configs: Arc<TokioMutex<Vec<FeatureConfig>>>,
    pub config_feature_map: Arc<TokioMutex<HashMap<String, HashMap<String, FeatureConfig>>>>,
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
    // First try native selected-text crate
    let result = get_selected_text().unwrap_or_default();

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化 tracing 日志 (RUST_LOG 环境变量可覆盖，默认 info)
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info,Aipp=info,aipp=info,rmcp=warn");
    }
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .with_line_number(true)
        .with_thread_ids(false)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let t_setup_start = std::time::Instant::now();
            info!("=== Application setup started ===");
            
            // 创建 Tokio runtime 用于所有数据库操作，避免重复创建临时 runtime
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create Tokio runtime: {}", e))?;

            let app_handle = app.handle();

            // 系统托盘菜单和图标初始化
            let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
            let show = MenuItemBuilder::with_id("show", "显示").build(app)?;
            let tray_menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;

            let tray = app.tray_by_id("aipp").unwrap();
            tray.set_menu(Some(tray_menu))?;
            tray.on_menu_event(move |app, event| match event.id().as_ref() {
                "quit" => {
                    std::process::exit(0);
                }
                "show" => {
                    awaken_aipp(&app);
                }
                _ => {}
            });
            let _ = tray.set_show_menu_on_left_click(true);

            let resource_path = app.path().resolve(
                "artifacts/templates/react/PreviewReactWindow.tsx",
                BaseDirectory::Resource,
            )?;
            debug!(?resource_path, "resource path");

            // 在共享的 Tokio runtime 中执行所有数据库操作
            rt.block_on(async {
                // 1. 首先使用本地 SQLite system.db 读取 data_storage 配置
                let t_system_new = std::time::Instant::now();
                let system_db = SystemDatabase::new(&app_handle)?;
                info!(elapsed_ms=%t_system_new.elapsed().as_millis(), "SystemDatabase::new() completed");
                
                let t_system_table = std::time::Instant::now();
                system_db.create_tables(&app_handle)?;
                info!(elapsed_ms=%t_system_table.elapsed().as_millis(), "SystemDatabase::create_tables() completed");

                // 2. 读取 data_storage 配置
                let t_config = std::time::Instant::now();
                let mut ds_flat: HashMap<String, String> = HashMap::new();
                if let Ok(items) = system_db.get_feature_config_by_feature_code(&app_handle, "data_storage") {
                    for c in items.into_iter() {
                        ds_flat.insert(c.key, c.value);
                    }
                }
                ds_flat.entry("storage_mode".to_string()).or_insert("local".to_string());
                info!(elapsed_ms=%t_config.elapsed().as_millis(), "Loading data_storage config completed");

                {
                    let storage_mode = ds_flat.get("storage_mode").cloned().unwrap_or_else(|| "local".into());
                    let remote_type = ds_flat.get("remote_type").cloned().unwrap_or_default();
                    let (host_key, port_key, db_key, user_key) = match remote_type.as_str() {
                        "postgresql" => ("pg_host", "pg_port", "pg_database", "pg_username"),
                        "mysql" => ("mysql_host", "mysql_port", "mysql_database", "mysql_username"),
                        _ => ("", "", "", ""),
                    };
                    let host = host_key.is_empty().then(|| "".to_string()).unwrap_or_else(|| ds_flat.get(host_key).cloned().unwrap_or_default());
                    let port = port_key.is_empty().then(|| "".to_string()).unwrap_or_else(|| ds_flat.get(port_key).cloned().unwrap_or_default());
                    let database = db_key.is_empty().then(|| "".to_string()).unwrap_or_else(|| ds_flat.get(db_key).cloned().unwrap_or_default());
                    let username_set = if user_key.is_empty() { false } else { ds_flat.get(user_key).is_some() };
                    debug!(storage_mode=%storage_mode, remote_type=%remote_type, host=%host, port=%port, database=%database, username_set=%username_set, "Data storage config snapshot");
                }
                
                let data_storage_state = DataStorageState { flat: Arc::new(TokioMutex::new(ds_flat.clone())) };
                app.manage(data_storage_state.clone());

                // 3. 根据配置建立全局数据库连接
                let t_conn = std::time::Instant::now();
                let db_conn = crate::utils::db_utils::initialize_database_connection(&app_handle, &ds_flat).await?;
                info!(elapsed_ms=%t_conn.elapsed().as_millis(), "Initialize global database connection completed");
                
                let db_state = DatabaseState { conn: Arc::new(db_conn) };
                app.manage(db_state.clone());

                // 4. 创建所有表
                let t_tables = std::time::Instant::now();
                LLMDatabase::create_tables(&app_handle)?;
                info!("LLMDatabase::create_tables() completed");
                
                AssistantDatabase::create_tables(&app_handle)?;
                info!("AssistantDatabase::create_tables() completed");
                
                ConversationDatabase::create_tables(&app_handle)?;
                info!("ConversationDatabase::create_tables() completed");
                
                PluginDatabase::create_tables(&app_handle)?;
                info!("PluginDatabase::create_tables() completed");
                
                MCPDatabase::create_tables(&app_handle)?;
                info!("MCPDatabase::create_tables() completed");
                
                SubTaskDatabase::create_tables(&app_handle)?;
                info!("SubTaskDatabase::create_tables() completed");
                
                ArtifactsDatabase::create_tables(&app_handle)?;
                info!(elapsed_ms=%t_tables.elapsed().as_millis(), "All create_tables() completed");

                // 5. 数据库升级
                let t_upgrade = std::time::Instant::now();
                let _ = database_upgrade(&app_handle);
                info!(elapsed_ms=%t_upgrade.elapsed().as_millis(), "database_upgrade() completed");

                // 6. 初始化其他状态
                let t_state = std::time::Instant::now();
                app.manage(initialize_state_with_db(&app_handle));
                info!(elapsed_ms=%t_state.elapsed().as_millis(), "initialize_state() completed");
                
                let t_cache = std::time::Instant::now();
                app.manage(initialize_name_cache_state_with_dbs(&app_handle));
                info!(elapsed_ms=%t_cache.elapsed().as_millis(), "initialize_name_cache_state_with_dbs() completed");

                Ok::<(), Box<dyn std::error::Error>>(())
            })?;

            // 安装全局快捷键插件（同步，尽早安装，但不读取配置、不注册具体按键）
            #[cfg(desktop)]
            {
                install_global_shortcut_plugin(&app_handle);
            }

            // 注册全局快捷键（在 setup 完成后异步执行，仅做 unregister/register，不安装插件）
            #[cfg(desktop)]
            {
                let app_clone = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    let t_shortcut = std::time::Instant::now();
                    register_global_shortcuts_async(&app_clone).await;
                    info!(elapsed_ms=%t_shortcut.elapsed().as_millis(), "register_global_shortcuts_async() completed");
                });
            }

            if app.get_webview_window("main").is_none() {
                create_ask_window(&app_handle)
            }

            // 通知前端：后端已就绪
            let _ = app_handle.emit("backend-ready", true);

            let total_setup_time = t_setup_start.elapsed().as_millis();
            info!(total_ms=%total_setup_time, "=== Application setup completed ===");
            
            Ok(())
        })
        .manage(AppState {
            selected_text: TokioMutex::new(String::new()),
            recording_shortcut: TokioMutex::new(false),
        })
        .manage(MessageTokenManager::new())
        .invoke_handler(tauri::generate_handler![
            ask_ai,
            tool_result_continue_ask_ai,
            regenerate_ai,
            regenerate_conversation_title,
            generate_artifact_metadata,
            cancel_ai,
            get_selected,
            open_config_window,
            open_chat_ui_window,
            open_plugin_window,
            open_plugin_store_window,
            open_artifact_preview_window,
            save_config,
            get_config,
            get_all_feature_config,
            save_feature_config,
            get_data_storage_config,
            save_data_storage_config,
            test_remote_storage_connection,
            open_data_folder,
            get_llm_providers,
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
            get_conversation_with_messages,
            create_conversation_with_messages,
            delete_conversation,
            fork_conversation,
            update_conversation,
            update_message_content,
            run_artifacts,
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
            check_bun_version,
            check_uv_version,
            install_bun,
            install_uv,
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
            get_assistant_mcp_servers_with_tools,
            update_assistant_mcp_config,
            update_assistant_mcp_tool_config,
            bulk_update_assistant_mcp_tools,
            update_assistant_model_config_value,
            create_mcp_tool_call,
            execute_mcp_tool_call,
            get_mcp_tool_call,
            get_mcp_tool_calls_by_conversation,
            list_aipp_builtin_templates,
            add_or_update_aipp_builtin_server,
            execute_aipp_builtin_tool,
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
            upload_local_data
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

    Ok(())
}

fn initialize_state_with_db(app_handle: &tauri::AppHandle) -> FeatureConfigState {
    let db = SystemDatabase::new(app_handle).expect("Failed to connect to system database");
    let configs = db.get_all_feature_config(app_handle).expect("Failed to load feature configs");
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

fn initialize_name_cache_state_with_dbs(app_handle: &tauri::AppHandle) -> NameCacheState {
    let assistant_db = AssistantDatabase::new(app_handle).expect("Failed to connect to assistant database");
    let llm_db = LLMDatabase::new(app_handle).expect("Failed to connect to llm database");
    
    let assistants = assistant_db.get_assistants().expect("Failed to load assistants");
    let mut assistant_names = HashMap::new();
    for assistant in assistants.clone().into_iter() {
        assistant_names.insert(assistant.id, assistant.name.clone());
    }

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

// 兼容接口：为了保持向后兼容，保留原有函数签名
#[allow(dead_code)]
fn initialize_state(app_handle: &tauri::AppHandle) -> FeatureConfigState {
    initialize_state_with_db(app_handle)
}

#[cfg(desktop)]
fn install_global_shortcut_plugin(app_handle: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt as _;
    // 仅安装插件，不注册处理逻辑（处理逻辑在按键释放时需要）
    let _ = app_handle.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|_app, _shortcut, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                if event.state() == ShortcutState::Released {
                    // 仅处理唤醒逻辑 / 选中文本逻辑（保持原先实现）
                    // 避免在 UI 线程阻塞，使用 try_lock 获取标志；若失败则跳过本次事件
                    if let Some(state) = _app.try_state::<AppState>() {
                        match state.recording_shortcut.try_lock() {
                            Ok(flag) => {
                                if *flag { return; }
                            }
                            Err(_) => {
                                return;
                            }
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        if let Ok(text) = get_selected_text() {
                            if !text.is_empty() {
                                let _ = _app.emit("get_selected_text_event", text.clone());
                                let app_handle = _app.clone();
                                let text_clone = text.clone();
                                tauri::async_runtime::spawn(async move {
                                    if let Some(app_state) = app_handle.try_state::<AppState>() {
                                        let mut guard = app_state.selected_text.lock().await;
                                        *guard = text_clone;
                                    }
                                });
                            }
                        }
                    }
                    handle_open_ask_window(_app);
                }
            })
            .build(),
    );
    info!("global_shortcut plugin installed (sync)");
}

#[cfg(desktop)]
async fn register_global_shortcuts_async(app_handle: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    info!("开始注册全局快捷键(异步) - computing shortcut string...");

    // 处理按键事件（插件已安装时才会触发）
    let _ = app_handle.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|_app, _shortcut, event| {
                // 仅在按键释放时触发
                if event.state() == ShortcutState::Released {
                    let t_event = std::time::Instant::now();
                    info!("Global shortcut released: start handling");
                    // 如果正在录入快捷键，忽略全局事件（使用 try_lock 避免阻塞 UI 线程）
                    if let Some(state) = _app.try_state::<AppState>() {
                        match state.recording_shortcut.try_lock() {
                            Ok(flag) => {
                                if *flag {
                                    debug!("正在录入快捷键，忽略全局快捷键事件");
                                    return;
                                }
                            }
                            Err(_) => {
                                // 锁被占用时跳过，避免阻塞
                                return;
                            }
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
                                    let text_clone = new_clip.clone();
                                    tauri::async_runtime::spawn(async move {
                                        let mut g = state.selected_text.lock().await;
                                        *g = text_clone;
                                    });
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
                                let app_handle = _app.clone();
                                let text_clone = text.clone();
                                tauri::async_runtime::spawn(async move {
                                    if let Some(state) = app_handle.try_state::<AppState>() {
                                        let mut guard = state.selected_text.lock().await;
                                        *guard = text_clone;
                                    }
                                });
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
        let config_feature_map = state.config_feature_map.lock().await;
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

    // 先清空旧注册，再注册新快捷键（带重试）
    for attempt in 1..=3 {
        if let Err(e) = app_handle.global_shortcut().unregister_all() {
            debug!(attempt=%attempt, error=%e, "卸载旧全局快捷键失败或未注册，继续");
        }
        match app_handle.global_shortcut().register(shortcut_str.as_str()) {
            Ok(_) => {
                if from_fallback {
                    info!(attempt=%attempt, "✓ 成功注册全局快捷键(回退): {}", shortcut_str);
                } else {
                    info!(attempt=%attempt, "✓ 成功注册全局快捷键: {}", shortcut_str);
                }
                break;
            }
            Err(e) => {
                warn!(attempt=%attempt, error=%e, shortcut=%shortcut_str, "注册全局快捷键失败");
                if attempt == 3 { warn!("放弃注册全局快捷键"); }
                else { std::thread::sleep(std::time::Duration::from_millis(150)); }
            }
        }
    }
}

// 已不再需要同步版本（避免在 runtime 中 block_in_place 嵌套）

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
