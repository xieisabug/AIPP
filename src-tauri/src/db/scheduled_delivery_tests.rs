use super::*;
#[path = "tests/test_helpers.rs"]
mod test_helpers;
use test_helpers::{create_test_db, create_test_message};
use crate::db::butler_goal;

fn fixture() -> (MessageRepository, Message) {
    let conn = create_test_db();
    conn.execute_batch("INSERT INTO conversation(id,name,conversation_kind,created_time)
        VALUES(1,'Butler','butler_main',CURRENT_TIMESTAMP),(2,'Run','scheduled_run',CURRENT_TIMESTAMP);
        CREATE TABLE scheduled_task_run(task_id INTEGER,run_id TEXT,conversation_id INTEGER);
        INSERT INTO scheduled_task_run VALUES(7,'run-7',2);
        INSERT INTO message(id,conversation_id,message_type,content,created_time)
        VALUES(10,1,'user','current work',CURRENT_TIMESTAMP),(20,2,'response','scheduled result',CURRENT_TIMESTAMP);").unwrap();
    let goal = butler_goal::apply(&conn, 1, "replace", "current work").unwrap().unwrap();
    butler_goal::bind_turn(&conn, 1, 10, goal.revision).unwrap();
    let repo = MessageRepository::new(conn);
    let mut trigger = create_test_message(1, "user", "scheduled trigger", None, None);
    trigger.metadata_json = Some(serde_json::json!({"origin":"scheduled_task","role":"trigger",
        "scheduled_task_id":7,"scheduled_run_id":"run-7","run_conversation_id":2}).to_string());
    let trigger = repo.create(&trigger).unwrap();
    let mut result = create_test_message(1, "response", "scheduled result", Some(trigger.id), None);
    result.metadata_json = Some(serde_json::json!({"origin":"scheduled_task","role":"result",
        "scheduled_task_id":7,"scheduled_run_id":"run-7","run_conversation_id":2,"final_message_id":20}).to_string());
    (repo, result)
}

#[tokio::test]
async fn test_scheduled_delivery_preserves_goal_and_conversation_continuation() {
    let (repo, result) = fixture();
    let before = butler_goal::current(&repo.conn, 1).unwrap();
    let delivered = butler_goal::OUTPUT_SOURCE.scope(Some(10), async {
        repo.create_scheduled_delivery(&result)
    }).await.unwrap();
    assert_eq!(delivered.parent_id, result.parent_id);
    assert_eq!(butler_goal::current(&repo.conn, 1).unwrap(), before);
    assert_eq!(butler_goal::message_revision(&repo.conn, 1, delivered.id).unwrap(), None);
    assert!(!delivered.metadata_json.unwrap().contains("goal_superseded"));
    // The already running reply still belongs to the original user turn.
    repo.create(&create_test_message(1, "response", "current work result", Some(10), None)).unwrap();
    // A later natural user message can continue the same goal normally.
    let next = repo.create(&create_test_message(1, "user", "explain that result", None, None)).unwrap();
    let goal = butler_goal::accept_natural_message(&repo.conn, 1, &next.content).unwrap().unwrap();
    butler_goal::bind_turn(&repo.conn, 1, next.id, goal.revision).unwrap();
    repo.create(&create_test_message(1, "response", "explanation", Some(next.id), None)).unwrap();
    assert_eq!(butler_goal::current(&repo.conn, 1).unwrap(), before);
}

#[test]
fn test_scheduled_delivery_rejects_invalid_provenance_and_keeps_normal_guard() {
    let (repo, result) = fixture();
    assert!(repo.create(&result).is_err()); // Metadata cannot bypass ordinary execution guards.
    for field in ["scheduled_task_id", "scheduled_run_id", "run_conversation_id", "final_message_id"] {
        let mut invalid = result.clone();
        let mut metadata: serde_json::Value = serde_json::from_str(invalid.metadata_json.as_deref().unwrap()).unwrap();
        metadata[field] = serde_json::json!(999);
        invalid.metadata_json = Some(metadata.to_string());
        assert!(repo.create_scheduled_delivery(&invalid).is_err(), "{field}");
    }
    let mut invalid = result.clone();
    invalid.parent_id = Some(10);
    assert!(repo.create_scheduled_delivery(&invalid).is_err());
    invalid = result.clone();
    invalid.conversation_id = 2;
    assert!(repo.create_scheduled_delivery(&invalid).is_err());
    butler_goal::apply(&repo.conn, 1, "stop", "").unwrap();
    let stopped = butler_goal::current(&repo.conn, 1).unwrap();
    repo.create_scheduled_delivery(&result).unwrap();
    assert_eq!(butler_goal::current(&repo.conn, 1).unwrap(), stopped);
    assert!(repo.create(&create_test_message(1, "response", "late", Some(10), None)).is_err());
}
