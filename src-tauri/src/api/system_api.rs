use std::cmp::Ord;
use std::collections::HashMap;
use tauri::{Manager, State, Emitter};

use crate::template_engine::{BangType, TemplateEngine};
use crate::AppState;
use crate::FeatureConfigState;

use crate::db::system_db::{FeatureConfig as FeatureConfigModel, SystemDatabase};
use sea_orm::DatabaseBackend;
use sea_orm::Schema;
use crate::entity::prelude::*; // re-exported Entity types

// 根据表名使用 SeaORM Entity 生成建表 SQL（如果有对应的 Entity）
fn generate_create_table_sql_from_entity(backend: DatabaseBackend, table_name: &str) -> Option<String> {
    let schema = Schema::new(backend);
    let stmt_opt = match table_name {
        // LLM
        "llm_provider" => Some(schema.create_table_from_entity(LlmProvider)),
        "llm_model" => Some(schema.create_table_from_entity(LlmModel)),
        "llm_provider_config" => Some(schema.create_table_from_entity(LlmProviderConfig)),
        // Conversation & message
        "conversation" => Some(schema.create_table_from_entity(Conversation)),
        "message" => Some(schema.create_table_from_entity(Message)),
        "message_attachment" => Some(schema.create_table_from_entity(MessageAttachment)),
        // System & feature config
        "system_config" => Some(schema.create_table_from_entity(SystemConfig)),
        "feature_config" => Some(schema.create_table_from_entity(FeatureConfig)),
        // MCP
        "mcp_server" => Some(schema.create_table_from_entity(McpServer)),
        "mcp_server_tool" => Some(schema.create_table_from_entity(McpServerTool)),
        "mcp_server_resource" => Some(schema.create_table_from_entity(McpServerResource)),
        "mcp_server_prompt" => Some(schema.create_table_from_entity(McpServerPrompt)),
        "mcp_tool_call" => Some(schema.create_table_from_entity(McpToolCall)),
        // Assistant
        "assistant" => Some(schema.create_table_from_entity(Assistant)),
        "assistant_model" => Some(schema.create_table_from_entity(AssistantModel)),
        "assistant_prompt" => Some(schema.create_table_from_entity(AssistantPrompt)),
        "assistant_model_config" => Some(schema.create_table_from_entity(AssistantModelConfig)),
        "assistant_prompt_param" => Some(schema.create_table_from_entity(AssistantPromptParam)),
        "assistant_mcp_config" => Some(schema.create_table_from_entity(AssistantMcpConfig)),
        "assistant_mcp_tool_config" => Some(schema.create_table_from_entity(AssistantMcpToolConfig)),
        // Plugins (注意表名大小写与 Entity 定义一致)
        "Plugins" => Some(schema.create_table_from_entity(Plugins)),
        "PluginStatus" => Some(schema.create_table_from_entity(PluginStatus)),
        "PluginConfigurations" => Some(schema.create_table_from_entity(PluginConfigurations)),
        "PluginData" => Some(schema.create_table_from_entity(PluginData)),
        // SubTask
        "sub_task_definition" => Some(schema.create_table_from_entity(SubTaskDefinition)),
        "sub_task_execution" => Some(schema.create_table_from_entity(SubTaskExecution)),
        // Artifacts
        "artifacts_collection" => Some(schema.create_table_from_entity(ArtifactsCollection)),
        _ => None,
    }?;
    let sql = match backend {
        DatabaseBackend::Postgres => stmt_opt.to_string(sea_orm::sea_query::PostgresQueryBuilder),
        DatabaseBackend::MySql => stmt_opt.to_string(sea_orm::sea_query::MysqlQueryBuilder),
        DatabaseBackend::Sqlite => stmt_opt.to_string(sea_orm::sea_query::SqliteQueryBuilder),
        _ => stmt_opt.to_string(sea_orm::sea_query::PostgresQueryBuilder),
    };
    Some(sql)
}

#[tauri::command]
pub async fn get_all_feature_config(
    state: State<'_, FeatureConfigState>,
) -> Result<Vec<FeatureConfigModel>, String> {
    let configs = state.configs.lock().await;
    Ok(configs.clone())
}

/// 获取数据存储配置（feature_code = "data_storage"），以扁平 map 返回
#[tauri::command]
pub async fn get_data_storage_config(
    state: State<'_, FeatureConfigState>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let config_feature_map = state.config_feature_map.lock().await;
    let mut result: std::collections::HashMap<String, String> = Default::default();
    if let Some(map) = config_feature_map.get("data_storage") {
        for (k, v) in map.iter() {
            result.insert(k.clone(), v.value.clone());
        }
    } else {
        // 默认本地存储
        result.insert("storage_mode".to_string(), "local".to_string());
    }
    Ok(result)
}

#[tauri::command]
pub async fn save_feature_config(
    app_handle: tauri::AppHandle,
    state: State<'_, FeatureConfigState>,
    feature_code: String,
    config: HashMap<String, String>,
) -> Result<(), String> {
    let db = SystemDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let _ = db.delete_feature_config_by_feature_code(feature_code.as_str());
    for (key, value) in config.iter() {
        db.add_feature_config(&FeatureConfigModel {
            id: None,
            feature_code: feature_code.clone(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        })
        .map_err(|e| e.to_string())?;
    }

    // 更新内存状态
    let mut configs = state.configs.lock().await;
    let mut config_feature_map = state.config_feature_map.lock().await;

    // 删除旧的配置
    configs.retain(|c| c.feature_code != feature_code);
    config_feature_map.remove(&feature_code);

    // 添加新的配置
    for (key, value) in config.iter() {
        let new_config = FeatureConfigModel {
            id: None,
            feature_code: feature_code.clone(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        };
        configs.push(new_config.clone());
        config_feature_map
            .entry(feature_code.clone())
            .or_insert(HashMap::new())
            .insert(key.clone(), new_config);
    }
    // 如果更新的是快捷键配置，则尝试重新注册全局快捷键（异步，避免阻塞 runtime）
    #[cfg(desktop)]
    if feature_code == "shortcuts" {
        let app = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            crate::reconfigure_global_shortcuts_async(&app).await;
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn open_data_folder(app: tauri::AppHandle) -> Result<(), String> {
    let app_dir = app.path().app_data_dir().unwrap();
    let db_path = app_dir.join("db");
    if let Err(e) = open::that(db_path) {
        return Err(format!("无法打开数据文件夹: {}", e));
    }
    Ok(())
}

/// Save data storage configuration into feature_config under feature_code = "data_storage"
#[tauri::command]
pub async fn save_data_storage_config(
    app_handle: tauri::AppHandle,
    state: State<'_, FeatureConfigState>,
    storage_mode: String,                 // "local" | "remote"
    remote_type: Option<String>,          // Some("supabase"|"postgresql"|"mysql") when remote
    payload: std::collections::HashMap<String, String>,
)
-> Result<(), String> {
    let db = SystemDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // Build a flattened map to store
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("storage_mode".to_string(), storage_mode.clone());
    if let Some(rt) = remote_type.clone() { config.insert("remote_type".to_string(), rt); }
    for (k,v) in payload.iter() { config.insert(k.clone(), v.clone()); }

    // Persist by replacing feature_code = data_storage
    let feature_code = "data_storage".to_string();
    let _ = db.delete_feature_config_by_feature_code(&feature_code);
    for (key, value) in config.iter() {
        db.add_feature_config(&FeatureConfigModel {
            id: None,
            feature_code: feature_code.clone(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        }).map_err(|e| e.to_string())?;
    }

    // Update in-memory state
    let mut configs = state.configs.lock().await;
    let mut config_feature_map = state.config_feature_map.lock().await;
    configs.retain(|c| c.feature_code != feature_code);
    config_feature_map.remove("data_storage");
    for (key, value) in config.iter() {
        let new_config = FeatureConfigModel {
            id: None,
            feature_code: "data_storage".to_string(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        };
        configs.push(new_config.clone());
        config_feature_map
            .entry("data_storage".to_string())
            .or_insert(Default::default())
            .insert(key.clone(), new_config);
    }

    Ok(())
}

/// Test connectivity to a remote storage based on type and payload
#[tauri::command]
pub async fn test_remote_storage_connection(
    remote_type: String,
    payload: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    match remote_type.as_str() {
        "supabase" => {
            let url = payload.get("supabase_url").cloned().ok_or("缺少 supabase_url")?;
            let key = payload.get("supabase_key").cloned().ok_or("缺少 supabase_key")?;
            // Try a lightweight request to the REST endpoint root; any non-5xx status counts as reachable
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
                .map_err(|e| e.to_string())?;
            let rest_url = format!("{}/rest/v1/", url.trim_end_matches('/'));
            let resp = client
                .get(&rest_url)
                .header("apikey", key.clone())
                .header("Authorization", format!("Bearer {}", key))
                .send()
                .await
                .map_err(|e| format!("请求失败: {}", e))?;
            if resp.status().is_server_error() {
                return Err(format!("Supabase 服务返回错误状态: {}", resp.status()));
            }
            Ok(())
        }
        "postgresql" => {
            let host = payload.get("pg_host").cloned().ok_or("缺少 pg_host")?;
            let port = payload.get("pg_port").cloned().unwrap_or_else(|| "5432".to_string());
            let db = payload.get("pg_database").cloned().ok_or("缺少 pg_database")?;
            let user = payload.get("pg_username").cloned().ok_or("缺少 pg_username")?;
            let pass = payload.get("pg_password").cloned().ok_or("缺少 pg_password")?;
            let url = format!("postgres://{}:{}@{}:{}/{}", urlencoding::encode(&user), urlencoding::encode(&pass), host, port, db);
            let conn = sea_orm::Database::connect(&url).await.map_err(|e| format!("连接失败: {}", e))?;
            // Simple ping by acquiring connection
            conn.ping().await.map_err(|e| format!("Ping 失败: {}", e))?;
            Ok(())
        }
        "mysql" => {
            let host = payload.get("mysql_host").cloned().ok_or("缺少 mysql_host")?;
            let port = payload.get("mysql_port").cloned().unwrap_or_else(|| "3306".to_string());
            let db = payload.get("mysql_database").cloned().ok_or("缺少 mysql_database")?;
            let user = payload.get("mysql_username").cloned().ok_or("缺少 mysql_username")?;
            let pass = payload.get("mysql_password").cloned().ok_or("缺少 mysql_password")?;
            let url = format!("mysql://{}:{}@{}:{}/{}", urlencoding::encode(&user), urlencoding::encode(&pass), host, port, db);
            let conn = sea_orm::Database::connect(&url).await.map_err(|e| format!("连接失败: {}", e))?;
            conn.ping().await.map_err(|e| format!("Ping 失败: {}", e))?;
            Ok(())
        }
        _ => Err("不支持的远程类型".to_string()),
    }
}

/// Upload local data to remote storage
#[tauri::command]
pub async fn upload_local_data(
    app: tauri::AppHandle,
    remote_type: String,
    payload: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    use sea_orm::Database;
    
    // Step 1: Test remote connection first
    tracing::info!(?remote_type, "Testing remote connection before upload");
    test_remote_storage_connection(remote_type.clone(), payload.clone()).await?;
    
    // Emit progress
    let _ = app.emit("upload-progress", serde_json::json!({
        "stage": "connection_verified",
        "message": "远程连接验证成功"
    }));
    
    // Step 2: Get local database paths
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let db_dir = app_dir.join("db");
    if !db_dir.exists() {
        return Err("本地数据目录不存在".to_string());
    }
    
    let local_dbs = vec![
        ("conversation.db", vec!["conversation", "message", "message_attachment"]),
        ("assistant.db", vec!["assistant", "assistant_model", "assistant_prompt", "assistant_model_config", "assistant_prompt_param", "assistant_mcp_config", "assistant_mcp_tool_config"]),
        ("llm.db", vec!["llm_provider", "llm_model", "llm_provider_config"]),
        ("mcp.db", vec!["mcp_server", "mcp_server_tool", "mcp_server_resource", "mcp_server_prompt", "mcp_tool_call"]),
        ("system.db", vec!["system_config", "feature_config"]),
        ("sub_task.db", vec!["sub_task_definition", "sub_task_execution"]),
        ("plugins.db", vec!["Plugins", "PluginStatus", "PluginConfigurations", "PluginData"]),
        ("artifacts.db", vec!["artifacts_collection"]),
    ];
    
    // Step 3: Connect to remote database
    let remote_url = build_remote_url(&remote_type, &payload)?;
    tracing::info!(remote_type = ?remote_type, "Connecting to remote database");
    
    let remote_db = Database::connect(&remote_url).await.map_err(|e| {
        let error_msg = format!("连接远程数据库失败: {}", e);
        tracing::error!(?e, "Database connection failed");
        
        // 提供更友好的错误提示
        if error_msg.contains("11001") || error_msg.contains("不知道这样的主机") {
            if let Some(host) = payload.get("supabase_db_host") {
                return format!(
                    "无法连接到数据库主机: {}\n\n请检查：\n1. 主机地址是否正确（应该类似: db.xxxxx.supabase.co）\n2. 网络连接是否正常\n3. 是否从 Supabase 项目设置中正确复制了连接信息",
                    host
                );
            }
        }
        error_msg
    })?;
    
    let _ = app.emit("upload-progress", serde_json::json!({
        "stage": "remote_connected",
        "message": "已连接到远程数据库"
    }));
    
    // Step 4: Process each database
    for (db_name, tables) in local_dbs.iter() {
        let local_db_path = db_dir.join(db_name);
        if !local_db_path.exists() {
            tracing::warn!(?db_name, "Local database not found, skipping");
            continue;
        }
        
        let _ = app.emit("upload-progress", serde_json::json!({
            "stage": "processing_db",
            "message": format!("正在处理 {}", db_name),
            "db_name": db_name
        }));
        
        // Process each table
        for table_name in tables {
            if let Err(e) = migrate_table_async(&local_db_path, &remote_db, &remote_type, table_name, &app).await {
                tracing::error!(?table_name, ?e, "Failed to migrate table");
                return Err(format!("迁移表 {} 失败: {}", table_name, e));
            }
        }
    }
    
    let _ = app.emit("upload-progress", serde_json::json!({
        "stage": "completed",
        "message": "数据上传完成"
    }));
    
    tracing::info!("All local data uploaded successfully");
    Ok(())
}

/// Build remote database connection URL
fn build_remote_url(remote_type: &str, payload: &std::collections::HashMap<String, String>) -> Result<String, String> {
    match remote_type {
        "postgresql" => {
            let host = payload.get("pg_host").ok_or("缺少 pg_host")?;
            let default_port = "5432".to_string();
            let port = payload.get("pg_port").unwrap_or(&default_port);
            let db = payload.get("pg_database").ok_or("缺少 pg_database")?;
            let user = payload.get("pg_username").ok_or("缺少 pg_username")?;
            let pass = payload.get("pg_password").ok_or("缺少 pg_password")?;
            Ok(format!("postgres://{}:{}@{}:{}/{}", 
                urlencoding::encode(user), urlencoding::encode(pass), host, port, db))
        }
        "mysql" => {
            let host = payload.get("mysql_host").ok_or("缺少 mysql_host")?;
            let default_port = "3306".to_string();
            let port = payload.get("mysql_port").unwrap_or(&default_port);
            let db = payload.get("mysql_database").ok_or("缺少 mysql_database")?;
            let user = payload.get("mysql_username").ok_or("缺少 mysql_username")?;
            let pass = payload.get("mysql_password").ok_or("缺少 mysql_password")?;
            Ok(format!("mysql://{}:{}@{}:{}/{}", 
                urlencoding::encode(user), urlencoding::encode(pass), host, port, db))
        }
        "supabase" => {
            // Supabase uses PostgreSQL connection string
            let _url = payload.get("supabase_url").ok_or("缺少 supabase_url")?;
            let password = payload.get("supabase_db_password")
                .ok_or("缺少数据库密码。请在 Supabase 项目设置 → Database → Connection string 中查找")?;
            let host = payload.get("supabase_db_host")
                .ok_or("缺少数据库主机地址。格式如: db.xxxxx.supabase.co")?;
            
            if host.is_empty() {
                return Err("数据库主机地址不能为空".to_string());
            }
            
            let default_port = "5432".to_string();
            let default_db = "postgres".to_string();
            let default_user = "postgres".to_string();
            let port = payload.get("supabase_db_port").unwrap_or(&default_port);
            let db = payload.get("supabase_db_name").unwrap_or(&default_db);
            let user = payload.get("supabase_db_user").unwrap_or(&default_user);
            
            let conn_str = format!("postgres://{}:{}@{}:{}/{}", 
                urlencoding::encode(user), urlencoding::encode(password), host, port, db);
            
            tracing::info!(host = ?host, port = ?port, db = ?db, user = ?user, "Supabase connection info");
            Ok(conn_str)
        }
        _ => Err("不支持的远程类型".to_string())
    }
}

/// Migrate a single table from local SQLite to remote database (async version)
async fn migrate_table_async(
    local_db_path: &std::path::PathBuf,
    remote_db: &sea_orm::DatabaseConnection,
    remote_type: &str,
    table_name: &str,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    use rusqlite::Connection;
    use sea_orm::ConnectionTrait;
    use sea_orm::DatabaseBackend;
    use sea_orm::sea_query::{Alias, ColumnDef, Index, Table, TableCreateStatement};
    
    tracing::info!(?table_name, "Starting table migration");
    
    let _ = app.emit("upload-progress", serde_json::json!({
        "stage": "migrating_table",
        "message": format!("正在迁移表 {}", table_name),
        "table": table_name
    }));
    
    // Read data from SQLite in spawn_blocking
    let local_db_path_clone = local_db_path.clone();
    let table_name_string = table_name.to_string();
    
    let (columns, column_types, rows_data, col_meta) = tokio::task::spawn_blocking(move || {
        let local_conn = Connection::open(&local_db_path_clone)
            .map_err(|e| format!("打开本地数据库失败: {}", e))?;
        
        // Step 1: Get column names and types
        let cols_with_types = get_table_columns_with_types(&local_conn, &table_name_string)?;
        let columns: Vec<String> = cols_with_types.iter().map(|(n, _)| n.clone()).collect();
        let column_types: Vec<String> = cols_with_types.iter().map(|(_, t)| t.clone()).collect();

        // Step 2: Get full column metadata for schema builder
        let col_meta = get_table_columns_meta(&local_conn, &table_name_string)?;
        
        // Step 3: Read all data
        let mut rows_data: Vec<Vec<serde_json::Value>> = Vec::new();
        if !columns.is_empty() {
            let query = format!("SELECT * FROM {}", table_name_string);
            let mut stmt = local_conn.prepare(&query)
                .map_err(|e| format!("准备查询失败: {}", e))?;
            
            let column_count = stmt.column_count();
            let rows = stmt.query_map([], |row| {
                let mut row_values = Vec::new();
                for i in 0..column_count {
                    let value: rusqlite::types::Value = row.get(i)?;
                    let json_value = sqlite_value_to_json(value);
                    row_values.push(json_value);
                }
                Ok(row_values)
            }).map_err(|e| format!("查询数据失败: {}", e))?;
            
            for row_result in rows {
                rows_data.push(row_result.map_err(|e| e.to_string())?);
            }
        }
        
        Ok::<_, String>((columns, column_types, rows_data, col_meta))
    }).await.map_err(|e| format!("读取本地数据失败: {}", e))??;
    
    tracing::info!(?table_name, row_count = rows_data.len(), "Read data from local table");
    
    // Drop if exists, then create table using dialect-aware schema builder
    let drop_sql = match remote_type {
        "postgresql" | "supabase" => format!("DROP TABLE IF EXISTS {} CASCADE", table_name),
        _ => format!("DROP TABLE IF EXISTS {}", table_name),
    };
    remote_db.execute_unprepared(&drop_sql).await.map_err(|e| format!("删除远程表失败: {}", e))?;

    // Prefer to build create table statement from SeaORM Entity definitions when available
    let backend = remote_db.get_database_backend();
    if let Some(sql) = generate_create_table_sql_from_entity(backend, table_name) {
        remote_db
            .execute_unprepared(&sql)
            .await
            .map_err(|e| format!("创建远程表失败: {}", e))?;
    } else {
        // Fallback: construct CREATE TABLE from SQLite PRAGMA metadata
        let mut create: TableCreateStatement = Table::create()
            .table(Alias::new(table_name))
            .to_owned();

        // Determine if composite primary key
        let pk_columns: Vec<String> = col_meta.iter().filter(|c| c.is_pk).map(|c| c.name.clone()).collect();

        for c in &col_meta {
            let mut col = ColumnDef::new(Alias::new(&c.name));
            let ty = map_sqlite_type_to_seaquery(&c.decl_type);
            match ty.as_str() {
                "big_integer" => { col.big_integer(); },
                "integer" => { col.integer(); },
                "boolean" => { col.boolean(); },
                "string" => { col.string(); },
                "text" => { col.text(); },
                "double" => { col.double(); },
                "float" => { col.float(); },
                "decimal" => { col.decimal_len(65, 30); },
                "binary" => { col.binary(); },
                "timestamp" => { col.timestamp(); },
                _ => { col.text(); },
            };

            if c.not_null { col.not_null(); }

            if let Some(default_raw) = &c.default {
                if !default_raw.is_empty() {
                    // Normalize default value across dialects
                    let def_expr = map_default_expr(&ty, default_raw);
                    col.default(def_expr);
                }
            }

            // For single-column PK mark here; for composite we'll add table-level PK below
            if pk_columns.len() == 1 && c.is_pk {
                // try to make it auto increment for integer types
                if ty == "big_integer" || ty == "integer" {
                    col.auto_increment();
                }
                col.primary_key();
            }

            create.col(col);
        }

        if pk_columns.len() > 1 {
            let mut idx = Index::create();
            for p in pk_columns.iter() { idx.col(Alias::new(p)); }
            create.primary_key(&mut idx);
        }

        // Render SQL with the proper query builder for the active backend
        let sql = match backend {
            DatabaseBackend::Postgres => create.to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => create.to_string(sea_orm::sea_query::MysqlQueryBuilder),
            DatabaseBackend::Sqlite => create.to_string(sea_orm::sea_query::SqliteQueryBuilder),
            _ => create.to_string(sea_orm::sea_query::PostgresQueryBuilder), // fallback
        };
        remote_db.execute_unprepared(&sql).await.map_err(|e| format!("创建远程表失败: {}", e))?;
    }
    
    // Insert data in batches
    if !rows_data.is_empty() {
        let batch_size = 50;
        for (batch_idx, chunk) in rows_data.chunks(batch_size).enumerate() {
            // Build a batch INSERT statement
            let values_placeholders: Vec<String> = chunk.iter()
                .map(|row| {
                    let row_placeholders: Vec<String> = row.iter()
                        .enumerate()
                        .map(|(col_idx, val)| {
                            // Determine if this column is boolean
                            let ty = column_types.get(col_idx).map(|s| s.to_lowercase()).unwrap_or_default();
                            let is_bool_col = ty.contains("bool");

                            if is_bool_col {
                                // Use SQL boolean literals
                                match val {
                                    serde_json::Value::Null => "NULL".to_string(),
                                    serde_json::Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                                    serde_json::Value::Number(n) => {
                                        if n.as_i64() == Some(1) { "TRUE".to_string() } else { "FALSE".to_string() }
                                    }
                                    serde_json::Value::String(s) => {
                                        let ls = s.to_lowercase();
                                        if ls == "1" || ls == "true" || ls == "t" { "TRUE".to_string() } else { "FALSE".to_string() }
                                    }
                                    _ => "FALSE".to_string(),
                                }
                            } else {
                                match val {
                                    serde_json::Value::Null => "NULL".to_string(),
                                    serde_json::Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                                    serde_json::Value::Number(n) => n.to_string(),
                                    serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                    _ => format!("'{}'", val.to_string().replace("'", "''")),
                                }
                            }
                        })
                        .collect();
                    format!("({})", row_placeholders.join(", "))
                })
                .collect();
            
            let insert_sql = format!(
                "INSERT INTO {} ({}) VALUES {}",
                table_name,
                columns.join(", "),
                values_placeholders.join(", ")
            );
            
            remote_db.execute_unprepared(&insert_sql).await
                .map_err(|e| format!("插入数据失败 (batch {}): {}", batch_idx, e))?;
            
            tracing::debug!(?table_name, batch_idx, rows = chunk.len(), "Inserted batch");
        }
    }
    
    tracing::info!(?table_name, total_rows = rows_data.len(), "Table migration completed");
    Ok(())
}

/// Get table schema from SQLite
fn get_table_schema(conn: &rusqlite::Connection, table_name: &str) -> Result<String, String> {
    let query = format!("SELECT sql FROM sqlite_master WHERE type='table' AND name=?");
    let sql: String = conn.query_row(&query, [table_name], |row| row.get(0))
        .map_err(|e| format!("获取表结构失败: {}", e))?;
    Ok(sql)
}

/// Get table column names
fn get_table_columns(conn: &rusqlite::Connection, table_name: &str) -> Result<Vec<String>, String> {
    let query = format!("PRAGMA table_info({})", table_name);
    let mut stmt = conn.prepare(&query)
        .map_err(|e| format!("获取表列信息失败: {}", e))?;
    
    let columns = stmt.query_map([], |row| {
        let name: String = row.get(1)?;
        Ok(name)
    }).map_err(|e| format!("查询列信息失败: {}", e))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;
    
    Ok(columns)
}

/// Get table column names with types (needed to map boolean correctly)
fn get_table_columns_with_types(conn: &rusqlite::Connection, table_name: &str) -> Result<Vec<(String, String)>, String> {
    let query = format!("PRAGMA table_info({})", table_name);
    let mut stmt = conn.prepare(&query)
        .map_err(|e| format!("获取表列信息失败: {}", e))?;

    let columns = stmt.query_map([], |row| {
        let name: String = row.get(1)?; // name
        let data_type: String = row.get(2)?; // type
        Ok((name, data_type))
    }).map_err(|e| format!("查询列信息失败: {}", e))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    Ok(columns)
}

#[derive(Debug, Clone)]
struct ColumnMeta {
    name: String,
    decl_type: String,
    not_null: bool,
    default: Option<String>,
    is_pk: bool,
}

/// Get detailed column metadata from SQLite PRAGMA table_info
fn get_table_columns_meta(conn: &rusqlite::Connection, table_name: &str) -> Result<Vec<ColumnMeta>, String> {
    let query = format!("PRAGMA table_info({})", table_name);
    let mut stmt = conn.prepare(&query)
        .map_err(|e| format!("获取表列信息失败: {}", e))?;

    let columns = stmt.query_map([], |row| {
        // PRAGMA table_info columns: cid, name, type, notnull, dflt_value, pk
        let name: String = row.get(1)?;
        let decl_type: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
        let not_null_int: i32 = row.get(3)?;
        let dflt: Option<String> = row.get(4)?;
        let pk_int: i32 = row.get(5)?;
        Ok(ColumnMeta {
            name,
            decl_type,
            not_null: not_null_int != 0,
            default: dflt,
            is_pk: pk_int != 0,
        })
    }).map_err(|e| format!("查询列信息失败: {}", e))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    Ok(columns)
}

/// Map SQLite declared type to sea_query logical type marker
fn map_sqlite_type_to_seaquery(decl: &str) -> String {
    let s = decl.to_lowercase();
    if s.contains("int") { return "big_integer".to_string(); }
    if s.contains("bool") { return "boolean".to_string(); }
    if s.contains("char") || s.contains("clob") { return "string".to_string(); }
    if s.contains("text") { return "text".to_string(); }
    if s.contains("real") || s.contains("floa") || s.contains("doub") { return "double".to_string(); }
    if s.contains("dec") || s.contains("num") { return "decimal".to_string(); }
    if s.contains("blob") { return "binary".to_string(); }
    if s.contains("date") || s.contains("time") { return "timestamp".to_string(); }
    // fallback
    "text".to_string()
}

/// Convert SQLite default value string into SeaQuery expression by target logical type
fn map_default_expr(logical_ty: &str, raw: &str) -> sea_orm::sea_query::SimpleExpr {
    use sea_orm::sea_query::{Expr, Value};
    let trimmed = raw.trim();
    // strip surrounding parentheses
    let unparen = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        &trimmed[1..trimmed.len()-1]
    } else { trimmed };
    let unparen = unparen.trim();

    // CURRENT_TIMESTAMP and similar functions
    let upper = unparen.to_uppercase();
    if upper == "CURRENT_TIMESTAMP" {
        return Expr::cust("CURRENT_TIMESTAMP");
    }

    match logical_ty {
        "boolean" => {
            let v = match unparen.to_ascii_lowercase().as_str() {
                "1" | "true" | "t" => true,
                _ => false,
            };
            Expr::val(Value::Bool(Some(v)))
        }
        "big_integer" | "integer" => {
            if let Ok(i) = unparen.parse::<i64>() {
                Expr::val(Value::BigInt(Some(i)))
            } else { Expr::cust(format!("{}", unparen)) }
        }
        "double" | "float" => {
            if let Ok(f) = unparen.parse::<f64>() {
                Expr::val(Value::Double(Some(f)))
            } else { Expr::cust(format!("{}", unparen)) }
        }
        "decimal" => {
            // store as string literal
            Expr::val(Value::String(Some(unparen.to_string())))
        }
        _ => {
            // string literal: strip quotes if present
            let unquoted = unparen
                .strip_prefix('\'')
                .and_then(|x| x.strip_suffix('\''))
                .unwrap_or(unparen);
            Expr::val(Value::String(Some(unquoted.to_string())))
        }
    }
}

/// Convert SQLite value to JSON value
fn sqlite_value_to_json(value: rusqlite::types::Value) -> serde_json::Value {
    use base64::{Engine as _, engine::general_purpose};
    match value {
        rusqlite::types::Value::Null => serde_json::Value::Null,
        rusqlite::types::Value::Integer(i) => serde_json::json!(i),
        rusqlite::types::Value::Real(f) => serde_json::json!(f),
        rusqlite::types::Value::Text(s) => serde_json::json!(s),
        rusqlite::types::Value::Blob(b) => serde_json::json!(general_purpose::STANDARD.encode(b)),
    }
}

/// Convert SQLite schema to target database schema
fn convert_schema(sqlite_schema: &str, target_type: &str) -> Result<String, String> {
    let mut schema = sqlite_schema.to_string();
    
    // Remove IF NOT EXISTS for clean recreation
    schema = schema.replace("IF NOT EXISTS ", "");
    
    match target_type {
        "postgresql" | "supabase" => {
            use regex::Regex;
            // Convert SQLite types to PostgreSQL types
            // 1) AUTOINCREMENT 专用（大小写多种可能）
            schema = schema.replace("INTEGER PRIMARY KEY AUTOINCREMENT", "SERIAL PRIMARY KEY");
            schema = schema.replace("integer primary key autoincrement", "SERIAL PRIMARY KEY");
            schema = schema.replace("Integer Primary Key Autoincrement", "SERIAL PRIMARY KEY");
            // 保险起见，移除任何残余的 autoincrement 关键字
            schema = schema.replace(" AUTOINCREMENT", "");
            schema = schema.replace(" autoincrement", "");

            // 2) 纯 INTEGER PRIMARY KEY（无 AUTOINCREMENT）转 Postgres 自增
            // 注意：必须在移除 autoincrement 之后执行
            schema = schema.replace("INTEGER PRIMARY KEY", "BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY");
            schema = schema.replace("integer primary key", "BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY");

            schema = schema.replace("DATETIME DEFAULT CURRENT_TIMESTAMP", "TIMESTAMP DEFAULT CURRENT_TIMESTAMP");
            schema = schema.replace("DATETIME", "TIMESTAMP");
            schema = schema.replace("BOOLEAN", "BOOLEAN");
            schema = schema.replace("UNIQUE(", "UNIQUE (");
            // Fix boolean default literals (SQLite often uses 0/1)，使用正则覆盖更多变体
            // BOOLEAN [NOT NULL] DEFAULT 0|1 或 DEFAULT(0|1)（大小写、空格可变）
            let re_bool_nn_0 = Regex::new(r"(?i)BOOLEAN\s+NOT\s+NULL\s+DEFAULT\s*\(?\s*0\s*\)?").unwrap();
            let re_bool_nn_1 = Regex::new(r"(?i)BOOLEAN\s+NOT\s+NULL\s+DEFAULT\s*\(?\s*1\s*\)?").unwrap();
            let re_bool_0 = Regex::new(r"(?i)BOOLEAN\s+DEFAULT\s*\(?\s*0\s*\)?").unwrap();
            let re_bool_1 = Regex::new(r"(?i)BOOLEAN\s+DEFAULT\s*\(?\s*1\s*\)?").unwrap();
            schema = re_bool_nn_0.replace_all(&schema, "BOOLEAN NOT NULL DEFAULT FALSE").to_string();
            schema = re_bool_nn_1.replace_all(&schema, "BOOLEAN NOT NULL DEFAULT TRUE").to_string();
            schema = re_bool_0.replace_all(&schema, "BOOLEAN DEFAULT FALSE").to_string();
            schema = re_bool_1.replace_all(&schema, "BOOLEAN DEFAULT TRUE").to_string();
            Ok(schema)
        }
        "mysql" => {
            // Convert SQLite types to MySQL types
            schema = schema.replace("INTEGER PRIMARY KEY AUTOINCREMENT", "INT AUTO_INCREMENT PRIMARY KEY");
            schema = schema.replace("integer primary key autoincrement", "INT AUTO_INCREMENT PRIMARY KEY");
            schema = schema.replace(" AUTOINCREMENT", "");
            schema = schema.replace(" autoincrement", "");
            schema = schema.replace("INTEGER PRIMARY KEY", "BIGINT AUTO_INCREMENT PRIMARY KEY");
            schema = schema.replace("integer primary key", "BIGINT AUTO_INCREMENT PRIMARY KEY");
            schema = schema.replace("DATETIME DEFAULT CURRENT_TIMESTAMP", "DATETIME DEFAULT CURRENT_TIMESTAMP");
            schema = schema.replace("BOOLEAN", "TINYINT(1)");
            schema = schema.replace("TEXT", "TEXT");
            Ok(schema)
        }
        _ => Err("不支持的数据库类型".to_string())
    }
}

#[tauri::command]
pub async fn get_bang_list() -> Result<Vec<(String, String, String, BangType)>, String> {
    let engine = TemplateEngine::new();
    let mut list = vec![];
    for bang in engine.get_commands().iter() {
        list.push((
            bang.name.clone(),
            bang.complete.clone(),
            bang.description.clone(),
            bang.bang_type.clone(),
        ));
    }
    list.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(list)
}
#[tauri::command]
pub async fn get_selected_text_api(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let selected_text = state.selected_text.lock().await;
    Ok(selected_text.clone())
}

#[tauri::command]
pub async fn set_shortcut_recording(state: tauri::State<'_, AppState>, active: bool) -> Result<(), String> {
    let mut flag = state.recording_shortcut.lock().await;
    *flag = active;
    Ok(())
}

#[tauri::command]
pub async fn suspend_global_shortcut(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        if let Err(e) = app.global_shortcut().unregister_all() {
            return Err(format!("无法暂停全局快捷键: {}", e));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn resume_global_shortcut(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(desktop)]
    {
        crate::reconfigure_global_shortcuts_async(&app).await;
    }
    Ok(())
}
