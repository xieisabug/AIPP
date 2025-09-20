use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use super::get_db_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServer {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub transport_type: String, // stdio, sse, http, builtin
    pub command: Option<String>,
    pub environment_variables: Option<String>,
    pub url: Option<String>,
    pub timeout: Option<i32>,
    pub is_long_running: bool,
    pub is_enabled: bool,
    pub is_builtin: bool, // 标识是否为内置服务器
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

pub struct MCPDatabase {
    pub conn: Connection,
}

impl MCPDatabase {
    #[instrument(level = "debug", skip(app_handle), err)]
    pub fn new(app_handle: &tauri::AppHandle) -> rusqlite::Result<Self> {
        let db_path = get_db_path(app_handle, "mcp.db");
        let conn = Connection::open(db_path.unwrap())?;
        Ok(MCPDatabase { conn })
    }

    #[instrument(level = "debug", skip(self), err)]
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
                url TEXT,
                timeout INTEGER DEFAULT 30000,
                is_long_running BOOLEAN NOT NULL DEFAULT 0,
                is_enabled BOOLEAN NOT NULL DEFAULT 1,
                is_builtin BOOLEAN NOT NULL DEFAULT 0,
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

        Ok(())
    }

    /// Migrate existing mcp_tool_call table to add new columns
    #[instrument(level = "debug", skip(self), err)]
    fn migrate_mcp_tool_call_table(&self) -> rusqlite::Result<()> {
        // Check if llm_call_id column exists
        let columns_result = self.conn.prepare("PRAGMA table_info(mcp_tool_call)");

        match columns_result {
            Ok(mut stmt) => {
                let column_info = stmt.query_map([], |row| {
                    Ok(row.get::<_, String>(1)?) // column name is at index 1
                })?;

                let mut has_llm_call_id = false;
                let mut has_assistant_message_id = false;

                for column in column_info {
                    match column {
                        Ok(name) => {
                            if name == "llm_call_id" {
                                has_llm_call_id = true;
                            } else if name == "assistant_message_id" {
                                has_assistant_message_id = true;
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
            }
            Err(_) => {
                // Table might not exist yet, which is fine
            }
        }

        Ok(())
    }

    pub fn get_mcp_servers(&self) -> rusqlite::Result<Vec<MCPServer>> {
        debug!("query mcp servers");
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, transport_type, command, environment_variables, url, timeout, is_long_running, is_enabled, COALESCE(is_builtin, 0), created_time 
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
                url: row.get(6)?,
                timeout: row.get(7)?,
                is_long_running: row.get(8)?,
                is_enabled: row.get(9)?,
                is_builtin: row.get(10)?,
                created_time: row.get(11)?,
            })
        })?;

        let mut result = Vec::new();
        for server in servers {
            result.push(server?);
        }
        Ok(result)
    }

    pub fn get_mcp_server(&self, id: i64) -> rusqlite::Result<MCPServer> {
        debug!(id, "query single mcp server");
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, transport_type, command, environment_variables, url, timeout, is_long_running, is_enabled, COALESCE(is_builtin, 0), created_time 
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
                    url: row.get(6)?,
                    timeout: row.get(7)?,
                    is_long_running: row.get(8)?,
                    is_enabled: row.get(9)?,
                    is_builtin: row.get(10)?,
                    created_time: row.get(11)?,
                })
            })?
            .next()
            .transpose()?;

        match server {
            Some(server) => Ok(server),
            None => Err(rusqlite::Error::QueryReturnedNoRows),
        }
    }

    pub fn update_mcp_server_with_builtin(
        &self,
        id: i64,
        name: &str,
        description: Option<&str>,
        transport_type: &str,
        command: Option<&str>,
        environment_variables: Option<&str>,
        url: Option<&str>,
        timeout: Option<i32>,
        is_long_running: bool,
        is_enabled: bool,
        is_builtin: bool,
    ) -> rusqlite::Result<()> {
        debug!(id, name, transport_type, is_enabled, is_builtin, "update mcp server");
        self.conn.execute(
            "UPDATE mcp_server SET name = ?, description = ?, transport_type = ?, command = ?, environment_variables = ?, url = ?, timeout = ?, is_long_running = ?, is_enabled = ?, is_builtin = ? WHERE id = ?",
            params![name, description, transport_type, command, environment_variables, url, timeout, is_long_running, is_enabled, is_builtin, id],
        )?;
        Ok(())
    }

    pub fn delete_mcp_server(&self, id: i64) -> rusqlite::Result<()> {
        debug!(id, "delete mcp server");
        // Cascade delete will handle tools and resources
        self.conn.execute("DELETE FROM mcp_server WHERE id = ?", params![id])?;
        Ok(())
    }

    pub fn toggle_mcp_server(&self, id: i64, is_enabled: bool) -> rusqlite::Result<()> {
        debug!(id, is_enabled, "toggle mcp server");
        self.conn.execute(
            "UPDATE mcp_server SET is_enabled = ? WHERE id = ?",
            params![is_enabled, id],
        )?;
        Ok(())
    }

    pub fn upsert_mcp_server_with_builtin(
        &self,
        name: &str,
        description: Option<&str>,
        transport_type: &str,
        command: Option<&str>,
        environment_variables: Option<&str>,
        url: Option<&str>,
        timeout: Option<i32>,
        is_long_running: bool,
        is_enabled: bool,
        is_builtin: bool,
    ) -> rusqlite::Result<i64> {
        debug!(name, transport_type, is_enabled, is_builtin, "upsert mcp server");
        // First try to get existing server by name
        let existing_id = self
            .conn
            .prepare("SELECT id FROM mcp_server WHERE name = ?")?
            .query_row([name], |row| row.get::<_, i64>(0))
            .optional()?;

        match existing_id {
            Some(id) => {
                // Update existing server
                self.conn.execute(
                    "UPDATE mcp_server SET description = ?, transport_type = ?, command = ?, 
                     environment_variables = ?, url = ?, timeout = ?, is_long_running = ?, is_enabled = ?, is_builtin = ?
                     WHERE id = ?",
                    params![description, transport_type, command, environment_variables, url, timeout, is_long_running, is_enabled, is_builtin, id],
                )?;
                Ok(id)
            }
            None => {
                // Insert new server
                let mut stmt = self.conn.prepare(
                    "INSERT INTO mcp_server (name, description, transport_type, command, environment_variables, url, timeout, is_long_running, is_enabled, is_builtin) 
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )?;

                stmt.execute(params![
                    name,
                    description,
                    transport_type,
                    command,
                    environment_variables,
                    url,
                    timeout,
                    is_long_running,
                    is_enabled,
                    is_builtin
                ])?;

                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn get_mcp_server_tools(&self, server_id: i64) -> rusqlite::Result<Vec<MCPServerTool>> {
        debug!(server_id, "query mcp server tools");
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

    pub fn update_mcp_server_tool(
        &self,
        id: i64,
        is_enabled: bool,
        is_auto_run: bool,
    ) -> rusqlite::Result<()> {
        debug!(id, is_enabled, is_auto_run, "update mcp server tool flags");
        self.conn.execute(
            "UPDATE mcp_server_tool SET is_enabled = ?, is_auto_run = ? WHERE id = ?",
            params![is_enabled, is_auto_run, id],
        )?;
        Ok(())
    }

    pub fn upsert_mcp_server_tool(
        &self,
        server_id: i64,
        tool_name: &str,
        tool_description: Option<&str>,
        parameters: Option<&str>,
    ) -> rusqlite::Result<i64> {
        debug!(server_id, tool_name, "upsert mcp server tool");
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
        debug!(server_id, "query mcp server resources");
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

    pub fn upsert_mcp_server_resource(
        &self,
        server_id: i64,
        resource_uri: &str,
        resource_name: &str,
        resource_type: &str,
        resource_description: Option<&str>,
    ) -> rusqlite::Result<i64> {
        debug!(server_id, resource_uri, "upsert mcp server resource");
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
        debug!(server_id, "query mcp server prompts");
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

    pub fn update_mcp_server_prompt(&self, id: i64, is_enabled: bool) -> rusqlite::Result<()> {
        debug!(id, is_enabled, "update mcp server prompt");
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
        debug!(server_id, prompt_name, "upsert mcp server prompt");
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
    pub fn create_mcp_tool_call(
        &self,
        conversation_id: i64,
        message_id: Option<i64>,
        server_id: i64,
        server_name: &str,
        tool_name: &str,
        parameters: &str,
    ) -> rusqlite::Result<MCPToolCall> {
        debug!(conversation_id, server_id, tool_name, "create mcp tool call");
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
        debug!(conversation_id, server_id, tool_name, llm_call_id = ?llm_call_id, assistant_message_id = ?assistant_message_id, "create mcp tool call with llm id");
        let mut stmt = self.conn.prepare(
            "INSERT INTO mcp_tool_call (conversation_id, message_id, server_id, server_name, tool_name, parameters, llm_call_id, assistant_message_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )?;

        stmt.execute(params![
            conversation_id,
            message_id,
            server_id,
            server_name,
            tool_name,
            parameters,
            llm_call_id,
            assistant_message_id
        ])?;

        let id = self.conn.last_insert_rowid();

        // Return the created tool call
        self.get_mcp_tool_call(id)
    }

    pub fn get_mcp_tool_call(&self, id: i64) -> rusqlite::Result<MCPToolCall> {
        debug!(id, "get mcp tool call");
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, message_id, server_id, server_name, tool_name, 
             parameters, status, result, error, created_time, started_time, finished_time, llm_call_id, assistant_message_id
             FROM mcp_tool_call WHERE id = ?"
        )?;

        stmt.query_row([id], |row| {
            Ok(MCPToolCall {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                message_id: row.get(2)?,
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

    pub fn update_mcp_tool_call_status(
        &self,
        id: i64,
        status: &str,
        result: Option<&str>,
        error: Option<&str>,
    ) -> rusqlite::Result<()> {
        debug!(id, status, has_result = result.is_some(), has_error = error.is_some(), "update mcp tool call status");
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

    /// Try to transition a tool call to executing state only if it is currently pending/failed and not yet started.
    /// Returns true if the transition happened, false if another executor already took it.
    pub fn mark_mcp_tool_call_executing_if_pending(&self, id: i64) -> rusqlite::Result<bool> {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        // 允许从 pending/failed 进入 executing；对于 failed 的重试，覆盖 started_time 即可
        let rows = self.conn.execute(
            "UPDATE mcp_tool_call SET status = 'executing', started_time = ? WHERE id = ? AND status IN ('pending', 'failed')",
            params![now, id],
        )?;
        debug!(id, transitioned = rows > 0, "try mark executing");
        Ok(rows > 0)
    }

    pub fn get_mcp_tool_calls_by_conversation(
        &self,
        conversation_id: i64,
    ) -> rusqlite::Result<Vec<MCPToolCall>> {
        debug!(conversation_id, "query tool calls by conversation");
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, message_id, server_id, server_name, tool_name, 
             parameters, status, result, error, created_time, started_time, finished_time, llm_call_id, assistant_message_id
             FROM mcp_tool_call WHERE conversation_id = ? ORDER BY created_time DESC"
        )?;

        let calls = stmt.query_map([conversation_id], |row| {
            Ok(MCPToolCall {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                message_id: row.get(2)?,
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
}
