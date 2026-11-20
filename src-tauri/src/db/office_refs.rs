//! Office 文件引用：用户把某个文件的选区交给助手、或 `office_edit` 生成副本时登记。
//! 助手只能通过引用号使用这些文件，不能凭路径访问任意文件；引用号绑定会话，
//! 用户在新会话里发出的选区（发送时还没有会话 id）在第一次使用时绑定到使用它的会话。
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OfficeFileRef {
    pub id: String,
    pub conversation_id: Option<i64>,
    /// `selection`（用户交给助手的文件）或 `output`（office_edit 生成的副本）。
    pub kind: String,
    pub file_path: String,
    pub file_name: String,
    pub format: String,
    /// 登记时的文件指纹；用户文件之后被改动时据此判定引用已过期。
    pub sha256: String,
    pub size_bytes: i64,
    /// 产物由哪个引用修改而来。
    pub origin_ref: Option<String>,
    /// 用户原始文件路径（另存时默认放在它旁边）。
    pub origin_path: Option<String>,
    pub created_at: i64,
}

pub const KIND_SELECTION: &str = "selection";
pub const KIND_OUTPUT: &str = "output";

fn schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS office_file_ref (
            id TEXT PRIMARY KEY,
            conversation_id INTEGER REFERENCES conversation(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            file_path TEXT NOT NULL,
            file_name TEXT NOT NULL,
            format TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            origin_ref TEXT,
            origin_path TEXT,
            created_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_office_file_ref_conversation ON office_file_ref(conversation_id);",
    )?;
    Ok(())
}

pub fn new_ref_id() -> String {
    format!("ofr_{}", &uuid::Uuid::new_v4().simple().to_string()[..16])
}

pub fn insert(conn: &Connection, r: &OfficeFileRef) -> Result<()> {
    schema(conn)?;
    conn.execute(
        "INSERT INTO office_file_ref (id, conversation_id, kind, file_path, file_name, format, sha256, size_bytes, origin_ref, origin_path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            r.id,
            r.conversation_id,
            r.kind,
            r.file_path,
            r.file_name,
            r.format,
            r.sha256,
            r.size_bytes,
            r.origin_ref,
            r.origin_path,
            r.created_at
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<OfficeFileRef>> {
    schema(conn)?;
    Ok(conn
        .query_row(
            "SELECT id, conversation_id, kind, file_path, file_name, format, sha256, size_bytes, origin_ref, origin_path, created_at
             FROM office_file_ref WHERE id = ?1",
            [id],
            |row| {
                Ok(OfficeFileRef {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    kind: row.get(2)?,
                    file_path: row.get(3)?,
                    file_name: row.get(4)?,
                    format: row.get(5)?,
                    sha256: row.get(6)?,
                    size_bytes: row.get(7)?,
                    origin_ref: row.get(8)?,
                    origin_path: row.get(9)?,
                    created_at: row.get(10)?,
                })
            },
        )
        .optional()?)
}

/// 助手在 `conversation_id` 里使用引用：只能用本会话的引用；还没绑定会话的选区在第一次使用时绑定。
/// 其他会话的引用按“不存在”处理，不透露它属于谁。
pub fn resolve_for_conversation(
    conn: &Connection,
    id: &str,
    conversation_id: Option<i64>,
) -> Result<Option<OfficeFileRef>> {
    let Some(mut found) = get(conn, id)? else { return Ok(None) };
    match (found.conversation_id, conversation_id) {
        (Some(owner), Some(current)) if owner == current => Ok(Some(found)),
        (None, Some(current)) => {
            let bound = conn.execute(
                "UPDATE office_file_ref SET conversation_id = ?1 WHERE id = ?2 AND conversation_id IS NULL",
                params![current, id],
            )?;
            if bound == 1 {
                found.conversation_id = Some(current);
                return Ok(Some(found));
            }
            // 并发绑定到了别的会话：重新读取再判断
            Ok(get(conn, id)?.filter(|r| r.conversation_id == Some(current)))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
#[path = "office_refs_tests.rs"]
mod tests;
