use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::db::get_db_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServer {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub transport_type: String, // stdio, sse, http, builtin
    pub command: Option<String>,
    pub environment_variables: Option<String>,
    pub headers: Option<String>, // JSON map string for custom request headers
    pub url: Option<String>,
    pub timeout: Option<i32>,
    pub is_long_running: bool,
    pub is_enabled: bool,
    pub is_builtin: bool,    // 标识是否为内置服务器
    pub is_deletable: bool,  // 标识是否可删除（系统初始化的内置工具集不可删除）
    pub proxy_enabled: bool, // 是否使用全局网络代理
    pub created_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServerTool {
    pub id: i64,
    pub server_id: i64,
    pub tool_name: String,
    pub tool_description: Option<String>,
    pub is_enabled: bool,
    pub is_auto_run: bool,
    pub parameters: Option<String>, // JSON string of tool parameters
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPServerResource {
    pub id: i64,
    pub server_id: i64,
    pub resource_uri: String,
    pub resource_name: String,
    pub resource_type: String,
    pub resource_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPServerPrompt {
    pub id: i64,
    pub server_id: i64,
    pub prompt_name: String,
    pub prompt_description: Option<String>,
    pub is_enabled: bool,
    pub arguments: Option<String>, // JSON string of prompt arguments
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolCall {
    pub id: i64,
    pub conversation_id: i64,
    pub message_id: Option<i64>,
    pub subtask_id: Option<i64>, // 新增：关联的子任务执行 ID
    pub server_id: i64,
    pub server_name: String,
    pub tool_name: String,
    pub parameters: String,     // JSON string of parameters
    pub status: String,         // pending, executing, success, failed
    pub result: Option<String>, // JSON string of result
    pub error: Option<String>,
    pub created_time: String,
    pub started_time: Option<String>,
    pub finished_time: Option<String>,
    pub llm_call_id: Option<String>,       // LLM 原生 tool_call_id
    pub assistant_message_id: Option<i64>, // 关联的 assistant 消息ID
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServerCapabilityEpochCatalog {
    pub server_id: i64,
    pub server_name: String,
    pub epoch: i64,
    pub last_refresh_at: String,
    pub summary: String,
    pub summary_generated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolCatalogEntry {
    pub tool_id: i64,
    pub server_id: i64,
    pub server_name: String,
    pub tool_name: String,
    pub summary: String,
    pub keywords_json: String,
    pub schema_hash: String,
    pub capability_epoch: i64,
    pub updated_at: String,
    pub summary_generated_at: Option<String>,
    pub server_enabled: bool,
    pub tool_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationLoadedMCPTool {
    pub id: i64,
    pub conversation_id: i64,
    pub tool_id: i64,
    pub loaded_server_name: String,
    pub loaded_tool_name: String,
    pub loaded_schema_hash: String,
    pub loaded_epoch: i64,
    pub status: String,
    pub invalid_reason: Option<String>,
    pub source: Option<String>,
    pub loaded_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationLoadedMCPToolResolved {
    pub id: i64,
    pub conversation_id: i64,
    pub tool_id: i64,
    pub server_id: i64,
    pub server_name: String,
    pub tool_name: String,
    pub tool_description: String,
    pub parameters: String,
}

pub struct MCPDatabase {
    pub conn: Connection,
}

impl MCPDatabase {
    #[instrument(level = "trace", skip(app_handle))]
    pub fn new(app_handle: &tauri::AppHandle) -> rusqlite::Result<Self> {
        let db_path = get_db_path(app_handle, "mcp.db");
        let conn = Connection::open(db_path.unwrap())?;
        Ok(MCPDatabase { conn })
    }

    fn short_hash(s: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    fn schema_hash(parameters: Option<&str>) -> String {
        Self::short_hash(parameters.unwrap_or("{}").trim())
    }

    fn now_string() -> String {
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }

    pub fn create_tables(&self) -> rusqlite::Result<()> {
        // Create MCP servers table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_server (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                description TEXT,
                transport_type TEXT NOT NULL,
                command TEXT,
                environment_variables TEXT,
                headers TEXT,
                url TEXT,
                timeout INTEGER DEFAULT 30000,
                is_long_running BOOLEAN NOT NULL DEFAULT 0,
                is_enabled BOOLEAN NOT NULL DEFAULT 1,
                is_builtin BOOLEAN NOT NULL DEFAULT 0,
                is_deletable BOOLEAN NOT NULL DEFAULT 1,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP
            );",
            [],
        )?;

        // Create MCP server tools table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_server_tool (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                server_id INTEGER NOT NULL,
                tool_name TEXT NOT NULL,
                tool_description TEXT,
                is_enabled BOOLEAN NOT NULL DEFAULT 1,
                is_auto_run BOOLEAN NOT NULL DEFAULT 0,
                parameters TEXT,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
                UNIQUE(server_id, tool_name)
            );",
            [],
        )?;

        // Create MCP server resources table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_server_resource (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                server_id INTEGER NOT NULL,
                resource_uri TEXT NOT NULL,
                resource_name TEXT NOT NULL,
                resource_type TEXT NOT NULL,
                resource_description TEXT,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
                UNIQUE(server_id, resource_uri)
            );",
            [],
        )?;

        // Create MCP server prompts table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_server_prompt (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                server_id INTEGER NOT NULL,
                prompt_name TEXT NOT NULL,
                prompt_description TEXT,
                is_enabled BOOLEAN NOT NULL DEFAULT 1,
                arguments TEXT,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
                UNIQUE(server_id, prompt_name)
            );",
            [],
        )?;

        // Create MCP tool calls history table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_tool_call (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL,
                message_id INTEGER,
                server_id INTEGER NOT NULL,
                server_name TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                parameters TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'executing', 'success', 'failed')),
                result TEXT,
                error TEXT,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                started_time DATETIME,
                finished_time DATETIME,
                llm_call_id TEXT,
                assistant_message_id INTEGER,
                FOREIGN KEY (server_id) REFERENCES mcp_server(id) ON DELETE CASCADE
            );",
            [],
        )?;

        self.migrate_mcp_tool_call_table()?;
        self.migrate_mcp_server_table()?; // ensure headers column exists
        self.create_dynamic_loading_tables()?;
        let _ = self.rebuild_dynamic_mcp_catalog();

        Ok(())
    }

    fn create_dynamic_loading_tables(&self) -> rusqlite::Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_server_capability_epoch_catalog (
                server_id INTEGER PRIMARY KEY,
                epoch INTEGER NOT NULL DEFAULT 1,
                last_refresh_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                summary TEXT NOT NULL DEFAULT '',
                summary_generated_at DATETIME
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_tool_catalog (
                tool_id INTEGER PRIMARY KEY,
                server_id INTEGER NOT NULL,
                tool_name TEXT NOT NULL,
                summary TEXT NOT NULL DEFAULT '',
                keywords_json TEXT NOT NULL DEFAULT '[]',
                schema_hash TEXT NOT NULL,
                capability_epoch INTEGER NOT NULL DEFAULT 1,
                updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                summary_generated_at DATETIME
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mcp_tool_catalog_server_tool_name ON mcp_tool_catalog(server_id, tool_name)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mcp_tool_catalog_schema_hash ON mcp_tool_catalog(schema_hash)",
            [],
        )?;
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS conversation_mcp_loaded_tool (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL,
                tool_id INTEGER NOT NULL,
                loaded_server_name TEXT NOT NULL DEFAULT '',
                loaded_tool_name TEXT NOT NULL DEFAULT '',
                loaded_schema_hash TEXT NOT NULL,
                loaded_epoch INTEGER NOT NULL,
                status TEXT NOT NULL,
                invalid_reason TEXT,
                source TEXT,
                loaded_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(conversation_id, tool_id)
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversation_mcp_loaded_tool_conversation ON conversation_mcp_loaded_tool(conversation_id)",
            [],
        )?;
        Ok(())
    }

    /// Migrate existing mcp_tool_call table to add new columns
    fn migrate_mcp_tool_call_table(&self) -> rusqlite::Result<()> {
        // Check if columns exist
        let columns_result = self.conn.prepare("PRAGMA table_info(mcp_tool_call)");

        match columns_result {
            Ok(mut stmt) => {
                let column_info = stmt.query_map([], |row| {
                    Ok(row.get::<_, String>(1)?) // column name is at index 1
                })?;

                let mut has_llm_call_id = false;
                let mut has_assistant_message_id = false;
                let mut has_subtask_id = false;

                for column in column_info {
                    match column {
                        Ok(name) => {
                            if name == "llm_call_id" {
                                has_llm_call_id = true;
                            } else if name == "assistant_message_id" {
                                has_assistant_message_id = true;
                            } else if name == "subtask_id" {
                                has_subtask_id = true;
                            }
                        }
                        Err(_) => continue,
                    }
                }

                // Add missing columns
                if !has_llm_call_id {
                    self.conn
                        .execute("ALTER TABLE mcp_tool_call ADD COLUMN llm_call_id TEXT", [])?;
                }
                if !has_assistant_message_id {
                    self.conn.execute(
                        "ALTER TABLE mcp_tool_call ADD COLUMN assistant_message_id INTEGER",
                        [],
                    )?;
                }
                if !has_subtask_id {
                    self.conn
                        .execute("ALTER TABLE mcp_tool_call ADD COLUMN subtask_id INTEGER", [])?;
                }
            }
            Err(_) => {
                // Table might not exist yet, which is fine
            }
        }

        Ok(())
    }

    fn migrate_mcp_server_table(&self) -> rusqlite::Result<()> {
        if let Ok(mut stmt) = self.conn.prepare("PRAGMA table_info(mcp_server)") {
            let mut has_headers = false;
            let mut has_is_deletable = false;
            let mut has_proxy_enabled = false;
            let cols = stmt.query_map([], |row| Ok(row.get::<_, String>(1)?))?;
            for c in cols {
                if let Ok(name) = c {
                    if name == "headers" {
                        has_headers = true;
                    }
                    if name == "is_deletable" {
                        has_is_deletable = true;
                    }
                    if name == "proxy_enabled" {
                        has_proxy_enabled = true;
                    }
                }
            }
            if !has_headers {
                let _ = self.conn.execute("ALTER TABLE mcp_server ADD COLUMN headers TEXT", []);
            }
            if !has_is_deletable {
                // 添加 is_deletable 字段，默认为 1（可删除）
                let _ = self.conn.execute(
                    "ALTER TABLE mcp_server ADD COLUMN is_deletable BOOLEAN NOT NULL DEFAULT 1",
                    [],
                );
            }
            if !has_proxy_enabled {
                // 添加 proxy_enabled 字段，默认为 0（不使用代理）
                let _ = self.conn.execute(
                    "ALTER TABLE mcp_server ADD COLUMN proxy_enabled BOOLEAN NOT NULL DEFAULT 0",
                    [],
                );
            }
        }
        Ok(())
    }

    #[instrument(level = "trace", skip(self))]
    pub fn get_mcp_servers(&self) -> rusqlite::Result<Vec<MCPServer>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, transport_type, command, environment_variables, headers, url, timeout, is_long_running, is_enabled, COALESCE(is_builtin, 0), COALESCE(is_deletable, 1), COALESCE(proxy_enabled, 0), created_time \
             FROM mcp_server ORDER BY created_time DESC"
        )?;

        let servers = stmt.query_map([], |row| {
            Ok(MCPServer {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                transport_type: row.get(3)?,
                command: row.get(4)?,
                environment_variables: row.get(5)?,
                headers: row.get(6)?,
                url: row.get(7)?,
                timeout: row.get(8)?,
                is_long_running: row.get(9)?,
                is_enabled: row.get(10)?,
                is_builtin: row.get(11)?,
                is_deletable: row.get(12)?,
                proxy_enabled: row.get(13)?,
                created_time: row.get(14)?,
            })
        })?;

        let mut result = Vec::new();
        for server in servers {
            result.push(server?);
        }
        Ok(result)
    }

    #[instrument(level = "trace", skip(self), fields(id))]
    pub fn get_mcp_server(&self, id: i64) -> rusqlite::Result<MCPServer> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, transport_type, command, environment_variables, headers, url, timeout, is_long_running, is_enabled, COALESCE(is_builtin, 0), COALESCE(is_deletable, 1), COALESCE(proxy_enabled, 0), created_time \
             FROM mcp_server WHERE id = ?"
        )?;

        let server = stmt
            .query_map([id], |row| {
                Ok(MCPServer {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    transport_type: row.get(3)?,
                    command: row.get(4)?,
                    environment_variables: row.get(5)?,
                    headers: row.get(6)?,
                    url: row.get(7)?,
                    timeout: row.get(8)?,
                    is_long_running: row.get(9)?,
                    is_enabled: row.get(10)?,
                    is_builtin: row.get(11)?,
                    is_deletable: row.get(12)?,
                    proxy_enabled: row.get(13)?,
                    created_time: row.get(14)?,
                })
            })?
            .next()
            .transpose()?;

        match server {
            Some(server) => Ok(server),
            None => Err(rusqlite::Error::QueryReturnedNoRows),
        }
    }

    /// 批量获取指定 ID 的服务器及其所有工具（不做启用过滤，调用方自行处理）
    pub fn get_mcp_servers_with_tools_by_ids(
        &self,
        server_ids: &[i64],
    ) -> rusqlite::Result<Vec<(MCPServer, Vec<MCPServerTool>)>> {
        if server_ids.is_empty() {
            return Ok(Vec::new());
        }

        // 构造占位符
        let placeholders = vec!["?"; server_ids.len()].join(",");
        let sql = format!(
            "SELECT id, name, description, transport_type, command, environment_variables, headers, url, timeout, is_long_running, is_enabled, COALESCE(is_builtin, 0), COALESCE(is_deletable, 1), COALESCE(proxy_enabled, 0), created_time \
             FROM mcp_server WHERE id IN ({})",
            placeholders
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let servers_iter =
            stmt.query_map(rusqlite::params_from_iter(server_ids.iter()), |row| {
                Ok(MCPServer {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    transport_type: row.get(3)?,
                    command: row.get(4)?,
                    environment_variables: row.get(5)?,
                    headers: row.get(6)?,
                    url: row.get(7)?,
                    timeout: row.get(8)?,
                    is_long_running: row.get(9)?,
                    is_enabled: row.get(10)?,
                    is_builtin: row.get(11)?,
                    is_deletable: row.get(12)?,
                    proxy_enabled: row.get(13)?,
                    created_time: row.get(14)?,
                })
            })?;

        let mut servers: Vec<MCPServer> = Vec::new();
        for s in servers_iter {
            servers.push(s?);
        }
        if servers.is_empty() {
            return Ok(Vec::new());
        }

        // 取所有 tool
        let placeholders_tools = vec!["?"; servers.len()].join(",");
        let tools_sql = format!(
            "SELECT id, server_id, tool_name, tool_description, is_enabled, is_auto_run, parameters \
             FROM mcp_server_tool WHERE server_id IN ({}) ORDER BY server_id, tool_name",
            placeholders_tools
        );
        let mut tool_stmt = self.conn.prepare(&tools_sql)?;
        let tools_iter = tool_stmt.query_map(
            rusqlite::params_from_iter(servers.iter().map(|s| s.id)),
            |row| {
                Ok(MCPServerTool {
                    id: row.get(0)?,
                    server_id: row.get(1)?,
                    tool_name: row.get(2)?,
                    tool_description: row.get(3)?,
                    is_enabled: row.get(4)?,
                    is_auto_run: row.get(5)?,
                    parameters: row.get(6)?,
                })
            },
        )?;

        use std::collections::HashMap;
        let mut tool_map: HashMap<i64, Vec<MCPServerTool>> = HashMap::new();
        for t in tools_iter {
            let tool = t?;
            tool_map.entry(tool.server_id).or_default().push(tool);
        }

        let mut result = Vec::new();
        for srv in servers {
            let tools = tool_map.remove(&srv.id).unwrap_or_default();
            result.push((srv, tools));
        }
        Ok(result)
    }

    pub fn update_mcp_server_with_builtin(
        &self,
        id: i64,
        name: &str,
        description: Option<&str>,
        transport_type: &str,
        command: Option<&str>,
        environment_variables: Option<&str>,
        headers: Option<&str>,
        url: Option<&str>,
        timeout: Option<i32>,
        is_long_running: bool,
        is_enabled: bool,
        is_builtin: bool,
        proxy_enabled: bool,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE mcp_server SET name = ?, description = ?, transport_type = ?, command = ?, environment_variables = ?, headers = ?, url = ?, timeout = ?, is_long_running = ?, is_enabled = ?, is_builtin = ?, proxy_enabled = ? WHERE id = ?",
            params![name, description, transport_type, command, environment_variables, headers, url, timeout, is_long_running, is_enabled, is_builtin, proxy_enabled, id],
        )?;
        Ok(())
    }

    pub fn delete_mcp_server(&self, id: i64) -> rusqlite::Result<()> {
        // Cascade delete will handle tools and resources
        self.conn.execute("DELETE FROM mcp_server WHERE id = ?", params![id])?;
        Ok(())
    }

    pub fn toggle_mcp_server(&self, id: i64, is_enabled: bool) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE mcp_server SET is_enabled = ? WHERE id = ?",
            params![is_enabled, id],
        )?;
        Ok(())
    }

    #[instrument(level = "trace", skip(self, description, command, environment_variables, headers, url), fields(name = name, transport_type = transport_type))]
    pub fn upsert_mcp_server_with_builtin(
        &self,
        name: &str,
        description: Option<&str>,
        transport_type: &str,
        command: Option<&str>,
        environment_variables: Option<&str>,
        headers: Option<&str>,
        url: Option<&str>,
        timeout: Option<i32>,
        is_long_running: bool,
        is_enabled: bool,
        is_builtin: bool,
        is_deletable: bool,
        proxy_enabled: bool,
    ) -> rusqlite::Result<i64> {
        // First try to get existing server by name
        let existing_id = self
            .conn
            .prepare("SELECT id FROM mcp_server WHERE name = ?")?
            .query_row([name], |row| row.get::<_, i64>(0))
            .optional()?;

        match existing_id {
            Some(id) => {
                // Update existing server (不更新 is_deletable，保持原值)
                self.conn.execute(
                    "UPDATE mcp_server SET description = ?, transport_type = ?, command = ?, \
                     environment_variables = ?, headers = ?, url = ?, timeout = ?, is_long_running = ?, is_enabled = ?, is_builtin = ?, proxy_enabled = ?
                     WHERE id = ?",
                    params![description, transport_type, command, environment_variables, headers, url, timeout, is_long_running, is_enabled, is_builtin, proxy_enabled, id],
                )?;
                Ok(id)
            }
            None => {
                // Insert new server
                let mut stmt = self.conn.prepare(
                    "INSERT INTO mcp_server (name, description, transport_type, command, environment_variables, headers, url, timeout, is_long_running, is_enabled, is_builtin, is_deletable, proxy_enabled)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )?;

                stmt.execute(params![
                    name,
                    description,
                    transport_type,
                    command,
                    environment_variables,
                    headers,
                    url,
                    timeout,
                    is_long_running,
                    is_enabled,
                    is_builtin,
                    is_deletable,
                    proxy_enabled
                ])?;

                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn get_mcp_server_tools(&self, server_id: i64) -> rusqlite::Result<Vec<MCPServerTool>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, server_id, tool_name, tool_description, is_enabled, is_auto_run, parameters 
             FROM mcp_server_tool WHERE server_id = ? ORDER BY tool_name"
        )?;

        let tools = stmt.query_map([server_id], |row| {
            Ok(MCPServerTool {
                id: row.get(0)?,
                server_id: row.get(1)?,
                tool_name: row.get(2)?,
                tool_description: row.get(3)?,
                is_enabled: row.get(4)?,
                is_auto_run: row.get(5)?,
                parameters: row.get(6)?,
            })
        })?;

        let mut result = Vec::new();
        for tool in tools {
            result.push(tool?);
        }
        Ok(result)
    }

    /// 删除指定服务器下所有、或指定名称集合之外的工具
    pub fn delete_mcp_server_tools_not_in(
        &self,
        server_id: i64,
        keep_names: &[String],
    ) -> rusqlite::Result<usize> {
        if keep_names.is_empty() {
            // 如果没有需要保留的工具，直接删除该服务器下所有工具
            let rows = self
                .conn
                .execute("DELETE FROM mcp_server_tool WHERE server_id = ?", params![server_id])?;
            return Ok(rows as usize);
        }

        // 构造 NOT IN (?, ?, ...) 语句
        let mut sql =
            String::from("DELETE FROM mcp_server_tool WHERE server_id = ? AND tool_name NOT IN (");
        for (i, _) in keep_names.iter().enumerate() {
            if i > 0 {
                sql.push_str(", ");
            }
            sql.push('?');
        }
        sql.push(')');

        let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + keep_names.len());
        params_vec.push(&server_id);
        for name in keep_names {
            params_vec.push(name);
        }

        let rows = self.conn.execute(&sql, params_vec.as_slice())?;
        Ok(rows as usize)
    }

    pub fn update_mcp_server_tool(
        &self,
        id: i64,
        is_enabled: bool,
        is_auto_run: bool,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE mcp_server_tool SET is_enabled = ?, is_auto_run = ? WHERE id = ?",
            params![is_enabled, is_auto_run, id],
        )?;
        Ok(())
    }

    #[instrument(
        level = "trace",
        skip(self, tool_description, parameters),
        fields(server_id, tool_name)
    )]
    pub fn upsert_mcp_server_tool(
        &self,
        server_id: i64,
        tool_name: &str,
        tool_description: Option<&str>,
        parameters: Option<&str>,
    ) -> rusqlite::Result<i64> {
        // First try to get existing tool by server_id and tool_name
        let existing_tool = self.conn.prepare(
            "SELECT id, is_enabled, is_auto_run FROM mcp_server_tool WHERE server_id = ? AND tool_name = ?"
        )?.query_row(params![server_id, tool_name], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?, row.get::<_, bool>(2)?))
        }).optional()?;

        match existing_tool {
            Some((id, _, _)) => {
                // Update existing tool, preserve user settings
                self.conn.execute(
                    "UPDATE mcp_server_tool SET tool_description = ?, parameters = ? WHERE id = ?",
                    params![tool_description, parameters, id],
                )?;
                Ok(id)
            }
            None => {
                // Insert new tool with default settings
                let mut stmt = self.conn.prepare(
                    "INSERT INTO mcp_server_tool (server_id, tool_name, tool_description, is_enabled, is_auto_run, parameters) 
                     VALUES (?, ?, ?, ?, ?, ?)"
                )?;

                stmt.execute(params![
                    server_id,
                    tool_name,
                    tool_description,
                    true,  // Default enabled
                    false, // Default not auto-run
                    parameters
                ])?;

                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn get_mcp_server_resources(
        &self,
        server_id: i64,
    ) -> rusqlite::Result<Vec<MCPServerResource>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, server_id, resource_uri, resource_name, resource_type, resource_description 
             FROM mcp_server_resource WHERE server_id = ? ORDER BY resource_name"
        )?;

        let resources = stmt.query_map([server_id], |row| {
            Ok(MCPServerResource {
                id: row.get(0)?,
                server_id: row.get(1)?,
                resource_uri: row.get(2)?,
                resource_name: row.get(3)?,
                resource_type: row.get(4)?,
                resource_description: row.get(5)?,
            })
        })?;

        let mut result = Vec::new();
        for resource in resources {
            result.push(resource?);
        }
        Ok(result)
    }

    /// 删除指定服务器下所有、或指定 URI 集合之外的资源
    pub fn delete_mcp_server_resources_not_in(
        &self,
        server_id: i64,
        keep_uris: &[String],
    ) -> rusqlite::Result<usize> {
        if keep_uris.is_empty() {
            let rows = self.conn.execute(
                "DELETE FROM mcp_server_resource WHERE server_id = ?",
                params![server_id],
            )?;
            return Ok(rows as usize);
        }

        let mut sql = String::from(
            "DELETE FROM mcp_server_resource WHERE server_id = ? AND resource_uri NOT IN (",
        );
        for (i, _) in keep_uris.iter().enumerate() {
            if i > 0 {
                sql.push_str(", ");
            }
            sql.push('?');
        }
        sql.push(')');

        let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + keep_uris.len());
        params_vec.push(&server_id);
        for uri in keep_uris {
            params_vec.push(uri);
        }

        let rows = self.conn.execute(&sql, params_vec.as_slice())?;
        Ok(rows as usize)
    }

    pub fn upsert_mcp_server_resource(
        &self,
        server_id: i64,
        resource_uri: &str,
        resource_name: &str,
        resource_type: &str,
        resource_description: Option<&str>,
    ) -> rusqlite::Result<i64> {
        // First try to get existing resource by server_id and resource_uri
        let existing_id = self
            .conn
            .prepare("SELECT id FROM mcp_server_resource WHERE server_id = ? AND resource_uri = ?")?
            .query_row(params![server_id, resource_uri], |row| row.get::<_, i64>(0))
            .optional()?;

        match existing_id {
            Some(id) => {
                // Update existing resource
                self.conn.execute(
                    "UPDATE mcp_server_resource SET resource_name = ?, resource_type = ?, resource_description = ? WHERE id = ?",
                    params![resource_name, resource_type, resource_description, id],
                )?;
                Ok(id)
            }
            None => {
                // Insert new resource
                let mut stmt = self.conn.prepare(
                    "INSERT INTO mcp_server_resource (server_id, resource_uri, resource_name, resource_type, resource_description) 
                     VALUES (?, ?, ?, ?, ?)"
                )?;

                stmt.execute(params![
                    server_id,
                    resource_uri,
                    resource_name,
                    resource_type,
                    resource_description
                ])?;

                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn get_mcp_server_prompts(&self, server_id: i64) -> rusqlite::Result<Vec<MCPServerPrompt>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, server_id, prompt_name, prompt_description, is_enabled, arguments 
             FROM mcp_server_prompt WHERE server_id = ? ORDER BY prompt_name",
        )?;

        let prompts = stmt.query_map([server_id], |row| {
            Ok(MCPServerPrompt {
                id: row.get(0)?,
                server_id: row.get(1)?,
                prompt_name: row.get(2)?,
                prompt_description: row.get(3)?,
                is_enabled: row.get(4)?,
                arguments: row.get(5)?,
            })
        })?;

        let mut result = Vec::new();
        for prompt in prompts {
            result.push(prompt?);
        }
        Ok(result)
    }

    /// 删除指定服务器下所有、或指定名称集合之外的提示
    pub fn delete_mcp_server_prompts_not_in(
        &self,
        server_id: i64,
        keep_names: &[String],
    ) -> rusqlite::Result<usize> {
        if keep_names.is_empty() {
            let rows = self
                .conn
                .execute("DELETE FROM mcp_server_prompt WHERE server_id = ?", params![server_id])?;
            return Ok(rows as usize);
        }

        let mut sql = String::from(
            "DELETE FROM mcp_server_prompt WHERE server_id = ? AND prompt_name NOT IN (",
        );
        for (i, _) in keep_names.iter().enumerate() {
            if i > 0 {
                sql.push_str(", ");
            }
            sql.push('?');
        }
        sql.push(')');

        let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + keep_names.len());
        params_vec.push(&server_id);
        for name in keep_names {
            params_vec.push(name);
        }

        let rows = self.conn.execute(&sql, params_vec.as_slice())?;
        Ok(rows as usize)
    }

    pub fn update_mcp_server_prompt(&self, id: i64, is_enabled: bool) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE mcp_server_prompt SET is_enabled = ? WHERE id = ?",
            params![is_enabled, id],
        )?;
        Ok(())
    }

    pub fn upsert_mcp_server_prompt(
        &self,
        server_id: i64,
        prompt_name: &str,
        prompt_description: Option<&str>,
        arguments: Option<&str>,
    ) -> rusqlite::Result<i64> {
        // First try to get existing prompt by server_id and prompt_name
        let existing_prompt = self.conn.prepare(
            "SELECT id, is_enabled FROM mcp_server_prompt WHERE server_id = ? AND prompt_name = ?"
        )?.query_row(params![server_id, prompt_name], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?))
        }).optional()?;

        match existing_prompt {
            Some((id, _is_enabled)) => {
                // Update existing prompt, preserve user settings
                self.conn.execute(
                    "UPDATE mcp_server_prompt SET prompt_description = ?, arguments = ? WHERE id = ?",
                    params![prompt_description, arguments, id],
                )?;
                Ok(id)
            }
            None => {
                // Insert new prompt with default settings
                let mut stmt = self.conn.prepare(
                    "INSERT INTO mcp_server_prompt (server_id, prompt_name, prompt_description, is_enabled, arguments) 
                     VALUES (?, ?, ?, ?, ?)"
                )?;

                stmt.execute(params![
                    server_id,
                    prompt_name,
                    prompt_description,
                    true, // Default enabled
                    arguments
                ])?;

                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    // MCP Tool Call methods
    #[instrument(
        level = "trace",
        skip(self, parameters),
        fields(conversation_id, server_id, tool_name)
    )]
    pub fn create_mcp_tool_call(
        &self,
        conversation_id: i64,
        message_id: Option<i64>,
        server_id: i64,
        server_name: &str,
        tool_name: &str,
        parameters: &str,
    ) -> rusqlite::Result<MCPToolCall> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO mcp_tool_call (conversation_id, message_id, server_id, server_name, tool_name, parameters)
             VALUES (?, ?, ?, ?, ?, ?)"
        )?;

        stmt.execute(params![
            conversation_id,
            message_id,
            server_id,
            server_name,
            tool_name,
            parameters
        ])?;

        let id = self.conn.last_insert_rowid();

        // Return the created tool call
        self.get_mcp_tool_call(id)
    }

    #[instrument(
        level = "trace",
        skip(self, parameters, llm_call_id),
        fields(conversation_id, server_id, tool_name)
    )]
    pub fn create_mcp_tool_call_with_llm_id(
        &self,
        conversation_id: i64,
        message_id: Option<i64>,
        server_id: i64,
        server_name: &str,
        tool_name: &str,
        parameters: &str,
        llm_call_id: Option<&str>,
        assistant_message_id: Option<i64>,
    ) -> rusqlite::Result<MCPToolCall> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO mcp_tool_call (conversation_id, message_id, server_id, server_name, tool_name, parameters, llm_call_id, assistant_message_id, subtask_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )?;

        stmt.execute(params![
            conversation_id,
            message_id,
            server_id,
            server_name,
            tool_name,
            parameters,
            llm_call_id,
            assistant_message_id,
            None::<i64> // Default subtask_id to None
        ])?;

        let id = self.conn.last_insert_rowid();

        // Return the created tool call
        self.get_mcp_tool_call(id)
    }

    /// Create MCP tool call specifically for subtask execution
    #[instrument(
        level = "trace",
        skip(self, parameters, llm_call_id),
        fields(conversation_id, server_id, tool_name, subtask_id)
    )]
    pub fn create_mcp_tool_call_for_subtask(
        &self,
        conversation_id: i64,
        subtask_id: i64,
        server_id: i64,
        server_name: &str,
        tool_name: &str,
        parameters: &str,
        llm_call_id: Option<&str>,
    ) -> rusqlite::Result<MCPToolCall> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO mcp_tool_call (conversation_id, message_id, server_id, server_name, tool_name, parameters, llm_call_id, assistant_message_id, subtask_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )?;

        stmt.execute(params![
            conversation_id,
            None::<i64>, // No specific message for subtask calls
            server_id,
            server_name,
            tool_name,
            parameters,
            llm_call_id,
            None::<i64>, // No assistant message for subtask calls
            subtask_id
        ])?;

        let id = self.conn.last_insert_rowid();

        // Return the created tool call
        self.get_mcp_tool_call(id)
    }

    pub fn get_mcp_tool_call(&self, id: i64) -> rusqlite::Result<MCPToolCall> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, message_id, server_id, server_name, tool_name, 
             parameters, status, result, error, created_time, started_time, finished_time, llm_call_id, assistant_message_id, subtask_id
             FROM mcp_tool_call WHERE id = ?"
        )?;

        stmt.query_row([id], |row| {
            Ok(MCPToolCall {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                message_id: row.get(2)?,
                subtask_id: row.get(15)?, // New field
                server_id: row.get(3)?,
                server_name: row.get(4)?,
                tool_name: row.get(5)?,
                parameters: row.get(6)?,
                status: row.get(7)?,
                result: row.get(8)?,
                error: row.get(9)?,
                created_time: row.get(10)?,
                started_time: row.get(11)?,
                finished_time: row.get(12)?,
                llm_call_id: row.get(13)?,
                assistant_message_id: row.get(14)?,
            })
        })
    }

    #[instrument(level = "trace", skip(self, result, error), fields(id, status))]
    pub fn update_mcp_tool_call_status(
        &self,
        id: i64,
        status: &str,
        result: Option<&str>,
        error: Option<&str>,
    ) -> rusqlite::Result<()> {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        match status {
            "executing" => {
                self.conn.execute(
                    "UPDATE mcp_tool_call SET status = ?, started_time = ? WHERE id = ?",
                    params![status, now, id],
                )?;
            }
            "success" | "failed" => {
                self.conn.execute(
                    "UPDATE mcp_tool_call SET status = ?, result = ?, error = ?, finished_time = ? WHERE id = ?",
                    params![status, result, error, now, id],
                )?;
            }
            _ => {
                self.conn.execute(
                    "UPDATE mcp_tool_call SET status = ? WHERE id = ?",
                    params![status, id],
                )?;
            }
        }
        Ok(())
    }

    #[instrument(level = "trace", skip(self, parameters), fields(id))]
    pub fn update_mcp_tool_call_metadata(
        &self,
        id: i64,
        server_name: &str,
        tool_name: &str,
        parameters: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE mcp_tool_call SET server_name = ?, tool_name = ?, parameters = ? WHERE id = ?",
            params![server_name, tool_name, parameters, id],
        )?;
        Ok(())
    }

    /// Try to transition a tool call to executing state only if it is currently pending/failed and not yet started.
    /// Returns true if the transition happened, false if another executor already took it.
    #[instrument(level = "trace", skip(self), fields(id))]
    pub fn mark_mcp_tool_call_executing_if_pending(&self, id: i64) -> rusqlite::Result<bool> {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        // 允许从 pending/failed 进入 executing；对于 failed 的重试，覆盖 started_time 即可
        let rows = self.conn.execute(
            "UPDATE mcp_tool_call SET status = 'executing', started_time = ? WHERE id = ? AND status IN ('pending', 'failed')",
            params![now, id],
        )?;
        Ok(rows > 0)
    }

    pub fn get_mcp_tool_calls_by_conversation(
        &self,
        conversation_id: i64,
    ) -> rusqlite::Result<Vec<MCPToolCall>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, message_id, server_id, server_name, tool_name, 
             parameters, status, result, error, created_time, started_time, finished_time, llm_call_id, assistant_message_id, subtask_id
             FROM mcp_tool_call WHERE conversation_id = ? ORDER BY created_time DESC"
        )?;

        let calls = stmt.query_map([conversation_id], |row| {
            Ok(MCPToolCall {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                message_id: row.get(2)?,
                subtask_id: row.get(15)?, // New field
                server_id: row.get(3)?,
                server_name: row.get(4)?,
                tool_name: row.get(5)?,
                parameters: row.get(6)?,
                status: row.get(7)?,
                result: row.get(8)?,
                error: row.get(9)?,
                created_time: row.get(10)?,
                started_time: row.get(11)?,
                finished_time: row.get(12)?,
                llm_call_id: row.get(13)?,
                assistant_message_id: row.get(14)?,
            })
        })?;

        let mut result = Vec::new();
        for call in calls {
            result.push(call?);
        }
        Ok(result)
    }

    /// Fetch MCP tool calls linked to a specific subtask execution
    pub fn get_mcp_tool_calls_by_subtask(
        &self,
        subtask_id: i64,
    ) -> rusqlite::Result<Vec<MCPToolCall>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, message_id, server_id, server_name, tool_name,
             parameters, status, result, error, created_time, started_time, finished_time,
             llm_call_id, assistant_message_id, subtask_id
             FROM mcp_tool_call WHERE subtask_id = ? ORDER BY created_time ASC",
        )?;

        let calls = stmt.query_map([subtask_id], |row| {
            Ok(MCPToolCall {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                message_id: row.get(2)?,
                subtask_id: row.get(15)?,
                server_id: row.get(3)?,
                server_name: row.get(4)?,
                tool_name: row.get(5)?,
                parameters: row.get(6)?,
                status: row.get(7)?,
                result: row.get(8)?,
                error: row.get(9)?,
                created_time: row.get(10)?,
                started_time: row.get(11)?,
                finished_time: row.get(12)?,
                llm_call_id: row.get(13)?,
                assistant_message_id: row.get(14)?,
            })
        })?;

        let mut result = Vec::new();
        for call in calls {
            result.push(call?);
        }
        Ok(result)
    }

    /// Fetch MCP tool calls linked to a specific message
    #[instrument(level = "trace", skip(self), fields(message_id))]
    pub fn get_mcp_tool_calls_by_message(
        &self,
        message_id: i64,
    ) -> rusqlite::Result<Vec<MCPToolCall>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, message_id, server_id, server_name, tool_name,
             parameters, status, result, error, created_time, started_time, finished_time,
             llm_call_id, assistant_message_id, subtask_id
             FROM mcp_tool_call WHERE message_id = ? ORDER BY id ASC",
        )?;

        let calls = stmt.query_map([message_id], |row| {
            Ok(MCPToolCall {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                message_id: row.get(2)?,
                subtask_id: row.get(15)?,
                server_id: row.get(3)?,
                server_name: row.get(4)?,
                tool_name: row.get(5)?,
                parameters: row.get(6)?,
                status: row.get(7)?,
                result: row.get(8)?,
                error: row.get(9)?,
                created_time: row.get(10)?,
                started_time: row.get(11)?,
                finished_time: row.get(12)?,
                llm_call_id: row.get(13)?,
                assistant_message_id: row.get(14)?,
            })
        })?;

        let mut result = Vec::new();
        for call in calls {
            result.push(call?);
        }
        Ok(result)
    }

    pub fn rebuild_dynamic_mcp_catalog(&self) -> rusqlite::Result<()> {
        let now = Self::now_string();
        self.conn.execute(
            "DELETE FROM mcp_server_capability_epoch_catalog WHERE server_id NOT IN (SELECT id FROM mcp_server)",
            [],
        )?;
        self.conn.execute(
            "DELETE FROM mcp_tool_catalog
             WHERE server_id NOT IN (SELECT id FROM mcp_server)
                OR tool_id NOT IN (SELECT id FROM mcp_server_tool)",
            [],
        )?;
        let mut server_stmt = self.conn.prepare(
            "SELECT id, name, description
             FROM mcp_server
             WHERE is_enabled = 1 OR command = 'aipp:dynamic_mcp'",
        )?;
        let servers: Vec<(i64, String, String)> = server_stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for (server_id, server_name, server_description) in servers {
            let existing_epoch = self
                .conn
                .prepare(
                    "SELECT epoch FROM mcp_server_capability_epoch_catalog WHERE server_id = ? LIMIT 1",
                )?
                .query_row(params![server_id], |row| row.get::<_, i64>(0))
                .optional()?
                .unwrap_or(1);

            let mut old_hash_stmt = self
                .conn
                .prepare("SELECT tool_id, schema_hash FROM mcp_tool_catalog WHERE server_id = ?")?;
            let old_hashes: std::collections::HashMap<i64, String> = old_hash_stmt
                .query_map(params![server_id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()?;

            let mut tool_stmt = self.conn.prepare(
                "SELECT id, tool_name, COALESCE(tool_description, ''), parameters
                 FROM mcp_server_tool
                 WHERE server_id = ?",
            )?;
            let tools: Vec<(i64, String, String, Option<String>)> = tool_stmt
                .query_map(params![server_id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let mut changed = false;
            let mut current_tool_ids = std::collections::HashSet::new();

            for (tool_id, _tool_name, _tool_summary, parameters) in &tools {
                current_tool_ids.insert(*tool_id);
                let new_hash = Self::schema_hash(parameters.as_deref());
                if old_hashes.get(tool_id).map(|h| h != &new_hash).unwrap_or(true) {
                    changed = true;
                }
            }

            if !changed {
                for old_id in old_hashes.keys() {
                    if !current_tool_ids.contains(old_id) {
                        changed = true;
                        break;
                    }
                }
            }

            let next_epoch = if changed { existing_epoch + 1 } else { existing_epoch };
            let summary = if server_description.trim().is_empty() {
                server_name.clone()
            } else {
                server_description.clone()
            };

            self.conn.execute(
                "INSERT INTO mcp_server_capability_epoch_catalog (
                    server_id, epoch, last_refresh_at, summary
                 ) VALUES (?, ?, ?, ?)
                 ON CONFLICT(server_id) DO UPDATE SET
                    epoch = excluded.epoch,
                    last_refresh_at = excluded.last_refresh_at,
                    summary = CASE
                        WHEN excluded.epoch != mcp_server_capability_epoch_catalog.epoch THEN excluded.summary
                        WHEN mcp_server_capability_epoch_catalog.summary_generated_at IS NOT NULL
                             AND trim(mcp_server_capability_epoch_catalog.summary) != '' THEN mcp_server_capability_epoch_catalog.summary
                        ELSE excluded.summary
                    END,
                    summary_generated_at = CASE
                        WHEN excluded.epoch != mcp_server_capability_epoch_catalog.epoch THEN NULL
                        ELSE mcp_server_capability_epoch_catalog.summary_generated_at
                    END",
                params![server_id, next_epoch, now, summary],
            )?;

            for (tool_id, tool_name, tool_summary, parameters) in tools.iter() {
                let schema_hash = Self::schema_hash(parameters.as_deref());
                let summary = if tool_summary.trim().is_empty() {
                    tool_name.clone()
                } else {
                    tool_summary.clone()
                };
                let keywords = serde_json::json!([server_name, tool_name]).to_string();
                self.conn.execute(
                    "INSERT INTO mcp_tool_catalog (
                        tool_id, server_id, tool_name, summary, keywords_json, schema_hash, capability_epoch, updated_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(tool_id) DO UPDATE SET
                        server_id = excluded.server_id,
                        tool_name = excluded.tool_name,
                        summary = CASE
                            WHEN excluded.capability_epoch != mcp_tool_catalog.capability_epoch THEN excluded.summary
                            WHEN mcp_tool_catalog.summary_generated_at IS NOT NULL
                                 AND trim(mcp_tool_catalog.summary) != '' THEN mcp_tool_catalog.summary
                            ELSE excluded.summary
                        END,
                        keywords_json = excluded.keywords_json,
                        schema_hash = excluded.schema_hash,
                        capability_epoch = excluded.capability_epoch,
                        updated_at = excluded.updated_at,
                        summary_generated_at = CASE
                            WHEN excluded.capability_epoch != mcp_tool_catalog.capability_epoch THEN NULL
                            ELSE mcp_tool_catalog.summary_generated_at
                        END",
                    params![
                        tool_id,
                        server_id,
                        tool_name,
                        summary,
                        keywords,
                        schema_hash,
                        next_epoch,
                        now
                    ],
                )?;
            }

            self.conn.execute(
                "DELETE FROM mcp_tool_catalog WHERE server_id = ? AND tool_id NOT IN (
                    SELECT id FROM mcp_server_tool WHERE server_id = ?
                 )",
                params![server_id, server_id],
            )?;
        }

        Ok(())
    }

    pub fn list_server_capability_catalog(&self) -> rusqlite::Result<Vec<MCPServerCapabilityEpochCatalog>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.server_id, s.name, c.epoch, c.last_refresh_at, c.summary, c.summary_generated_at
             FROM mcp_server_capability_epoch_catalog c
             JOIN mcp_server s ON s.id = c.server_id
             WHERE s.is_enabled = 1 OR s.command = 'aipp:dynamic_mcp'
             ORDER BY s.name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MCPServerCapabilityEpochCatalog {
                server_id: row.get(0)?,
                server_name: row.get(1)?,
                epoch: row.get(2)?,
                last_refresh_at: row.get(3)?,
                summary: row.get(4)?,
                summary_generated_at: row.get(5)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn list_tool_catalog(&self, server_id: Option<i64>) -> rusqlite::Result<Vec<MCPToolCatalogEntry>> {
        let sql = if server_id.is_some() {
            "SELECT c.tool_id, c.server_id, s.name, c.tool_name, c.summary, c.keywords_json, c.schema_hash,
                    c.capability_epoch, c.updated_at, c.summary_generated_at, s.is_enabled, t.is_enabled
             FROM mcp_tool_catalog c
             JOIN mcp_server s ON s.id = c.server_id
             JOIN mcp_server_tool t ON t.id = c.tool_id
             WHERE c.server_id = ?
             ORDER BY s.name, c.tool_name"
        } else {
            "SELECT c.tool_id, c.server_id, s.name, c.tool_name, c.summary, c.keywords_json, c.schema_hash,
                    c.capability_epoch, c.updated_at, c.summary_generated_at, s.is_enabled, t.is_enabled
             FROM mcp_tool_catalog c
             JOIN mcp_server s ON s.id = c.server_id
             JOIN mcp_server_tool t ON t.id = c.tool_id
             ORDER BY s.name, c.tool_name"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok(MCPToolCatalogEntry {
                tool_id: row.get(0)?,
                server_id: row.get(1)?,
                server_name: row.get(2)?,
                tool_name: row.get(3)?,
                summary: row.get(4)?,
                keywords_json: row.get(5)?,
                schema_hash: row.get(6)?,
                capability_epoch: row.get(7)?,
                updated_at: row.get(8)?,
                summary_generated_at: row.get(9)?,
                server_enabled: row.get(10)?,
                tool_enabled: row.get(11)?,
            })
        };
        let rows = if let Some(sid) = server_id {
            stmt.query_map(params![sid], mapper)?
        } else {
            stmt.query_map([], mapper)?
        };
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn update_server_catalog_summary(&self, server_id: i64, summary: &str) -> rusqlite::Result<()> {
        let now = Self::now_string();
        self.conn.execute(
            "UPDATE mcp_server_capability_epoch_catalog
             SET summary = ?, summary_generated_at = ?
             WHERE server_id = ?",
            params![summary, now, server_id],
        )?;
        Ok(())
    }

    pub fn update_tool_catalog_summary(&self, tool_id: i64, summary: &str) -> rusqlite::Result<()> {
        let now = Self::now_string();
        self.conn.execute(
            "UPDATE mcp_tool_catalog
             SET summary = ?, summary_generated_at = ?, updated_at = ?
             WHERE tool_id = ?",
            params![summary, now, now, tool_id],
        )?;
        Ok(())
    }

    pub fn upsert_conversation_loaded_tool(
        &self,
        conversation_id: i64,
        tool_id: i64,
        source: Option<&str>,
    ) -> rusqlite::Result<()> {
        let now = Self::now_string();
        let mut stmt = self.conn.prepare(
            "SELECT c.schema_hash, c.capability_epoch, s.name, t.tool_name
             FROM mcp_tool_catalog c
             JOIN mcp_server s ON s.id = c.server_id
             JOIN mcp_server_tool t ON t.id = c.tool_id
             WHERE c.tool_id = ?",
        )?;
        let row = stmt
            .query_row(params![tool_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .optional()?;

        let (schema_hash, capability_epoch, server_name, tool_name) = if let Some(v) = row {
            v
        } else {
            let mut fallback = self.conn.prepare(
                "SELECT s.name, t.tool_name, t.parameters
                 FROM mcp_server_tool t
                 JOIN mcp_server s ON s.id = t.server_id
                 WHERE t.id = ?",
            )?;
            let (server_name, tool_name, parameters): (String, String, Option<String>) =
                fallback.query_row(params![tool_id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?;
            (Self::schema_hash(parameters.as_deref()), 1, server_name, tool_name)
        };

        self.conn.execute(
            "INSERT INTO conversation_mcp_loaded_tool (
                conversation_id, tool_id, loaded_server_name, loaded_tool_name, loaded_schema_hash,
                loaded_epoch, status, invalid_reason, source, loaded_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, 'valid', NULL, ?, ?, ?)
             ON CONFLICT(conversation_id, tool_id) DO UPDATE SET
                loaded_server_name = excluded.loaded_server_name,
                loaded_tool_name = excluded.loaded_tool_name,
                loaded_schema_hash = excluded.loaded_schema_hash,
                loaded_epoch = excluded.loaded_epoch,
                status = 'valid',
                invalid_reason = NULL,
                source = excluded.source,
                updated_at = excluded.updated_at",
            params![
                conversation_id,
                tool_id,
                server_name,
                tool_name,
                schema_hash,
                capability_epoch,
                source,
                now,
                now
            ],
        )?;
        Ok(())
    }

    pub fn refresh_conversation_loaded_tool_statuses(&self, conversation_id: i64) -> rusqlite::Result<()> {
        let now = Self::now_string();
        let mut stmt = self.conn.prepare(
            "SELECT id, tool_id, loaded_schema_hash
             FROM conversation_mcp_loaded_tool
             WHERE conversation_id = ?",
        )?;
        let rows: Vec<(i64, i64, String)> = stmt
            .query_map(params![conversation_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for (id, tool_id, loaded_schema_hash) in rows {
            let tool_row = self
                .conn
                .prepare(
                    "SELECT s.name, t.tool_name, t.is_enabled, s.is_enabled, t.parameters
                     FROM mcp_server_tool t
                     JOIN mcp_server s ON s.id = t.server_id
                     WHERE t.id = ?",
                )?
                .query_row(params![tool_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, bool>(2)?,
                        row.get::<_, bool>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                })
                .optional()?;

            let (status, invalid_reason, server_name, tool_name) = match tool_row {
                None => (
                    "invalid_deleted".to_string(),
                    Some("Tool no longer exists".to_string()),
                    None,
                    None,
                ),
                Some((server_name, tool_name, tool_enabled, server_enabled, parameters)) => {
                    let current_hash = Self::schema_hash(parameters.as_deref());
                    if !server_enabled {
                        (
                            "invalid_server_disabled".to_string(),
                            Some("Server is disabled".to_string()),
                            Some(server_name),
                            Some(tool_name),
                        )
                    } else if !tool_enabled {
                        (
                            "invalid_disabled".to_string(),
                            Some("Tool is disabled".to_string()),
                            Some(server_name),
                            Some(tool_name),
                        )
                    } else if current_hash != loaded_schema_hash {
                        (
                            "invalid_changed".to_string(),
                            Some("Tool schema has changed".to_string()),
                            Some(server_name),
                            Some(tool_name),
                        )
                    } else {
                        ("valid".to_string(), None, Some(server_name), Some(tool_name))
                    }
                }
            };

            self.conn.execute(
                "UPDATE conversation_mcp_loaded_tool
                 SET status = ?, invalid_reason = ?, updated_at = ?,
                     loaded_server_name = COALESCE(?, loaded_server_name),
                     loaded_tool_name = COALESCE(?, loaded_tool_name)
                 WHERE id = ?",
                params![status, invalid_reason, now, server_name, tool_name, id],
            )?;
        }

        Ok(())
    }

    pub fn is_tool_loaded_for_conversation(
        &self,
        conversation_id: i64,
        server_id: i64,
        tool_name: &str,
    ) -> rusqlite::Result<bool> {
        let found = self
            .conn
            .prepare(
                "SELECT 1
                 FROM conversation_mcp_loaded_tool c
                 JOIN mcp_server_tool t ON t.id = c.tool_id
                 WHERE c.conversation_id = ?
                   AND c.status = 'valid'
                   AND t.server_id = ?
                   AND t.tool_name = ?
                 LIMIT 1",
            )?
            .query_row(params![conversation_id, server_id, tool_name], |row| row.get::<_, i64>(0))
            .optional()?;
        Ok(found.is_some())
    }

    pub fn get_valid_loaded_tools_for_conversation(
        &self,
        conversation_id: i64,
    ) -> rusqlite::Result<Vec<ConversationLoadedMCPToolResolved>> {
        self.refresh_conversation_loaded_tool_statuses(conversation_id)?;
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.conversation_id, c.tool_id, t.server_id, s.name, t.tool_name,
                    COALESCE(t.tool_description, ''), COALESCE(t.parameters, '{}')
             FROM conversation_mcp_loaded_tool c
             JOIN mcp_server_tool t ON t.id = c.tool_id
             JOIN mcp_server s ON s.id = t.server_id
             WHERE c.conversation_id = ?
               AND c.status = 'valid'
               AND s.is_enabled = 1
               AND t.is_enabled = 1
             ORDER BY c.loaded_at ASC",
        )?;
        let rows = stmt.query_map(params![conversation_id], |row| {
            Ok(ConversationLoadedMCPToolResolved {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                tool_id: row.get(2)?,
                server_id: row.get(3)?,
                server_name: row.get(4)?,
                tool_name: row.get(5)?,
                tool_description: row.get(6)?,
                parameters: row.get(7)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn get_conversation_loaded_mcp_tools(
        &self,
        conversation_id: i64,
    ) -> rusqlite::Result<Vec<ConversationLoadedMCPTool>> {
        self.refresh_conversation_loaded_tool_statuses(conversation_id)?;
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, tool_id, loaded_server_name, loaded_tool_name,
                    loaded_schema_hash, loaded_epoch, status, invalid_reason, source, loaded_at, updated_at
             FROM conversation_mcp_loaded_tool
             WHERE conversation_id = ?
             ORDER BY loaded_at ASC",
        )?;
        let rows = stmt.query_map(params![conversation_id], |row| {
            Ok(ConversationLoadedMCPTool {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                tool_id: row.get(2)?,
                loaded_server_name: row.get(3)?,
                loaded_tool_name: row.get(4)?,
                loaded_schema_hash: row.get(5)?,
                loaded_epoch: row.get(6)?,
                status: row.get(7)?,
                invalid_reason: row.get(8)?,
                source: row.get(9)?,
                loaded_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}
