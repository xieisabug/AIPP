//! Explicit ownership cleanup shared by repositories and remote tombstones.
//! Existing profiles need no table rebuild and work with foreign_keys ON or OFF.
use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, Result};

fn diagnostic(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(message.into())))
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Only attach existing siblings of a recognized business DB. In-memory tests
/// can attach in-memory peers explicitly; offline recovery only sees its copies.
fn attach_related(conn: &Connection, table: &str) -> Result<()> {
    let databases: Vec<(String, String)> = conn.prepare("PRAGMA database_list")?
        .query_map([], |r| Ok((r.get(1)?, r.get(2)?)))?
        .collect::<Result<_>>()?;
    let Some((_, main)) = databases.iter().find(|(name, _)| name == "main") else { return Ok(()); };
    let main = Path::new(main);
    let known = ["assistant.db", "conversation.db", "mcp.db", "llm.db", "plugin.db", "artifacts.db"];
    if !main.file_name().and_then(|s| s.to_str()).is_some_and(|s| known.contains(&s)) {
        return Ok(());
    }
    let peers: &[&str] = match table {
        "assistant" => &["conversation", "plugin", "artifacts"],
        "conversation" | "message" => &["mcp", "plugin"],
        "mcp_server" | "mcp_server_tool" => &["assistant"],
        "llm_provider" => &["assistant", "conversation"],
        "llm_model" => &["conversation"],
        _ => &[],
    };
    let parent = main.parent().expect("business DB has a parent");
    for peer in peers {
        let path = parent.join(format!("{peer}.db"));
        let alias = format!("aipp_delete_{peer}");
        if databases.iter().any(|(_, file)| Path::new(file) == path) || !path.exists() {
            continue;
        }
        // Never create a missing DB as a side effect of deleting a record.
        conn.execute(&format!("ATTACH DATABASE ?1 AS {}", quote(&alias)), [path.to_string_lossy().as_ref()])?;
    }
    Ok(())
}

struct RollbackOnDrop<'a> {
    conn: &'a Connection,
    committed: bool,
}
impl Drop for RollbackOnDrop<'_> {
    fn drop(&mut self) {
        if !self.committed {
            if let Err(error) = self.conn.execute_batch("ROLLBACK TO aipp_cascade_delete; RELEASE aipp_cascade_delete;") {
                tracing::error!(%error, "Failed to roll back cascade deletion");
            }
        }
    }
}

/// Savepoints also work when an importer already owns the outer transaction.
fn atomic<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    conn.execute_batch("SAVEPOINT aipp_cascade_delete")?;
    let mut guard = RollbackOnDrop { conn, committed: false };
    match f() {
        Ok(value) => {
            conn.execute_batch("RELEASE aipp_cascade_delete")?;
            guard.committed = true;
            Ok(value)
        }
        Err(error) => {
            conn.execute_batch("ROLLBACK TO aipp_cascade_delete; RELEASE aipp_cascade_delete;")
                .map_err(|rollback| diagnostic(format!("{error}; rollback failed: {rollback}")))?;
            guard.committed = true;
            Err(error)
        }
    }
}

struct Tables<'a> {
    conn: &'a Connection,
    names: HashMap<String, String>,
    columns: HashMap<String, Vec<String>>,
}
impl<'a> Tables<'a> {
    fn new(conn: &'a Connection) -> Result<Self> {
        let schemas: Vec<String> = conn.prepare("PRAGMA database_list")?
            .query_map([], |r| r.get(1))?.collect::<Result<_>>()?;
        let mut result = Self { conn, names: HashMap::new(), columns: HashMap::new() };
        for schema in schemas.into_iter().filter(|s| s == "main" || s.starts_with("aipp_delete_")) {
            let tables: Vec<String> = conn.prepare(&format!("SELECT name FROM {}.sqlite_master WHERE type='table'", quote(&schema)))?
                .query_map([], |r| r.get(0))?.collect::<Result<_>>()?;
            for table in tables {
                let columns = conn.prepare(&format!("PRAGMA {}.table_info({})", quote(&schema), quote(&table)))?
                    .query_map([], |r| r.get(1))?.collect::<Result<Vec<String>>>()?;
                result.columns.entry(table.clone()).or_insert(columns);
                result.names.entry(table.clone()).or_insert_with(|| format!("{}.{}", quote(&schema), quote(&table)));
            }
        }
        Ok(result)
    }

    fn has(&self, table: &str, column: &str) -> bool {
        self.columns.get(table).is_some_and(|cols| cols.iter().any(|c| c == column))
    }

    fn remove(&self, table: &str, predicate: &str, id: i64) -> Result<()> {
        if let Some(name) = self.names.get(table) {
            self.conn.execute(&format!("DELETE FROM {name} WHERE {predicate}"), [id])
                .map_err(|e| diagnostic(format!("delete {table}: {e}")))?;
        }
        Ok(())
    }

    fn update(&self, table: &str, column: &str, value: &str, predicate: &str, id: i64) -> Result<()> {
        if self.has(table, column) {
            let name = &self.names[table];
            self.conn.execute(&format!("UPDATE {name} SET {}={value} WHERE {predicate}", quote(column)), [id])
                .map_err(|e| diagnostic(format!("clear {table}.{column}: {e}")))?;
        }
        Ok(())
    }

    fn ids(&self, table: &str, predicate: &str, id: i64) -> Result<Vec<i64>> {
        let Some(name) = self.names.get(table) else { return Ok(Vec::new()); };
        self.conn.prepare(&format!("SELECT id FROM {name} WHERE {predicate}"))?
            .query_map([id], |r| r.get(0))?.collect()
    }

    fn calls(&self, predicate: &str, id: i64) -> Result<()> {
        if let Some(calls) = self.names.get("mcp_tool_call") {
            self.remove("tool_review_log", &format!("mcp_tool_call_id IN (SELECT id FROM {calls} WHERE {predicate})"), id)?;
            self.remove("mcp_tool_call", predicate, id)?;
        }
        Ok(())
    }

    fn messages(&self, predicate: &str, id: i64) -> Result<()> {
        let Some(messages) = self.names.get("message") else { return Ok(()); };
        let ids = format!("SELECT id FROM {messages} WHERE {predicate}");
        for (table, column) in [("message_attachment", "message_id"),
            ("external_channel_message_link", "local_message_id"),
            ("external_channel_message_delivery", "local_message_id"), ("PluginHookAuditLog", "message_id")] {
            self.remove(table, &format!("{column} IN ({ids})"), id)?;
        }
        if self.names.contains_key("mcp_tool_call") {
            let mut references = Vec::new();
            for column in ["message_id", "assistant_message_id"] {
                if self.has("mcp_tool_call", column) { references.push(format!("{column} IN ({ids})")); }
            }
            if !references.is_empty() { self.calls(&references.join(" OR "), id)?; }
        }
        self.update("message", "parent_id", "NULL", &format!("parent_id IN ({ids})"), id)?;
        for column in ["handoff_message_id", "final_message_id"] {
            self.update("butler_task_result", column, "NULL", &format!("{column} IN ({ids})"), id)?;
        }
        // Relay cursors are watermarks, not ownership references. Retain them
        // when an individual message disappears to avoid replaying old replies.
        self.remove("message", predicate, id)
    }

    fn conversation(&self, id: i64) -> Result<()> {
        self.messages("conversation_id=?1", id)?;
        self.calls("conversation_id=?1", id)?;
        for table in ["tool_review_log", "conversation_mcp_loaded_tool", "PluginHookAuditLog",
            "external_channel_message_delivery", "external_channel_relay_scope", "external_channel_message_link",
            "acp_session", "queued_conversation_message", "conversation_summary", "conversation_todo"] {
            self.remove(table, "conversation_id=?1", id)?;
        }
        self.remove("butler_task_result", "task_conversation_id=?1", id)?;
        self.remove("butler_task_definition", "task_conversation_id=?1 OR butler_conversation_id=?1", id)?;
        self.remove("butler_main_state", "butler_conversation_id=?1", id)?;
        // Main conversations have no user-facing delete. A remote tombstone
        // detaches surviving children instead of recursively erasing history.
        self.update("conversation", "parent_butler_conversation_id", "NULL", "parent_butler_conversation_id=?1", id)?;
        self.update("scheduled_task", "butler_conversation_id", "NULL", "butler_conversation_id=?1", id)?;
        self.remove("conversation", "id=?1", id)
    }

    fn assistant(&self, id: i64) -> Result<()> {
        if let Some(tasks) = self.names.get("scheduled_task") {
            let tasks: Vec<String> = self.conn.prepare(&format!("SELECT id, name FROM {tasks} WHERE assistant_id=?1 ORDER BY id"))?
                .query_map([id], |r| Ok(format!("{} (ID: {})", r.get::<_, String>(1)?, r.get::<_, i64>(0)?)))?
                .collect::<Result<_>>()?;
            if !tasks.is_empty() {
                return Err(diagnostic(format!("无法删除助手 {id}：以下定时任务仍在使用，请先更换助手或删除任务：{}", tasks.join("、"))));
            }
        }
        if let Some(queue) = self.names.get("queued_conversation_message") {
            let pending: Vec<String> = self.conn.prepare(&format!("SELECT id, conversation_id FROM {queue} WHERE assistant_id=?1 ORDER BY id"))?
                .query_map([id], |r| Ok(format!("消息 {}（对话 {}）", r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<Result<_>>()?;
            if !pending.is_empty() {
                return Err(diagnostic(format!("无法删除助手 {id}：仍有排队或派发中的请求，请先处理：{}", pending.join("、"))));
            }
        }
        // Some legacy profiles do not have quick assistant 1 yet.
        let quick_exists = self.conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {} WHERE id=1 AND id!=?1)", self.names["assistant"]), [id], |r| r.get::<_, bool>(0))?;
        self.update("conversation", "assistant_id", if quick_exists { "1" } else { "NULL" }, "assistant_id=?1", id)?;
        self.update("artifacts_collection", "assistant_id", "NULL", "assistant_id=?1", id)?;
        self.remove("PluginAssistantConfigurations", "assistant_id=?1", id)?;
        for table in ["assistant_prompt_param", "assistant_model_config", "assistant_model", "assistant_prompt",
            "assistant_mcp_tool_config", "assistant_mcp_config", "assistant_skill_config", "assistant_workspace", "assistant_summary"] {
            self.remove(table, "assistant_id=?1", id)?;
        }
        self.remove("assistant", "id=?1", id)
    }

    fn tool(&self, id: i64, preserve_loaded: bool) -> Result<()> {
        self.remove("assistant_mcp_tool_config", "mcp_tool_id=?1", id)?;
        self.remove("mcp_tool_catalog", "tool_id=?1", id)?;
        if preserve_loaded {
            self.update("conversation_mcp_loaded_tool", "status", "'invalid_deleted'", "tool_id=?1", id)?;
            self.update("conversation_mcp_loaded_tool", "invalid_reason", "'Tool no longer exists'", "tool_id=?1", id)?;
        } else {
            self.remove("conversation_mcp_loaded_tool", "tool_id=?1", id)?;
        }
        self.remove("mcp_server_tool", "id=?1", id)
    }

    fn provider(&self, id: i64) -> Result<()> {
        // Preserve assistant model slots/configuration, with the existing
        // explicit unbound state. Never select a replacement provider/model.
        if let Some(models) = self.names.get("assistant_model") {
            self.conn.execute(&format!("UPDATE {models} SET provider_id=0, model_code='', alias='' WHERE provider_id=?1"), [id])?;
        }
        for model in self.ids("llm_model", "llm_provider_id=?1", id)? { self.model(model)?; }
        self.remove("llm_provider_config", "llm_provider_id=?1", id)?;
        self.remove("llm_model_request_mode_preference", "llm_provider_id=?1", id)?;
        self.remove("llm_provider", "id=?1", id)
    }

    fn model(&self, id: i64) -> Result<()> {
        if self.names.contains_key("llm_model_request_mode_preference") {
            let models = &self.names["llm_model"];
            self.remove("llm_model_request_mode_preference", &format!("(llm_provider_id, model_code) IN (SELECT llm_provider_id, code FROM {models} WHERE id=?1)"), id)?;
        }
        self.update("message", "llm_model_id", "NULL", "llm_model_id=?1", id)?;
        self.remove("llm_model", "id=?1", id)
    }

    fn delete(&self, table: &str, id: i64) -> Result<()> {
        match table {
            "assistant" => self.assistant(id),
            "conversation" => self.conversation(id),
            "message" => self.messages("id=?1", id),
            "assistant_prompt" => {
                self.remove("assistant_prompt_param", "assistant_prompt_id=?1", id)?;
                self.remove(table, "id=?1", id)
            }
            "assistant_model" => {
                self.remove("assistant_model_config", "assistant_model_id=?1", id)?;
                self.remove(table, "id=?1", id)
            }
            "llm_provider" => self.provider(id),
            "llm_model" => self.model(id),
            "mcp_server_tool" => self.tool(id, false),
            "mcp_server" => {
                // Refresh can retain snapshots for tools whose definitions are
                // already gone. Their captured server name still identifies
                // the deleted service (server names are unique).
                if self.has("conversation_mcp_loaded_tool", "loaded_server_name") && self.has("mcp_server", "name") {
                    self.remove("conversation_mcp_loaded_tool", &format!("loaded_server_name IN (SELECT name FROM {} WHERE id=?1)", self.names["mcp_server"]), id)?;
                }
                for tool in self.ids("mcp_server_tool", "server_id=?1", id)? { self.tool(tool, false)?; }
                self.calls("server_id=?1", id)?;
                self.remove("assistant_mcp_config", "mcp_server_id=?1", id)?;
                for child in ["mcp_server_resource", "mcp_server_prompt", "mcp_server_capability_epoch_catalog", "mcp_tool_catalog"] {
                    self.remove(child, "server_id=?1", id)?;
                }
                self.remove(table, "id=?1", id)
            }
            "Plugins" => {
                for child in ["PluginData", "PluginConfigurations", "PluginStatus", "PluginHookRegistration",
                    "PluginHookAuditLog", "PluginAssistantConfigurations", "PluginRuntime"] {
                    self.remove(child, "plugin_id=?1", id)?;
                }
                self.remove(table, "plugin_id=?1", id)
            }
            "scheduled_task" => {
                self.remove("scheduled_task_log", "task_id=?1", id)?;
                self.remove("scheduled_task_run", "task_id=?1", id)?;
                self.remove(table, "id=?1", id)
            }
            _ => self.remove(table, "id=?1", id),
        }
    }
}

pub(crate) fn delete(conn: &Connection, table: &str, id: i64) -> Result<()> {
    attach_related(conn, table).map_err(|e| diagnostic(format!("delete {table} id={id}: attach related databases: {e}")))?;
    atomic(conn, || {
        let tables = Tables::new(conn)?;
        if !tables.names.contains_key(table) { return Err(diagnostic(format!("missing table {table}"))); }
        tables.delete(table, id)
    }).map_err(|e| diagnostic(format!("delete {table} id={id}: {e}")))
}

/// Used for prompt/model replacement too: children must disappear before parents.
pub(crate) fn delete_by_owner(conn: &Connection, table: &str, column: &str, owner: i64) -> Result<()> {
    attach_related(conn, table)?;
    atomic(conn, || {
        let tables = Tables::new(conn)?;
        for id in tables.ids(table, &format!("{}=?1", quote(column)), owner)? { tables.delete(table, id)?; }
        Ok(())
    })
}

/// A refresh retains invalid loaded-tool snapshots so the UI can explain why
/// the capability disappeared. Explicit service/tool deletion removes them.
pub(crate) fn remove_missing_tools(conn: &Connection, server: i64, keep: &[String]) -> Result<usize> {
    attach_related(conn, "mcp_server_tool")?;
    atomic(conn, || {
        let tables = Tables::new(conn)?;
        let name = &tables.names["mcp_server_tool"];
        let rows: Vec<(i64, String)> = conn.prepare(&format!("SELECT id, tool_name FROM {name} WHERE server_id=?1"))?
            .query_map([server], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_>>()?;
        let mut removed = 0;
        for (id, name) in rows {
            if !keep.contains(&name) { tables.tool(id, true)?; removed += 1; }
        }
        Ok(removed)
    })
}

/// Older profiles may predate the request-mode preference table.
pub(crate) fn delete_model_preferences(conn: &Connection, provider: i64, code: Option<&str>) -> Result<()> {
    let tables = Tables::new(conn)?;
    if let Some(preferences) = tables.names.get("llm_model_request_mode_preference") {
        match code {
            Some(code) => conn.execute(&format!("DELETE FROM {preferences} WHERE llm_provider_id=?1 AND model_code=?2"), rusqlite::params![provider, code])?,
            None => conn.execute(&format!("DELETE FROM {preferences} WHERE llm_provider_id=?1"), [provider])?,
        };
    }
    Ok(())
}

/// A model-list replacement must retain preferences for the surviving codes.
pub(crate) fn prune_model_preferences(conn: &Connection, provider: i64) -> Result<()> {
    conn.execute("DELETE FROM llm_model_request_mode_preference WHERE llm_provider_id=?1 AND NOT EXISTS
        (SELECT 1 FROM llm_model WHERE llm_model.llm_provider_id=llm_model_request_mode_preference.llm_provider_id
         AND llm_model.code=llm_model_request_mode_preference.model_code)", [provider])?;
    Ok(())
}

/// Attach before opening the transaction, including model list replacements.
pub(crate) fn with_transaction<T>(conn: &Connection, table: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    attach_related(conn, table)?;
    atomic(conn, f)
}
