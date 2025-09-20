use std::path::PathBuf;

use chrono::prelude::*;
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::utils::db_utils::{get_datetime_from_row, get_required_datetime_from_row};

use super::get_db_path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubTaskDefinition {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub description: String,
    pub system_prompt: String,
    pub plugin_source: String, // 'mcp' | 'plugin'
    pub source_id: i64,
    pub is_enabled: bool,
    pub created_time: DateTime<Utc>,
    pub updated_time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubTaskExecution {
    pub id: i64,
    pub task_definition_id: i64,
    pub task_code: String,
    pub task_name: String,
    pub task_prompt: String,
    pub parent_conversation_id: i64,
    pub parent_message_id: Option<i64>,
    pub status: String, // 'pending' | 'running' | 'success' | 'failed' | 'cancelled'
    pub result_content: Option<String>,
    pub error_message: Option<String>,
    pub mcp_result_json: Option<String>,

    // 消息消费相关字段 (参考 message 表)
    pub llm_model_id: Option<i64>,
    pub llm_model_name: Option<String>,
    pub token_count: i32,
    pub input_token_count: i32,
    pub output_token_count: i32,

    // 时间字段
    pub started_time: Option<DateTime<Utc>>,
    pub finished_time: Option<DateTime<Utc>>,
    pub created_time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubTaskExecutionSummary {
    pub id: i64,
    pub task_code: String,
    pub task_name: String,
    pub task_prompt: String,
    pub status: String,
    pub created_time: DateTime<Utc>,
    pub token_count: i32,
}

impl SubTaskDefinition {
    // helper: nothing for now
}

pub struct SubTaskDatabase {
    pub conn: Connection,
    pub db_path: PathBuf,
}

impl SubTaskDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "conversation.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> rusqlite::Result<Self> {
        let db_path = get_db_path(app_handle, "conversation.db").unwrap();
        let conn = Connection::open(&db_path)?;
        debug!("Opened sub task database");
        Ok(SubTaskDatabase { conn, db_path })
    }

    pub fn get_connection(&self) -> rusqlite::Result<Connection> {
        // Open a fresh connection when needed by migrations or external helpers
        Connection::open(&self.db_path)
    }

    // Definition methods
    #[instrument(level = "debug", skip(self, plugin_source, source_id, is_enabled), fields(plugin_source = plugin_source.unwrap_or("*"), source_id = source_id.map(|v| v.to_string()).unwrap_or_else(|| "*".into()), is_enabled = is_enabled.map(|v| v.to_string()).unwrap_or_else(|| "*".into())))]
    pub fn list_definitions_by_source(
        &self,
        plugin_source: Option<&str>,
        source_id: Option<i64>,
        is_enabled: Option<bool>,
    ) -> Result<Vec<SubTaskDefinition>> {
        let mut query = String::from("SELECT id, name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time FROM sub_task_definition WHERE 1=1");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(source) = plugin_source {
            query.push_str(" AND plugin_source = ?");
            params.push(Box::new(source.to_string()));
        }

        if let Some(sid) = source_id {
            query.push_str(" AND source_id = ?");
            params.push(Box::new(sid));
        }

        if let Some(enabled) = is_enabled {
            query.push_str(" AND is_enabled = ?");
            params.push(Box::new(enabled));
        }

        query.push_str(" ORDER BY created_time DESC");

        let mut stmt = self.conn.prepare(&query)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(&param_refs[..], |row| {
            Ok(SubTaskDefinition {
                id: row.get(0)?,
                name: row.get(1)?,
                code: row.get(2)?,
                description: row.get(3)?,
                system_prompt: row.get(4)?,
                plugin_source: row.get(5)?,
                source_id: row.get(6)?,
                is_enabled: row.get(7)?,
                created_time: row.get(8)?,
                updated_time: row.get(9)?,
            })
        })?;
        let defs: Vec<SubTaskDefinition> = rows.collect::<Result<Vec<_>>>()?;
        debug!(count = defs.len(), "Fetched sub task definitions");
        Ok(defs)
    }
    #[instrument(level = "debug", skip(self), fields(code))]
    pub fn find_definition_by_code(&self, code: &str) -> Result<Option<SubTaskDefinition>> {
        let def = self.conn
            .query_row(
                "SELECT id, name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time FROM sub_task_definition WHERE code = ?",
                [code],
                |row| {
                    Ok(SubTaskDefinition {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        code: row.get(2)?,
                        description: row.get(3)?,
                        system_prompt: row.get(4)?,
                        plugin_source: row.get(5)?,
                        source_id: row.get(6)?,
                        is_enabled: row.get(7)?,
                        created_time: row.get(8)?,
                        updated_time: row.get(9)?,
                    })
                },
            )
            .optional()?;
        debug!(found = def.is_some(), "Fetched definition by code");
        Ok(def)
    }

    #[instrument(level = "debug", skip(self), fields(plugin_source, source_id, code))]
    pub fn find_definition_by_source_and_code(
        &self,
        plugin_source: &str,
        source_id: i64,
        code: &str,
    ) -> Result<Option<SubTaskDefinition>> {
        let def = self.conn
            .query_row(
                "SELECT id, name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time FROM sub_task_definition WHERE plugin_source = ? AND source_id = ? AND code = ?",
                params![plugin_source, source_id, code],
                |row| {
                    Ok(SubTaskDefinition {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        code: row.get(2)?,
                        description: row.get(3)?,
                        system_prompt: row.get(4)?,
                        plugin_source: row.get(5)?,
                        source_id: row.get(6)?,
                        is_enabled: row.get(7)?,
                        created_time: row.get(8)?,
                        updated_time: row.get(9)?,
                    })
                },
            )
            .optional()?;
        debug!(found = def.is_some(), "Fetched definition by source & code");
        Ok(def)
    }

    #[instrument(level = "debug", skip(self, definition), fields(code = %definition.code, source_id = definition.source_id, plugin_source = %definition.plugin_source))]
    pub fn upsert_sub_task_definition(
        &self,
        definition: &SubTaskDefinition,
    ) -> Result<SubTaskDefinition> {
        // First try to find existing definition by source and code
        if let Some(existing) = self.find_definition_by_source_and_code(
            &definition.plugin_source,
            definition.source_id,
            &definition.code,
        )? {
            // Update existing definition
            let updated_definition = SubTaskDefinition {
                id: existing.id,
                name: definition.name.clone(),
                code: definition.code.clone(),
                description: definition.description.clone(),
                system_prompt: definition.system_prompt.clone(),
                plugin_source: existing.plugin_source,
                source_id: existing.source_id,
                is_enabled: definition.is_enabled,
                created_time: existing.created_time,
                updated_time: Utc::now(),
            };

            self.update_sub_task_definition(&updated_definition)?;
            debug!(id = updated_definition.id, "Updated sub task definition");
            Ok(updated_definition)
        } else {
            // Create new definition
            let new_definition = SubTaskDefinition {
                id: 0,
                name: definition.name.clone(),
                code: definition.code.clone(),
                description: definition.description.clone(),
                system_prompt: definition.system_prompt.clone(),
                plugin_source: definition.plugin_source.clone(),
                source_id: definition.source_id,
                is_enabled: definition.is_enabled,
                created_time: Utc::now(),
                updated_time: Utc::now(),
            };

            let inserted = self.create_sub_task_definition(&new_definition)?;
            debug!(id = inserted.id, "Inserted sub task definition");
            Ok(inserted)
        }
    }

    #[instrument(level = "debug", skip(self), fields(id, is_enabled))]
    pub fn update_definition_enabled_status(&self, id: i64, is_enabled: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE sub_task_definition SET is_enabled = ?, updated_time = CURRENT_TIMESTAMP WHERE id = ?",
            params![is_enabled, id],
        )?;
        debug!("Updated definition enabled status");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, definition), fields(code = %definition.code, source_id = definition.source_id))]
    pub fn create_sub_task_definition(
        &self,
        definition: &SubTaskDefinition,
    ) -> Result<SubTaskDefinition> {
        self.conn.execute(
            "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![definition.name, definition.code, definition.description, definition.system_prompt, definition.plugin_source, definition.source_id, definition.is_enabled, definition.created_time, definition.updated_time],
        )?;
        let id = self.conn.last_insert_rowid();
        debug!(id, "Inserted sub task definition row");
        Ok(SubTaskDefinition { id, ..definition.clone() })
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn read_sub_task_definition(&self, id: i64) -> Result<Option<SubTaskDefinition>> {
        let def = self.conn
            .query_row(
                "SELECT id, name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time FROM sub_task_definition WHERE id = ?",
                [id],
                |row| {
                    Ok(SubTaskDefinition {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        code: row.get(2)?,
                        description: row.get(3)?,
                        system_prompt: row.get(4)?,
                        plugin_source: row.get(5)?,
                        source_id: row.get(6)?,
                        is_enabled: row.get(7)?,
                        created_time: row.get(8)?,
                        updated_time: row.get(9)?,
                    })
                },
            )
            .optional()?;
        debug!(found = def.is_some(), "Read definition by id");
        Ok(def)
    }

    #[instrument(level = "debug", skip(self, definition), fields(id = definition.id))]
    pub fn update_sub_task_definition(&self, definition: &SubTaskDefinition) -> Result<()> {
        self.conn.execute(
            "UPDATE sub_task_definition SET name = ?1, description = ?2, system_prompt = ?3, is_enabled = ?4, updated_time = CURRENT_TIMESTAMP WHERE id = ?5",
            params![definition.name, definition.description, definition.system_prompt, definition.is_enabled, definition.id],
        )?;
        debug!("Updated sub task definition row");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn delete_sub_task_definition_row(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM sub_task_definition WHERE id = ?", [id])?;
        debug!("Deleted sub task definition row");
        Ok(())
    }

    // Execution methods
    #[instrument(level = "debug", skip(self, status), fields(parent_conversation_id, parent_message_id, status = status.unwrap_or("*"), page, page_size))]
    pub fn list_executions_by_conversation(
        &self,
        parent_conversation_id: i64,
        parent_message_id: Option<i64>,
        status: Option<&str>,
        page: u32,
        page_size: u32,
    ) -> Result<Vec<SubTaskExecutionSummary>> {
        let offset = (page - 1) * page_size;
        let mut query = String::from(
            "SELECT id, task_code, task_name, task_prompt, status, created_time, token_count 
             FROM sub_task_execution 
             WHERE parent_conversation_id = ?",
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(parent_conversation_id)];

        // 修复：明确处理 parent_message_id 的查询条件
        match parent_message_id {
            Some(msg_id) => {
                query.push_str(" AND parent_message_id = ?");
                params.push(Box::new(msg_id));
            }
            None => {
                // 如果要查询所有消息（包括 parent_message_id 为 NULL 的），保持原逻辑
                // 如果只要查询 parent_message_id 为 NULL 的，使用下面这行：
                // query.push_str(" AND parent_message_id IS NULL");
            }
        }

        if let Some(st) = status {
            query.push_str(" AND status = ?");
            params.push(Box::new(st.to_string()));
        }

        query.push_str(" ORDER BY created_time DESC LIMIT ? OFFSET ?");
        params.push(Box::new(page_size as i64)); // 修复：确保类型一致
        params.push(Box::new(offset as i64)); // 修复：确保类型一致

        let mut stmt = self.conn.prepare(&query)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(&param_refs[..], |row| {
            Ok(SubTaskExecutionSummary {
                id: row.get(0)?,
                task_code: row.get(1)?,
                task_name: row.get(2)?,
                task_prompt: row.get(3)?,
                status: row.get(4)?,
                created_time: get_required_datetime_from_row(row, 5, "created_time")?,
                token_count: row.get(6)?,
            })
        })?;

        let summaries: Vec<SubTaskExecutionSummary> = rows.collect::<Result<Vec<_>>>()?;
        debug!(count = summaries.len(), "Fetched executions by conversation");
        Ok(summaries)
    }

    #[instrument(level = "debug", skip(self, started_time), fields(id, status))]
    pub fn update_execution_status(
        &self,
        id: i64,
        status: &str,
        started_time: Option<DateTime<Utc>>,
    ) -> Result<()> {
        match started_time {
            Some(time) => {
                self.conn.execute(
                    "UPDATE sub_task_execution SET status = ?, started_time = ? WHERE id = ?",
                    params![status, time, id],
                )?;
            }
            None => {
                self.conn.execute(
                    "UPDATE sub_task_execution SET status = ? WHERE id = ?",
                    params![status, id],
                )?;
            }
        }
        debug!("Updated execution status");
        Ok(())
    }

    #[instrument(
        level = "debug",
        skip(self, result_content, error_message, token_stats, finished_time),
        fields(id, status)
    )]
    pub fn update_execution_result(
        &self,
        id: i64,
        status: &str,
        result_content: Option<&str>,
        error_message: Option<&str>,
        token_stats: Option<(i32, i32, i32)>,
        finished_time: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let (token_count, input_tokens, output_tokens) = token_stats.unwrap_or((0, 0, 0));

        self.conn.execute(
            "UPDATE sub_task_execution SET status = ?, result_content = ?, error_message = ?, token_count = ?, input_token_count = ?, output_token_count = ?, finished_time = ? WHERE id = ?",
            params![status, result_content, error_message, token_count, input_tokens, output_tokens, finished_time, id],
        )?;
        debug!(token_count, input_tokens, output_tokens, "Updated execution result");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, source_id), fields(parent_conversation_id, source_id = source_id.map(|v| v.to_string()).unwrap_or_else(|| "*".into())))]
    pub fn list_executions_by_source_filter(
        &self,
        parent_conversation_id: i64,
        source_id: Option<i64>,
    ) -> Result<Vec<SubTaskExecutionSummary>> {
        let mut query = String::from(
            "SELECT ste.id, ste.task_code, ste.task_name, ste.task_prompt, ste.status, ste.created_time, ste.token_count 
             FROM sub_task_execution ste
             JOIN sub_task_definition std ON ste.task_definition_id = std.id
             WHERE ste.parent_conversation_id = ?",
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(parent_conversation_id)];

        if let Some(sid) = source_id {
            query.push_str(" AND std.source_id = ?");
            params.push(Box::new(sid));
        }

        query.push_str(" ORDER BY ste.created_time DESC");

        let mut stmt = self.conn.prepare(&query)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(&param_refs[..], |row| {
            Ok(SubTaskExecutionSummary {
                id: row.get(0)?,
                task_code: row.get(1)?,
                task_name: row.get(2)?,
                task_prompt: row.get(3)?,
                status: row.get(4)?,
                created_time: get_required_datetime_from_row(row, 5, "created_time")?,
                token_count: row.get(6)?,
            })
        })?;
        let items: Vec<SubTaskExecutionSummary> = rows.collect::<Result<Vec<_>>>()?;
        debug!(count = items.len(), "Fetched executions by source filter");
        Ok(items)
    }

    #[instrument(level = "debug", skip(self, execution), fields(task_code = %execution.task_code, parent_conversation_id = execution.parent_conversation_id))]
    pub fn create_sub_task_execution(
        &self,
        execution: &SubTaskExecution,
    ) -> Result<SubTaskExecution> {
        // 使用 rusqlite::params! 宏来处理多个参数
        self.conn.execute(
            "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, parent_message_id, status, result_content, error_message, mcp_result_json, llm_model_id, llm_model_name, token_count, input_token_count, output_token_count, started_time, finished_time, created_time) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            rusqlite::params![
                execution.task_definition_id,
                execution.task_code,
                execution.task_name,
                execution.task_prompt,
                execution.parent_conversation_id,
                execution.parent_message_id,
                execution.status,
                execution.result_content,
                execution.error_message,
                execution.mcp_result_json,
                execution.llm_model_id,
                execution.llm_model_name,
                execution.token_count,
                execution.input_token_count,
                execution.output_token_count,
                execution.started_time,
                execution.finished_time,
                execution.created_time,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        debug!(id, "Inserted sub task execution row");
        Ok(SubTaskExecution {
            id,
            task_definition_id: execution.task_definition_id,
            task_code: execution.task_code.clone(),
            task_name: execution.task_name.clone(),
            task_prompt: execution.task_prompt.clone(),
            parent_conversation_id: execution.parent_conversation_id,
            parent_message_id: execution.parent_message_id,
            status: execution.status.clone(),
            result_content: execution.result_content.clone(),
            error_message: execution.error_message.clone(),
            mcp_result_json: execution.mcp_result_json.clone(),
            llm_model_id: execution.llm_model_id,
            llm_model_name: execution.llm_model_name.clone(),
            token_count: execution.token_count,
            input_token_count: execution.input_token_count,
            output_token_count: execution.output_token_count,
            started_time: execution.started_time,
            finished_time: execution.finished_time,
            created_time: execution.created_time,
        })
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn read_sub_task_execution(&self, id: i64) -> Result<Option<SubTaskExecution>> {
        let exec = self.conn
            .query_row(
                "SELECT id, task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, parent_message_id, status, result_content, error_message, mcp_result_json, llm_model_id, llm_model_name, token_count, input_token_count, output_token_count, started_time, finished_time, created_time FROM sub_task_execution WHERE id = ?",
                [id],
                |row| {
                    Ok(SubTaskExecution {
                        id: row.get(0)?,
                        task_definition_id: row.get(1)?,
                        task_code: row.get(2)?,
                        task_name: row.get(3)?,
                        task_prompt: row.get(4)?,
                        parent_conversation_id: row.get(5)?,
                        parent_message_id: row.get(6)?,
                        status: row.get(7)?,
                        result_content: row.get(8)?,
                        error_message: row.get(9)?,
                        mcp_result_json: row.get(10)?,
                        llm_model_id: row.get(11)?,
                        llm_model_name: row.get(12)?,
                        token_count: row.get(13)?,
                        input_token_count: row.get(14)?,
                        output_token_count: row.get(15)?,
                        started_time: get_datetime_from_row(row, 16)?,
                        finished_time: get_datetime_from_row(row, 17)?,
                        created_time: get_required_datetime_from_row(row, 18, "created_time")?,
                    })
                },
            )
            .optional()?;
        debug!(found = exec.is_some(), "Read execution by id");
        Ok(exec)
    }

    #[instrument(level = "debug", skip(self, execution), fields(id = execution.id))]
    pub fn update_sub_task_execution(&self, execution: &SubTaskExecution) -> Result<()> {
        self.conn.execute(
            "UPDATE sub_task_execution SET task_prompt = ?1, status = ?2, result_content = ?3, error_message = ?4, mcp_result_json = ?5, token_count = ?6, input_token_count = ?7, output_token_count = ?8, finished_time = ?9 WHERE id = ?10",
            params![execution.task_prompt, execution.status, execution.result_content, execution.error_message, execution.mcp_result_json, execution.token_count, execution.input_token_count, execution.output_token_count, execution.finished_time, execution.id],
        )?;
        debug!("Updated sub task execution row");
        Ok(())
    }

    /// Update only the mcp_result_json column for a given subtask execution
    pub fn set_execution_mcp_result_json(
        &self,
        id: i64,
        mcp_result_json: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sub_task_execution SET mcp_result_json = ?1 WHERE id = ?2",
            params![mcp_result_json, id],
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn delete_sub_task_execution_row(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM sub_task_execution WHERE id = ?", [id])?;
        debug!("Deleted sub task execution row");
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn create_tables(&self) -> rusqlite::Result<()> {
        let conn = &self.conn;

        // 创建 sub_task_definition 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sub_task_definition (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                code TEXT NOT NULL UNIQUE,
                description TEXT NOT NULL,
                system_prompt TEXT NOT NULL,
                plugin_source TEXT NOT NULL,
                source_id INTEGER NOT NULL,
                is_enabled BOOLEAN NOT NULL DEFAULT 1,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_time DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        // 创建 sub_task_execution 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sub_task_execution (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_definition_id INTEGER NOT NULL,
                task_code TEXT NOT NULL,
                task_name TEXT NOT NULL,
                task_prompt TEXT NOT NULL,
                parent_conversation_id INTEGER NOT NULL,
                parent_message_id INTEGER,
                status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'running', 'success', 'failed', 'cancelled')),
                result_content TEXT,
                error_message TEXT,
                mcp_result_json TEXT,
                
                -- 消息消费相关字段
                llm_model_id INTEGER,
                llm_model_name TEXT,
                token_count INTEGER DEFAULT 0,
                input_token_count INTEGER DEFAULT 0,
                output_token_count INTEGER DEFAULT 0,
                
                -- 时间字段
                started_time DATETIME,
                finished_time DATETIME,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                
                -- 外键约束
                FOREIGN KEY (task_definition_id) REFERENCES sub_task_definition(id) ON DELETE CASCADE
            )",
            [],
        )?;

        // Migration: ensure mcp_result_json column exists
        if let Ok(mut stmt) = conn.prepare("PRAGMA table_info(sub_task_execution)") {
            let mut has_mcp_col = false;
            let cols = stmt.query_map([], |row| Ok(row.get::<_, String>(1)?))?;
            for c in cols {
                if let Ok(name) = c {
                    if name == "mcp_result_json" {
                        has_mcp_col = true;
                        break;
                    }
                }
            }
            if !has_mcp_col {
                let _ = conn
                    .execute("ALTER TABLE sub_task_execution ADD COLUMN mcp_result_json TEXT", []);
            }
        }

        // 创建索引以优化查询性能
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sub_task_definition_code ON sub_task_definition(code)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sub_task_definition_source ON sub_task_definition(plugin_source, source_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sub_task_execution_conversation ON sub_task_execution(parent_conversation_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sub_task_execution_message ON sub_task_execution(parent_message_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sub_task_execution_status ON sub_task_execution(status)",
            [],
        )?;

        debug!("Sub task tables ensured");
        Ok(())
    }
}
