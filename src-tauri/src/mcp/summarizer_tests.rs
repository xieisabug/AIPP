use super::{read_progress, save_progress, MCPSummaryProgressPayload};
use crate::db::{connection::Connection, mcp_db::MCPDatabase};

#[test]
fn test_summary_progress_recovers_interrupted_and_completed_batch() {
    let db = MCPDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    assert!(read_progress(&db.conn, false).unwrap().is_none());
    let mut batch = MCPSummaryProgressPayload {
        phase: "progress".into(), total: 3, completed: 2, succeeded: 1, failed: 1,
        updated_at: Some("2026-11-15T01:01:01Z".into()),
        failures: vec!["Custom：模型请求超时".into()], server_name: None, message: None,
    };
    save_progress(&db.conn, &batch).unwrap();
    assert_eq!(read_progress(&db.conn, true).unwrap().unwrap().phase, "progress");
    let stopped = read_progress(&db.conn, false).unwrap().unwrap();
    assert_eq!(stopped.phase, "interrupted");
    assert_eq!(stopped.completed, 2);
    batch.phase = "completed".into();
    batch.completed = 3;
    batch.succeeded = 2;
    save_progress(&db.conn, &batch).unwrap();
    let restored = read_progress(&db.conn, false).unwrap().unwrap();
    assert_eq!(restored.phase, "completed");
    assert_eq!((restored.succeeded, restored.failed), (2, 1));
    assert_eq!(restored.failures, batch.failures);
    assert_eq!(restored.updated_at, batch.updated_at);
}
