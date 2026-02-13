use rusqlite::{params_from_iter, Connection, Result};
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
    /// 获取 artifact 数据目录
    fn get_artifact_data_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
        let app_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
        let data_path = app_dir.join("artifact_data");
        std::fs::create_dir_all(&data_path).map_err(|e| e.to_string())?;
        Ok(data_path)
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
        if !db_id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(
                "db_id can only contain alphanumeric characters, underscores, and hyphens"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// 打开或创建指定 db_id 的数据库
    pub fn new(app_handle: &tauri::AppHandle, db_id: &str) -> Result<Self, String> {
        Self::validate_db_id(db_id)?;

        let data_dir = Self::get_artifact_data_dir(app_handle)?;
        let db_path = data_dir.join(format!("{}.db", db_id));

        let conn = Connection::open(&db_path).map_err(|e| format!("Failed to open database: {}", e))?;

        // 启用 WAL 模式以提高并发性能
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| format!("Failed to set pragmas: {}", e))?;

        Ok(ArtifactDataDatabase {
            conn,
            db_id: db_id.to_string(),
        })
    }

    /// 执行查询语句 (SELECT)
    pub fn query(&self, sql: &str, params: Vec<JsonValue>) -> Result<QueryResult, String> {
        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        // 获取列名
        let columns: Vec<String> = stmt
            .column_names()
            .iter()
            .map(|s| s.to_string())
            .collect();

        // 转换参数
        let params_vec: Vec<Box<dyn rusqlite::ToSql>> = params
            .iter()
            .map(|v| json_to_sql_param(v))
            .collect();

        let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();

        // 执行查询
        let mut rows_data: Vec<Vec<JsonValue>> = Vec::new();
        let mut rows = stmt
            .query(param_refs.as_slice())
            .map_err(|e| format!("Failed to execute query: {}", e))?;

        while let Some(row) = rows.next().map_err(|e| format!("Failed to fetch row: {}", e))? {
            let mut row_data = Vec::new();
            for i in 0..columns.len() {
                let value = row_value_to_json(row, i)?;
                row_data.push(value);
            }
            rows_data.push(row_data);
        }

        let row_count = rows_data.len();
        Ok(QueryResult {
            columns,
            rows: rows_data,
            row_count,
        })
    }

    /// 执行修改语句 (INSERT/UPDATE/DELETE/CREATE/DROP)
    pub fn execute(&self, sql: &str, params: Vec<JsonValue>) -> Result<ExecuteResult, String> {
        // 转换参数
        let params_vec: Vec<Box<dyn rusqlite::ToSql>> = params
            .iter()
            .map(|v| json_to_sql_param(v))
            .collect();

        let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();

        let rows_affected = self
            .conn
            .execute(sql, param_refs.as_slice())
            .map_err(|e| format!("Failed to execute statement: {}", e))?;

        let last_insert_rowid = self.conn.last_insert_rowid();

        Ok(ExecuteResult {
            rows_affected,
            last_insert_rowid,
        })
    }

    /// 批量执行语句（用于初始化表结构等）
    pub fn execute_batch(&self, sql: &str) -> Result<(), String> {
        self.conn
            .execute_batch(sql)
            .map_err(|e| format!("Failed to execute batch: {}", e))
    }

    /// 获取数据库中所有表的信息
    pub fn get_tables(&self) -> Result<Vec<TableInfo>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let tables = stmt
            .query_map([], |row| {
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
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("Failed to query columns: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(columns)
    }

    /// 检查数据库是否存在
    pub fn exists(app_handle: &tauri::AppHandle, db_id: &str) -> Result<bool, String> {
        Self::validate_db_id(db_id)?;
        let data_dir = Self::get_artifact_data_dir(app_handle)?;
        let db_path = data_dir.join(format!("{}.db", db_id));
        Ok(db_path.exists())
    }

    /// 删除数据库
    pub fn delete(app_handle: &tauri::AppHandle, db_id: &str) -> Result<(), String> {
        Self::validate_db_id(db_id)?;
        let data_dir = Self::get_artifact_data_dir(app_handle)?;
        let db_path = data_dir.join(format!("{}.db", db_id));

        if db_path.exists() {
            std::fs::remove_file(&db_path).map_err(|e| format!("Failed to delete database: {}", e))?;
            // 也删除 WAL 和 SHM 文件
            let wal_path = data_dir.join(format!("{}.db-wal", db_id));
            let shm_path = data_dir.join(format!("{}.db-shm", db_id));
            let _ = std::fs::remove_file(wal_path);
            let _ = std::fs::remove_file(shm_path);
        }
        Ok(())
    }

    /// 列出所有 artifact 数据库
    pub fn list_databases(app_handle: &tauri::AppHandle) -> Result<Vec<String>, String> {
        let data_dir = Self::get_artifact_data_dir(app_handle)?;

        let entries = std::fs::read_dir(&data_dir).map_err(|e| format!("Failed to read directory: {}", e))?;

        let mut db_ids = Vec::new();
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".db") && !name.ends_with("-wal") && !name.ends_with("-shm") {
                    db_ids.push(name.trim_end_matches(".db").to_string());
                }
            }
        }

        Ok(db_ids)
    }
}

/// 将 JSON 值转换为 SQL 参数
fn json_to_sql_param(value: &JsonValue) -> Box<dyn rusqlite::ToSql> {
    match value {
        JsonValue::Null => Box::new(rusqlite::types::Null),
        JsonValue::Bool(b) => Box::new(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else if let Some(f) = n.as_f64() {
                Box::new(f)
            } else {
                Box::new(n.to_string())
            }
        }
        JsonValue::String(s) => Box::new(s.clone()),
        JsonValue::Array(_) | JsonValue::Object(_) => Box::new(value.to_string()),
    }
}

/// 将 SQLite 行值转换为 JSON
fn row_value_to_json(row: &rusqlite::Row, idx: usize) -> Result<JsonValue, String> {
    use rusqlite::types::ValueRef;

    let value = row
        .get_ref(idx)
        .map_err(|e| format!("Failed to get column value: {}", e))?;

    Ok(match value {
        ValueRef::Null => JsonValue::Null,
        ValueRef::Integer(i) => JsonValue::Number(i.into()),
        ValueRef::Real(f) => {
            serde_json::Number::from_f64(f)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null)
        }
        ValueRef::Text(s) => {
            let text = std::str::from_utf8(s).map_err(|e| format!("Invalid UTF-8: {}", e))?;
            JsonValue::String(text.to_string())
        }
        ValueRef::Blob(b) => {
            // 将 blob 转为 base64 字符串
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(b);
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
