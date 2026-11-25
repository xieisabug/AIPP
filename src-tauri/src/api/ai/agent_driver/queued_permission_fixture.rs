use crate::db::conversation_db::{ConversationDatabase, QueuedConversationMessage};
use rusqlite::Connection;

pub(crate) struct Queue {
    pub db: ConversationDatabase,
    pub conn: Connection,
    pub cid: i64,
}

impl Queue {
    pub fn new() -> Self {
        let (db, conn) = ConversationDatabase::shared_in_memory();
        db.create_tables().unwrap();
        conn.execute("INSERT INTO conversation (name,created_time,updated_time) VALUES ('test',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)", []).unwrap();
        let cid = conn.last_insert_rowid();
        Self { db, conn, cid }
    }

    pub fn enqueue(&self, json: &str) -> i64 {
        self.db.queued_message_repo().unwrap().enqueue(self.cid, "normal", json, "second", 1).unwrap().id
    }

    pub fn dequeue(&self, id: i64) -> QueuedConversationMessage {
        let row = self.db.queued_message_repo().unwrap().take_next_for_dispatch(self.cid, false).unwrap().unwrap();
        assert_eq!(row.id, id);
        row
    }
}
