//! Local, scoped Butler memory. Callers authorize the source/current turn separately.
//! Mutations require an existing transaction so authorization and writes share a fence.
use anyhow::{ensure, Context};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Memory {
    pub id: i64,
    pub content: String,
    pub scope_type: String,
    pub scope_id: Option<i64>,
    pub source_message_id: Option<i64>,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS butler_memory (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            content TEXT NOT NULL CHECK(length(trim(content)) BETWEEN 1 AND 2000),
            scope_type TEXT NOT NULL CHECK(scope_type IN ('global','assistant','group')),
            scope_id INTEGER,
            source_message_id INTEGER,
            revision INTEGER NOT NULL DEFAULT 1 CHECK(revision >= 1),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK((scope_type='global' AND scope_id IS NULL) OR
                  (scope_type IN ('assistant','group') AND scope_id IS NOT NULL AND scope_id > 0))
        );
        CREATE INDEX IF NOT EXISTS idx_butler_memory_scope
            ON butler_memory(scope_type,scope_id,updated_at DESC,id DESC);
        CREATE INDEX IF NOT EXISTS idx_butler_memory_source ON butler_memory(source_message_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_butler_memory_exact
            ON butler_memory(scope_type,COALESCE(scope_id,0),content);",
    )
}

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: row.get(0)?, content: row.get(1)?, scope_type: row.get(2)?,
        scope_id: row.get(3)?, source_message_id: row.get(4)?, revision: row.get(5)?,
        created_at: row.get(6)?, updated_at: row.get(7)?,
    })
}
const COLUMNS: &str = "id,content,scope_type,scope_id,source_message_id,revision,created_at,updated_at";
const VISIBLE: &str = "(scope_type='global' OR (scope_type='assistant' AND scope_id=?1))";

fn valid_actor(actor: i64) -> anyhow::Result<()> {
    ensure!(actor > 0, "记忆操作缺少有效助手身份");
    Ok(())
}
fn write_guard(conn: &Connection, actor: i64) -> anyhow::Result<()> {
    valid_actor(actor)?;
    ensure!(!conn.is_autocommit(), "记忆写入必须处于已校验当前回合的事务中");
    Ok(())
}
fn normalize(content: &str) -> anyhow::Result<String> {
    let content = content.trim().replace("\r\n", "\n");
    ensure!(!content.is_empty() && content.chars().count() <= 2000, "记忆内容必须为 1 至 2000 字");
    ensure!(!content.contains('\0'), "记忆内容不能包含空字符");
    Ok(content)
}

/// Scope is bound to actor here, never to a model-provided scope id.
pub fn remember(conn: &Connection, actor: i64, scope: &str, content: &str, source: i64) -> anyhow::Result<Memory> {
    write_guard(conn, actor)?;
    ensure!(matches!(scope, "global" | "assistant"), "记忆范围无效或群记忆尚未开放");
    ensure!(source > 0, "记忆缺少来源消息");
    let content = normalize(content)?;
    let scope_id = (scope == "assistant").then_some(actor);
    let existing = conn.query_row(
        &format!("SELECT {COLUMNS} FROM butler_memory WHERE scope_type=?1 AND scope_id IS ?2 AND content=?3"),
        params![scope, scope_id, content], row,
    ).optional()?;
    if let Some(memory) = existing { return Ok(memory); }
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT INTO butler_memory(content,scope_type,scope_id,source_message_id,revision,created_at,updated_at) VALUES(?1,?2,?3,?4,1,?5,?5)",
        params![content, scope, scope_id, source, now],
    )?;
    get(conn, actor, conn.last_insert_rowid())?.context("新记忆读取失败")
}

pub fn get(conn: &Connection, actor: i64, id: i64) -> anyhow::Result<Option<Memory>> {
    valid_actor(actor)?;
    Ok(conn.query_row(
        &format!("SELECT {COLUMNS} FROM butler_memory WHERE {VISIBLE} AND id=?2"),
        params![actor, id], row,
    ).optional()?)
}

/// Literal substring search; % and _ are not wildcards. No full-text cache is retained.
pub fn recall(conn: &Connection, actor: i64, query: &str) -> anyhow::Result<Vec<Memory>> {
    valid_actor(actor)?;
    let query = query.trim();
    ensure!(query.chars().count() <= 512, "记忆查询最多 512 字");
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM butler_memory WHERE {VISIBLE}
         AND (?2='' OR instr(lower(content),lower(?2))>0)
         ORDER BY updated_at DESC,id DESC LIMIT 10"
    ))?;
    let rows = stmt.query_map(params![actor,query], row)?;
    let mut output = Vec::new();
    let mut chars = 0;
    for memory in rows {
        let memory = memory?;
        let count = memory.content.chars().count();
        if chars + count > 6000 { break; }
        chars += count;
        output.push(memory);
    }
    Ok(output)
}

/// Does not change scope. The API separately checks user intent for global writes.
pub fn update(conn: &Connection, actor: i64, id: i64, revision: i64, content: &str, source: i64) -> anyhow::Result<Memory> {
    write_guard(conn, actor)?;
    ensure!(revision > 0 && source > 0, "记忆修改缺少有效版本或来源消息");
    let content = normalize(content)?;
    let old = get(conn, actor, id)?.context("记忆不存在或无权访问")?;
    ensure!(old.revision == revision, "记忆已被修改，请重新读取后再试");
    ensure!(revision < i64::MAX, "记忆版本已达到上限");
    let duplicate: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM butler_memory WHERE scope_type=?1 AND scope_id IS ?2 AND content=?3 AND id<>?4)",
        params![old.scope_type, old.scope_id, content, id], |r|r.get(0),
    )?;
    ensure!(!duplicate, "同一范围已存在相同记忆，请保留已有记录");
    let changed = conn.execute(
        &format!("UPDATE butler_memory SET content=?4,source_message_id=?5,revision=revision+1,updated_at=?6 WHERE {VISIBLE} AND id=?2 AND revision=?3"),
        params![actor,id,revision,content,source,chrono::Utc::now().timestamp_millis()],
    )?;
    ensure!(changed == 1, "记忆不存在或版本已改变，请重新读取");
    get(conn, actor, id)?.context("修改后的记忆读取失败")
}

/// False means absent or inaccessible, without revealing another assistant's record.
pub fn forget(conn: &Connection, actor: i64, id: i64, revision: i64) -> anyhow::Result<bool> {
    write_guard(conn, actor)?;
    ensure!(revision > 0, "删除记忆缺少有效版本");
    let Some(old) = get(conn, actor, id)? else { return Ok(false); };
    ensure!(old.revision == revision, "记忆已被修改，请重新读取后再删除");
    Ok(conn.execute(
        &format!("DELETE FROM butler_memory WHERE {VISIBLE} AND id=?2 AND revision=?3"),
        params![actor,id,revision],
    )? == 1)
}

#[cfg(test)]
#[path = "butler_memory_tests.rs"]
mod tests;
