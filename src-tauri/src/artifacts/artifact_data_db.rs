use crate::db::connection::{params_from_iter, sync_metadata_path, Connection};
use crate::db::get_db_dir;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::PathBuf;
use tauri::Manager;

/// 管理 Artifact 独立数据库
/// 每个 db_id 对应一个独立的 SQLite 数据库文件
pub struct ArtifactDataDatabase {
    pub conn: Connection,
    pub db_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<JsonValue>>,
    pub row_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExecuteResult {
    pub rows_affected: usize,
    pub last_insert_rowid: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableInfo {
    pub name: String,
    pub sql: String,
}

impl ArtifactDataDatabase {
    const MANAGED_DB_PREFIX: &'static str = "artifact-data-";

    /// 获取旧版 artifact 数据目录
    fn get_legacy_artifact_data_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
        let app_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
        let data_path = app_dir.join("artifact_data");
        std::fs::create_dir_all(&data_path).map_err(|e| e.to_string())?;
        Ok(data_path)
    }

    fn managed_db_file_name(db_id: &str) -> String {
        format!("{}{}.db", Self::MANAGED_DB_PREFIX, db_id)
    }

    fn managed_db_path(app_handle: &tauri::AppHandle, db_id: &str) -> Result<PathBuf, String> {
        Ok(get_db_dir(app_handle)?.join(Self::managed_db_file_name(db_id)))
    }

    fn legacy_db_path(app_handle: &tauri::AppHandle, db_id: &str) -> Result<PathBuf, String> {
        Ok(Self::get_legacy_artifact_data_dir(app_handle)?.join(format!("{}.db", db_id)))
    }

    fn parse_managed_db_id(file_name: &str) -> Option<String> {
        let suffix = file_name.strip_prefix(Self::MANAGED_DB_PREFIX)?;
        suffix.strip_suffix(".db").map(str::to_string)
    }

    /// 验证 db_id 是否合法（防止路径注入）
    fn validate_db_id(db_id: &str) -> Result<(), String> {
        if db_id.is_empty() {
            return Err("db_id cannot be empty".to_string());
        }
        if db_id.len() > 64 {
            return Err("db_id too long (max 64 characters)".to_string());
        }
        // 只允许字母、数字、下划线、连字符
        if !db_id.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return Err("db_id can only contain alphanumeric characters, underscores, and hyphens"
                .to_string());
        }
        Ok(())
    }

    /// 打开或创建指定 db_id 的数据库
    pub fn new(app_handle: &tauri::AppHandle, db_id: &str) -> Result<Self, String> {
        Self::validate_db_id(db_id)?;
        Self::migrate_legacy_database(app_handle, db_id)?;
        let db_path = Self::managed_db_path(app_handle, db_id)?;

        let conn =
            Connection::open(&db_path).map_err(|e| format!("Failed to open database: {}", e))?;

        // 启用 WAL 模式以提高并发性能
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| format!("Failed to set pragmas: {}", e))?;

        Ok(ArtifactDataDatabase { conn, db_id: db_id.to_string() })
    }

    /// 执行查询语句 (SELECT)
    pub fn query(&self, sql: &str, params: Vec<JsonValue>) -> Result<QueryResult, String> {
        let mut stmt =
            self.conn.prepare(sql).map_err(|e| format!("Failed to prepare statement: {}", e))?;

        // 转换参数
        let param_values: Vec<libsql::Value> =
            params.iter().map(|v| json_to_libsql_value(v)).collect();

        // 获取列名（从第一行中提取）和执行查询
        let mut columns: Vec<String> = Vec::new();
        let rows_result = stmt
            .query_map(params_from_iter(param_values), |row| {
                if columns.is_empty() {
                    let count = row.column_count();
                    for i in 0..count {
                        if let Some(name) = row.column_name(i) {
                            columns.push(name.to_string());
                        }
                    }
                }
                let col_count =
                    if !columns.is_empty() { columns.len() } else { row.column_count() as usize };

                let mut row_data = Vec::new();
                for i in 0..col_count {
                    let value = row_value_to_json(row, i)
                        .map_err(|e| crate::db::connection::DbError::Custom(e))?;
                    row_data.push(value);
                }
                Ok(row_data)
            })
            .map_err(|e| format!("Failed to execute query: {}", e))?;

        let mut rows_data: Vec<Vec<JsonValue>> = Vec::new();
        for row_result in rows_result {
            rows_data.push(row_result.map_err(|e| format!("Failed to fetch row: {}", e))?);
        }

        let row_count = rows_data.len();
        Ok(QueryResult { columns, rows: rows_data, row_count })
    }

    /// 执行修改语句 (INSERT/UPDATE/DELETE/CREATE/DROP)
    pub fn execute(&self, sql: &str, params: Vec<JsonValue>) -> Result<ExecuteResult, String> {
        // 转换参数
        let param_values: Vec<libsql::Value> =
            params.iter().map(|v| json_to_libsql_value(v)).collect();

        let rows_affected = self
            .conn
            .execute(sql, params_from_iter(param_values))
            .map_err(|e| format!("Failed to execute statement: {}", e))?;

        let last_insert_rowid = self.conn.last_insert_rowid();

        Ok(ExecuteResult { rows_affected, last_insert_rowid })
    }

    /// 批量执行语句（用于初始化表结构等）
    pub fn execute_batch(&self, sql: &str) -> Result<(), String> {
        self.conn.execute_batch(sql).map_err(|e| format!("Failed to execute batch: {}", e))
    }

    /// 获取数据库中所有表的信息
    pub fn get_tables(&self) -> Result<Vec<TableInfo>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let tables = stmt
            .query_map((), |row| {
                Ok(TableInfo {
                    name: row.get(0)?,
                    sql: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                })
            })
            .map_err(|e| format!("Failed to query tables: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(tables)
    }

    /// 获取指定表的列信息
    pub fn get_table_columns(&self, table_name: &str) -> Result<Vec<String>, String> {
        let sql = format!("PRAGMA table_info({})", table_name);
        let mut stmt =
            self.conn.prepare(&sql).map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let columns = stmt
            .query_map((), |row| row.get::<_, String>(1))
            .map_err(|e| format!("Failed to query columns: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(columns)
    }

    /// 检查数据库是否存在
    pub fn exists(app_handle: &tauri::AppHandle, db_id: &str) -> Result<bool, String> {
        Self::validate_db_id(db_id)?;
        Self::migrate_legacy_database(app_handle, db_id)?;
        let managed_db_path = Self::managed_db_path(app_handle, db_id)?;
        let legacy_db_path = Self::legacy_db_path(app_handle, db_id)?;
        Ok(managed_db_path.exists() || legacy_db_path.exists())
    }

    /// 删除数据库
    pub fn delete(app_handle: &tauri::AppHandle, db_id: &str) -> Result<(), String> {
        Self::validate_db_id(db_id)?;
        for db_path in
            [Self::managed_db_path(app_handle, db_id)?, Self::legacy_db_path(app_handle, db_id)?]
        {
            if db_path.exists() {
                std::fs::remove_file(&db_path)
                    .map_err(|e| format!("Failed to delete database: {}", e))?;
            }
            for sidecar in [
                PathBuf::from(format!("{}-wal", db_path.to_string_lossy())),
                PathBuf::from(format!("{}-shm", db_path.to_string_lossy())),
                sync_metadata_path(&db_path),
            ] {
                let _ = std::fs::remove_file(sidecar);
            }
        }
        Ok(())
    }

    /// 列出所有 artifact 数据库
    pub fn list_databases(app_handle: &tauri::AppHandle) -> Result<Vec<String>, String> {
        Self::migrate_all_legacy_databases(app_handle)?;

        let db_dir = get_db_dir(app_handle)?;
        let entries =
            std::fs::read_dir(&db_dir).map_err(|e| format!("Failed to read directory: {}", e))?;

        let mut db_ids = std::collections::BTreeSet::new();
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(db_id) = Self::parse_managed_db_id(name) {
                    db_ids.insert(db_id);
                }
            }
        }

        let legacy_dir = Self::get_legacy_artifact_data_dir(app_handle)?;
        if legacy_dir.exists() {
            let legacy_entries = std::fs::read_dir(&legacy_dir)
                .map_err(|e| format!("Failed to read legacy artifact directory: {}", e))?;
            for entry in legacy_entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".db") && !name.ends_with("-wal") && !name.ends_with("-shm") {
                        db_ids.insert(name.trim_end_matches(".db").to_string());
                    }
                }
            }
        }

        let db_ids: Vec<String> = db_ids.into_iter().collect();
        Ok(db_ids)
    }

    pub fn migrate_all_legacy_databases(app_handle: &tauri::AppHandle) -> Result<(), String> {
        let legacy_dir = Self::get_legacy_artifact_data_dir(app_handle)?;
        if !legacy_dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(&legacy_dir)
            .map_err(|e| format!("Failed to read legacy artifact directory: {}", e))?;
        for entry in entries.flatten() {
            let Some(file_name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if !file_name.ends_with(".db") {
                continue;
            }
            let db_id = file_name.trim_end_matches(".db");
            if Self::validate_db_id(db_id).is_ok() {
                Self::migrate_legacy_database(app_handle, db_id)?;
            }
        }

        Ok(())
    }

    fn migrate_legacy_database(app_handle: &tauri::AppHandle, db_id: &str) -> Result<(), String> {
        let legacy_db_path = Self::legacy_db_path(app_handle, db_id)?;
        if !legacy_db_path.exists() {
            return Ok(());
        }

        let managed_db_path = Self::managed_db_path(app_handle, db_id)?;
        if managed_db_path.exists() {
            return Ok(());
        }

        for (source, target) in [
            (legacy_db_path.clone(), managed_db_path.clone()),
            (
                PathBuf::from(format!("{}-wal", legacy_db_path.to_string_lossy())),
                PathBuf::from(format!("{}-wal", managed_db_path.to_string_lossy())),
            ),
            (
                PathBuf::from(format!("{}-shm", legacy_db_path.to_string_lossy())),
                PathBuf::from(format!("{}-shm", managed_db_path.to_string_lossy())),
            ),
        ] {
            move_file_if_exists(&source, &target)?;
        }

        Ok(())
    }
}

fn move_file_if_exists(source: &PathBuf, target: &PathBuf) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!("Failed to create target directory `{}`: {}", parent.display(), e)
        })?;
    }

    match std::fs::rename(source, target) {
        Ok(_) => Ok(()),
        Err(_) => {
            std::fs::copy(source, target).map_err(|e| {
                format!(
                    "Failed to migrate artifact DB `{}` to `{}`: {}",
                    source.display(),
                    target.display(),
                    e
                )
            })?;
            std::fs::remove_file(source).map_err(|e| {
                format!("Failed to remove legacy artifact DB `{}`: {}", source.display(), e)
            })
        }
    }
}

/// 将 JSON 值转换为 SQL 参数
fn json_to_libsql_value(value: &JsonValue) -> libsql::Value {
    match value {
        JsonValue::Null => libsql::Value::Null,
        JsonValue::Bool(b) => libsql::Value::Integer(if *b { 1 } else { 0 }),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                libsql::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                libsql::Value::Real(f)
            } else {
                libsql::Value::Text(n.to_string())
            }
        }
        JsonValue::String(s) => libsql::Value::Text(s.clone()),
        JsonValue::Array(_) | JsonValue::Object(_) => libsql::Value::Text(value.to_string()),
    }
}

/// 将 SQLite 行值转换为 JSON
fn row_value_to_json(
    row: &crate::db::connection::Row,
    idx: usize,
) -> std::result::Result<JsonValue, String> {
    let value = row.get_value(idx).map_err(|e| format!("Failed to get column value: {}", e))?;

    Ok(match value {
        libsql::Value::Null => JsonValue::Null,
        libsql::Value::Integer(i) => JsonValue::Number(i.into()),
        libsql::Value::Real(f) => {
            serde_json::Number::from_f64(f).map(JsonValue::Number).unwrap_or(JsonValue::Null)
        }
        libsql::Value::Text(s) => JsonValue::String(s),
        libsql::Value::Blob(b) => {
            // 将 blob 转为 base64 字符串
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&b);
            JsonValue::String(format!("base64:{}", encoded))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_db_id() {
        assert!(ArtifactDataDatabase::validate_db_id("my-app").is_ok());
        assert!(ArtifactDataDatabase::validate_db_id("my_app_123").is_ok());
        assert!(ArtifactDataDatabase::validate_db_id("").is_err());
        assert!(ArtifactDataDatabase::validate_db_id("../evil").is_err());
        assert!(ArtifactDataDatabase::validate_db_id("a".repeat(65).as_str()).is_err());
    }
}
