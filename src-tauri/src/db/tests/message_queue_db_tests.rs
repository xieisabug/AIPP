use std::sync::{Arc, Mutex};

use crate::db::conversation_db::QueuedConversationMessageRepository;

use super::test_helpers::{build_test_conversation, create_test_db};

#[test]
fn test_mobile_queue_failure_pauses_until_explicit_retry() {
    let (repo, conversation_id) = create_queue_test_repo();
    let queued = repo.enqueue(conversation_id, "normal", "{}", "preserved input", 1).unwrap();
    repo.take_next_for_dispatch(conversation_id, false).unwrap().unwrap();
    repo.pause_dispatch(queued.id).unwrap().unwrap();
    assert!(repo.take_next_for_dispatch(conversation_id, false).unwrap().is_none());
    let pending = repo.list_queued_by_conversation(conversation_id).unwrap();
    assert_eq!(pending[0].status, "paused");
    assert_eq!(pending[0].prompt, "preserved input");
    repo.promote_to_interrupt(queued.id).unwrap().unwrap();
    assert_eq!(repo.take_next_for_dispatch(conversation_id, true).unwrap().unwrap().id, queued.id);
}

/// 管家内部续跑让用户新消息优先：排队/刚开始派发的用户消息算待处理，暂停与崩溃遗留不阻塞
#[test]
fn test_has_pending_user_turn_prioritizes_user_messages() {
    let (repo, conversation_id) = create_queue_test_repo();
    assert!(!repo.has_pending_user_turn(conversation_id).unwrap());

    let queued = repo.enqueue(conversation_id, "normal", "{}", "用户追问", 1).unwrap();
    assert!(repo.has_pending_user_turn(conversation_id).unwrap());

    repo.take_next_for_dispatch(conversation_id, false).unwrap().unwrap();
    assert!(repo.has_pending_user_turn(conversation_id).unwrap(), "刚开始派发仍算用户优先");

    repo.pause_dispatch(queued.id).unwrap().unwrap();
    assert!(!repo.has_pending_user_turn(conversation_id).unwrap(), "暂停的消息不阻塞续跑");
}

/// 手动“立即发送”可以越过队首直接认领某一条，也能取回暂停的消息；已被认领的不会再发一次
#[test]
fn test_take_for_dispatch_by_id_claims_only_waiting_message() {
    let (repo, conversation_id) = create_queue_test_repo();
    let first = repo.enqueue(conversation_id, "normal", "{}", "先排的", 1).unwrap();
    let stuck = repo.enqueue(conversation_id, "interrupt", "{}", "卡住的续接", 1).unwrap();
    assert_eq!(repo.get_pending(stuck.id).unwrap().unwrap().conversation_id, conversation_id);

    let claimed = repo.take_for_dispatch_by_id(stuck.id).unwrap().unwrap();
    assert_eq!((claimed.id, claimed.status.as_str()), (stuck.id, "dispatching"));
    assert!(repo.take_for_dispatch_by_id(stuck.id).unwrap().is_none(), "已认领的不能重复发送");
    assert!(repo.get_pending(stuck.id).unwrap().is_none());
    assert_eq!(repo.list_queued_by_conversation(conversation_id).unwrap()[0].id, first.id, "其余排队消息保持原样");

    repo.take_next_for_dispatch(conversation_id, false).unwrap().unwrap();
    repo.pause_dispatch(first.id).unwrap().unwrap();
    assert_eq!(repo.take_for_dispatch_by_id(first.id).unwrap().unwrap().status, "dispatching", "暂停的消息可手动发送");
}

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
