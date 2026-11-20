use rusqlite::{params, Connection, OptionalExtension, Result};
use uuid::Uuid;

pub(crate) fn installed(data: &Connection) -> Result<bool> {
    data.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='conversation_identity_insert')", [], |row| row.get(0))
}

pub(crate) fn initialize(data: &Connection, legacy: Option<&Connection>) -> Result<()> {
    let transaction = data.unchecked_transaction()?;
    transaction.execute_batch("CREATE TABLE IF NOT EXISTS conversation_identity (
        conversation_id INTEGER PRIMARY KEY, uuid TEXT NOT NULL UNIQUE);
        CREATE TRIGGER IF NOT EXISTS conversation_identity_insert AFTER INSERT ON conversation BEGIN
          INSERT INTO conversation_identity VALUES (NEW.id, lower(hex(randomblob(4)) || '-' ||
            hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)),2) || '-' ||
            substr('89ab',1+(random() & 3),1) || substr(hex(randomblob(2)),2) || '-' || hex(randomblob(6))));
        END;
        CREATE TRIGGER IF NOT EXISTS conversation_identity_delete AFTER DELETE ON conversation BEGIN
          DELETE FROM conversation_identity WHERE conversation_id=OLD.id;
        END;")?;
    let has_map = match legacy {
        Some(sync) => sync.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sync_object_map')", [], |row| row.get(0))?,
        None => false,
    };
    let mut existing = legacy.filter(|_| has_map)
        .map(|sync| sync.prepare("SELECT sync_id FROM sync_object_map WHERE object_type='conversation' AND local_id=?1")).transpose()?;
    let ids = transaction.prepare("SELECT id FROM conversation WHERE id NOT IN (SELECT conversation_id FROM conversation_identity)")?
        .query_map([], |row| row.get::<_, i64>(0))?.collect::<Result<Vec<_>>>()?;
    for id in ids {
        let uuid: Option<String> = match existing.as_mut() {
            Some(query) => query.query_row([id], |row| row.get(0)).optional()?,
            None => None,
        };
        transaction.execute("INSERT INTO conversation_identity VALUES (?1,?2)",
            params![id, uuid.unwrap_or_else(|| Uuid::new_v4().to_string())])?;
    }
    transaction.commit()
}

pub(crate) fn get(data: &Connection, id: i64) -> Result<String> {
    data.query_row("SELECT uuid FROM conversation_identity WHERE conversation_id=?1", [id], |row| row.get(0))
}
