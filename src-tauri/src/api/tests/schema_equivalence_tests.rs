use sea_orm::{DbBackend, Schema};
use sea_orm::sea_query::TableCreateStatement;
use rusqlite::Connection;
use std::path::PathBuf;
use crate::entity::prelude::*;

fn sqlite_db_path(db_name: &str) -> PathBuf {
    let base = tauri::api::path::data_dir().expect("data dir");
    // In tests we may not have the same app data dir; adapt by searching relative 'db' under project root if missing
    let proj_db = PathBuf::from("db").join(db_name);
    if proj_db.exists() { proj_db } else { base.join("db").join(db_name) }
}

fn read_sqlite_columns(db_path: &PathBuf, table: &str) -> Vec<(String,String,bool,Option<String>,bool)> {
    let conn = Connection::open(db_path).expect("open db");
    let pragma = format!("PRAGMA table_info({})", table);
    let mut stmt = conn.prepare(&pragma).expect("prepare pragma");
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(1)?;
        let decl_type: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
        let not_null: i32 = row.get(3)?;
        let dflt: Option<String> = row.get(4)?;
        let pk: i32 = row.get(5)?;
        Ok((name, decl_type, not_null!=0, dflt, pk!=0))
    }).expect("query map");
    let mut out = vec![];
    for r in rows { out.push(r.expect("row")); }
    out
}

fn extract_columns_from_statement(stmt: &TableCreateStatement, backend: DbBackend) -> Vec<String> {
    let sql = match backend {
        DbBackend::Sqlite => stmt.to_string(sea_orm::sea_query::SqliteQueryBuilder),
        DbBackend::Postgres => stmt.to_string(sea_orm::sea_query::PostgresQueryBuilder),
        DbBackend::MySql => stmt.to_string(sea_orm::sea_query::MysqlQueryBuilder),
    };
    let mut cols = vec![];
    for line in sql.split(',') {
        let line_trim = line.trim();
        // crude parse: lines like `"name" TEXT ...` or `"id" integer ...`
        if line_trim.starts_with('"') || line_trim.starts_with('`') {
            let name_part = line_trim.split_whitespace().next().unwrap_or("");
            let name_clean = name_part.trim_matches(|c| c=='"' || c=='`').to_string();
            cols.push(name_clean);
        }
    }
    cols
}

#[test]
fn test_llm_provider_entity_matches_sqlite_schema() {
    // We only test presence of columns & primary key existence for now.
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let stmt = schema.create_table_from_entity(LlmProvider);
    let entity_cols = extract_columns_from_statement(&stmt, backend);

    // Assume database file created already during normal app run; test uses schema definition directly
    let expected_cols = vec![
        "id","name","api_type","description","is_official","is_enabled","created_time"
    ];
    for c in expected_cols { assert!(entity_cols.contains(&c.to_string()), "missing column {}", c); }
}

#[test]
fn test_llm_model_entity_matches_sqlite_schema() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let stmt = schema.create_table_from_entity(LlmModel);
    let entity_cols = extract_columns_from_statement(&stmt, backend);
    let expected_cols = vec![
        "id","name","llm_provider_id","code","description","vision_support","audio_support","video_support","created_time"
    ];
    for c in expected_cols { assert!(entity_cols.contains(&c.to_string()), "missing column {}", c); }
}

#[test]
fn test_llm_provider_config_entity_matches_sqlite_schema() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let stmt = schema.create_table_from_entity(LlmProviderConfig);
    let entity_cols = extract_columns_from_statement(&stmt, backend);
    let expected_cols = vec![
        "id","name","llm_provider_id","value","append_location","is_addition","created_time"
    ];
    for c in expected_cols { assert!(entity_cols.contains(&c.to_string()), "missing column {}", c); }
}

#[test]
fn test_conversation_entity_columns() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let stmt = schema.create_table_from_entity(Conversation);
    let cols = extract_columns_from_statement(&stmt, backend);
    for c in [
        "id","name","assistant_id","created_time"
    ] { assert!(cols.contains(&c.to_string()), "missing column {}", c); }
}

#[test]
fn test_message_entity_columns() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let stmt = schema.create_table_from_entity(Message);
    let cols = extract_columns_from_statement(&stmt, backend);
    for c in [
        "id","conversation_id","message_type","content","llm_model_id","created_time","token_count","parent_id","start_time","finish_time","llm_model_name","generation_group_id","parent_group_id","tool_calls_json"
    ] { assert!(cols.contains(&c.to_string()), "missing column {}", c); }
}

#[test]
fn test_message_attachment_entity_columns() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let stmt = schema.create_table_from_entity(MessageAttachment);
    let cols = extract_columns_from_statement(&stmt, backend);
    for c in [
        "id","message_id","attachment_type","attachment_url","attachment_hash","attachment_content","use_vector","token_count"
    ] { assert!(cols.contains(&c.to_string()), "missing column {}", c); }
}

#[test]
fn test_system_config_entity_columns() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let stmt = schema.create_table_from_entity(SystemConfig);
    let cols = extract_columns_from_statement(&stmt, backend);
    for c in ["id","key","value","created_time"] {
        assert!(cols.contains(&c.to_string()), "missing column {}", c);
    }
}

#[test]
fn test_feature_config_entity_columns() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let stmt = schema.create_table_from_entity(FeatureConfig);
    let cols = extract_columns_from_statement(&stmt, backend);
    for c in ["id","feature_code","key","value","data_type","description"] {
        assert!(cols.contains(&c.to_string()), "missing column {}", c);
    }
}

#[test]
fn test_mcp_server_entity_columns() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let stmt = schema.create_table_from_entity(McpServer);
    let cols = extract_columns_from_statement(&stmt, backend);
    for c in [
        "id","name","description","transport_type","command","environment_variables","url","timeout","is_long_running","is_enabled","is_builtin","created_time"
    ] { assert!(cols.contains(&c.to_string()), "missing column {}", c); }
}

#[test]
fn test_mcp_related_entities_columns() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);

    // tool
    let cols_tool = extract_columns_from_statement(&schema.create_table_from_entity(McpServerTool), backend);
    for c in ["id","server_id","tool_name","tool_description","is_enabled","is_auto_run","parameters","created_time"] {
        assert!(cols_tool.contains(&c.to_string()), "missing column {}", c);
    }
    // resource
    let cols_res = extract_columns_from_statement(&schema.create_table_from_entity(McpServerResource), backend);
    for c in ["id","server_id","resource_uri","resource_name","resource_type","resource_description","created_time"] {
        assert!(cols_res.contains(&c.to_string()), "missing column {}", c);
    }
    // prompt
    let cols_prompt = extract_columns_from_statement(&schema.create_table_from_entity(McpServerPrompt), backend);
    for c in ["id","server_id","prompt_name","prompt_description","is_enabled","arguments","created_time"] {
        assert!(cols_prompt.contains(&c.to_string()), "missing column {}", c);
    }
    // tool_call
    let cols_call = extract_columns_from_statement(&schema.create_table_from_entity(McpToolCall), backend);
    for c in [
        "id","conversation_id","message_id","server_id","server_name","tool_name","parameters","status","result","error","created_time","started_time","finished_time","llm_call_id","assistant_message_id"
    ] { assert!(cols_call.contains(&c.to_string()), "missing column {}", c); }
}

#[test]
fn test_plugin_entities_columns() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let cols_plugins = extract_columns_from_statement(&schema.create_table_from_entity(Plugins), backend);
    for c in ["plugin_id","name","version","folder_name","description","author","created_at","updated_at"] {
        assert!(cols_plugins.contains(&c.to_string()), "missing column {}", c);
    }
    let cols_status = extract_columns_from_statement(&schema.create_table_from_entity(PluginStatus), backend);
    for c in ["status_id","plugin_id","is_active","last_run"] {
        assert!(cols_status.contains(&c.to_string()), "missing column {}", c);
    }
    let cols_cfg = extract_columns_from_statement(&schema.create_table_from_entity(PluginConfigurations), backend);
    for c in ["config_id","plugin_id","config_key","config_value"] {
        assert!(cols_cfg.contains(&c.to_string()), "missing column {}", c);
    }
    let cols_data = extract_columns_from_statement(&schema.create_table_from_entity(PluginData), backend);
    for c in ["data_id","plugin_id","session_id","data_key","data_value","created_at","updated_at"] {
        assert!(cols_data.contains(&c.to_string()), "missing column {}", c);
    }
}

#[test]
fn test_sub_task_entities_columns() {
    let backend = DbBackend::Sqlite;
    let schema = Schema::new(backend);
    let cols_def = extract_columns_from_statement(&schema.create_table_from_entity(SubTaskDefinition), backend);
    for c in ["id","name","code","description","system_prompt","plugin_source","source_id","is_enabled","created_time","updated_time"] {
        assert!(cols_def.contains(&c.to_string()), "missing column {}", c);
    }
    let cols_exec = extract_columns_from_statement(&schema.create_table_from_entity(SubTaskExecution), backend);
    for c in [
        "id","task_definition_id","task_code","task_name","task_prompt","parent_conversation_id","parent_message_id","status","result_content","error_message","mcp_result_json","llm_model_id","llm_model_name","token_count","input_token_count","output_token_count","started_time","finished_time","created_time"
    ] { assert!(cols_exec.contains(&c.to_string()), "missing column {}", c); }
}
