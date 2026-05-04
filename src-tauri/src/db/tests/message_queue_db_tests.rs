use std::sync::{Arc, Mutex};

use crate::db::conversation_db::QueuedConversationMessageRepository;

use super::test_helpers::{build_test_conversation, create_test_db};

fn create_queue_test_repo() -> (QueuedConversationMessageRepository, i64) {
    let conn = create_test_db();
    let conversation = build_test_conversation("queue test", Some(1));
    let conversation_id = conn
        .execute(
            "INSERT INTO conversation (name, assistant_id, created_time, updated_time)
             VALUES (?1, ?2, ?3, ?4)",
            (
                conversation.name,
                conversation.assistant_id,
                conversation.created_time,
                conversation.updated_time,
            ),
        )
        .map(|_| conn.last_insert_rowid())
        .unwrap();

    (
        QueuedConversationMessageRepository::new_with_write_lock(
            conn,
            Arc::new(Mutex::new(())),
        ),
        conversation_id,
    )
}

/// 测试普通排队消息可以提升为打断消息，并按打断优先级出队。
///
/// 验证内容：
/// - Create: 入队后能在 pending 列表读取
/// - Update: 普通队列消息可以提升为 interrupt
/// - Dispatch: interrupt_only 只取打断消息，并将状态改为 dispatching
/// - Reset/Delete: 失败可重置，成功后可删除
#[test]
fn test_queued_message_promote_and_dispatch_lifecycle() {
    let (repo, conversation_id) = create_queue_test_repo();
    let request_json = r#"{"conversation_id":"1","assistant_id":1,"prompt":"next"}"#;

    let queued = repo
        .enqueue(conversation_id, "normal", request_json, "next", 1)
        .unwrap();
    assert_eq!(queued.queue_kind, "normal");

    let listed = repo.list_queued_by_conversation(conversation_id).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].prompt, "next");

    let promoted = repo.promote_to_interrupt(queued.id).unwrap().unwrap();
    assert_eq!(promoted.queue_kind, "interrupt");

    let dispatching = repo.take_next_for_dispatch(conversation_id, true).unwrap().unwrap();
    assert_eq!(dispatching.id, queued.id);
    assert_eq!(dispatching.status, "dispatching");
    assert!(repo.take_next_for_dispatch(conversation_id, true).unwrap().is_none());

    let reset = repo.reset_dispatch(queued.id).unwrap().unwrap();
    assert_eq!(reset.status, "queued");

    let dispatching = repo.take_next_for_dispatch(conversation_id, false).unwrap().unwrap();
    repo.finish_dispatch(dispatching.id).unwrap();
    assert!(repo.list_queued_by_conversation(conversation_id).unwrap().is_empty());
}
