use super::*;

fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init(&conn).unwrap();
    conn
}

fn add(conn: &mut Connection, actor: i64, scope: &str, text: &str) -> Memory {
    let tx = conn.transaction().unwrap();
    let value = remember(&tx, actor, scope, text, 1).unwrap();
    tx.commit().unwrap();
    value
}

#[test]
fn test_memory_init_idempotent_keeps_records() {
    let mut conn = db();
    let value = add(&mut conn, 1, "assistant", "回答简短");
    init(&conn).unwrap();
    assert_eq!(get(&conn, 1, value.id).unwrap().unwrap(), value);
}

#[test]
fn test_memory_scope_constraints() {
    let conn = db();
    for sql in [
        "INSERT INTO butler_memory(content,scope_type,scope_id,created_at,updated_at) VALUES('a','global',1,0,0)",
        "INSERT INTO butler_memory(content,scope_type,scope_id,created_at,updated_at) VALUES('a','assistant',NULL,0,0)",
        "INSERT INTO butler_memory(content,scope_type,scope_id,created_at,updated_at) VALUES('a','unknown',1,0,0)",
        "INSERT INTO butler_memory(content,scope_type,scope_id,created_at,updated_at) VALUES('','global',NULL,0,0)",
    ] { assert!(conn.execute(sql, []).is_err()); }
}

#[test]
fn test_memory_requires_transaction_and_identity() {
    let mut conn = db();
    assert!(remember(&conn, 1, "assistant", "偏好", 1).is_err());
    let tx = conn.transaction().unwrap();
    assert!(remember(&tx, 0, "assistant", "偏好", 1).is_err());
    assert!(get(&tx, 0, 1).is_err());
    assert!(recall(&tx, 0, "").is_err());
}

#[test]
fn test_memory_validates_content_scope_source() {
    let mut conn = db();
    let tx = conn.transaction().unwrap();
    for text in ["   ".to_owned(), "字".repeat(2001), "a\0b".to_owned()] {
        assert!(remember(&tx, 1, "assistant", &text, 1).is_err());
    }
    assert!(remember(&tx, 1, "group", "群内容", 1).is_err());
    assert!(remember(&tx, 1, "unknown", "内容", 1).is_err());
    assert!(remember(&tx, 1, "assistant", "内容", 0).is_err());
    assert!(remember(&tx, 1, "assistant", &"字".repeat(2000), 1).is_ok());
}

#[test]
fn test_memory_isolates_assistants_and_reads_global() {
    let mut conn = db();
    let a = add(&mut conn, 1, "assistant", "助手一的偏好");
    let b = add(&mut conn, 2, "assistant", "助手二的偏好");
    let global = add(&mut conn, 1, "global", "全部助手使用中文");
    assert!(get(&conn, 2, a.id).unwrap().is_none());
    assert!(get(&conn, 1, b.id).unwrap().is_none());
    assert!(get(&conn, 2, global.id).unwrap().is_some());
    let found = recall(&conn, 1, "").unwrap();
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|m| m.id != b.id));
}

#[test]
fn test_memory_foreign_update_and_delete_do_not_leak_or_mutate() {
    let mut conn = db();
    let own = add(&mut conn, 1, "assistant", "私有内容");
    let tx = conn.transaction().unwrap();
    assert!(update(&tx, 2, own.id, 1, "攻击", 2).is_err());
    assert!(!forget(&tx, 2, own.id, 1).unwrap());
    assert_eq!(get(&tx, 1, own.id).unwrap().unwrap().content, "私有内容");
}

#[test]
fn test_memory_deduplicates_only_same_scope() {
    let mut conn = db();
    let first = add(&mut conn, 1, "assistant", "  中文偏好\r\n简短  ");
    let duplicate = add(&mut conn, 1, "assistant", "中文偏好\n简短");
    assert_eq!(first.id, duplicate.id);
    assert_eq!(duplicate.revision, 1);
    assert_ne!(first.id, add(&mut conn, 2, "assistant", "中文偏好\n简短").id);
    let global = add(&mut conn, 1, "global", "中文偏好\n简短");
    assert_eq!(global.id, add(&mut conn, 2, "global", "中文偏好\n简短").id);
}

#[test]
fn test_memory_uses_literal_chinese_search() {
    let mut conn = db();
    let item = add(&mut conn, 1, "assistant", "请使用中文，限制 10%_且简短");
    add(&mut conn, 1, "assistant", "无关说明");
    assert_eq!(recall(&conn, 1, "中文").unwrap(), vec![item.clone()]);
    assert_eq!(recall(&conn, 1, "%_").unwrap(), vec![item]);
    assert!(recall(&conn, 1, "不存在").unwrap().is_empty());
    assert!(recall(&conn, 1, &"字".repeat(201)).is_err());
}

#[test]
fn test_memory_recall_limits_count_and_stable_order() {
    let mut conn = db();
    for i in 0..15 { add(&mut conn, 1, "assistant", &format!("偏好{i}")); }
    conn.execute("UPDATE butler_memory SET updated_at=10", []).unwrap();
    let result = recall(&conn, 1, "").unwrap();
    assert_eq!(result.len(), 10);
    assert!(result.windows(2).all(|pair| pair[0].id > pair[1].id));
}

#[test]
fn test_memory_recall_counts_unicode_without_partial_record() {
    let mut conn = db();
    for i in 0..5 { add(&mut conn, 1, "assistant", &format!("{}{}", "字".repeat(1999), i)); }
    let result = recall(&conn, 1, "").unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.iter().map(|m|m.content.chars().count()).sum::<usize>(), 6000);
}

#[test]
fn test_memory_update_revision_and_source() {
    let mut conn = db();
    let old = add(&mut conn, 1, "assistant", "原偏好");
    let tx = conn.transaction().unwrap();
    let new = update(&tx, 1, old.id, 1, "新偏好", 99).unwrap();
    assert_eq!(new.id, old.id);
    assert_eq!(new.revision, 2);
    assert_eq!(new.source_message_id, Some(99));
    assert_eq!(new.created_at, old.created_at);
    assert!(update(&tx, 1, old.id, 1, "过期值", 99).is_err());
    assert!(forget(&tx, 1, old.id, 1).is_err());
    assert_eq!(get(&tx, 1, old.id).unwrap().unwrap().content, "新偏好");
}

#[test]
fn test_memory_update_rejects_duplicate() {
    let mut conn = db();
    let first = add(&mut conn, 1, "assistant", "第一条");
    add(&mut conn, 1, "assistant", "第二条");
    let tx = conn.transaction().unwrap();
    assert!(update(&tx, 1, first.id, 1, "第二条", 1).is_err());
    assert_eq!(get(&tx, 1, first.id).unwrap().unwrap(), first);
}

#[test]
fn test_memory_delete_is_idempotent_and_old_update_cannot_restore() {
    let mut conn = db();
    let value = add(&mut conn, 1, "assistant", "忘掉它");
    let tx = conn.transaction().unwrap();
    assert!(forget(&tx, 1, value.id, 1).unwrap());
    assert!(!forget(&tx, 1, value.id, 1).unwrap());
    assert!(update(&tx, 1, value.id, 1, "重新恢复", 1).is_err());
    assert!(recall(&tx, 1, "").unwrap().is_empty());
}

#[test]
fn test_memory_rollback_leaves_no_partial_data() {
    let mut conn = db();
    {
        let tx = conn.transaction().unwrap();
        remember(&tx, 1, "assistant", "不应保存", 1).unwrap();
    }
    assert!(recall(&conn, 1, "").unwrap().is_empty());
}

#[test]
fn test_memory_deleted_source_does_not_cascade_to_memory() {
    let mut conn = db();
    conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE message(id INTEGER PRIMARY KEY); INSERT INTO message VALUES(1);").unwrap();
    let value = add(&mut conn, 1, "assistant", "已保存的偏好");
    conn.execute("DELETE FROM message WHERE id=1", []).unwrap();
    assert_eq!(get(&conn, 1, value.id).unwrap().unwrap(), value);
}

#[test]
fn test_memory_reserved_group_is_invisible() {
    let conn = db();
    conn.execute("INSERT INTO butler_memory(content,scope_type,scope_id,created_at,updated_at) VALUES('群资料','group',1,0,0)", []).unwrap();
    assert!(get(&conn, 1, conn.last_insert_rowid()).unwrap().is_none());
    assert!(recall(&conn, 1, "").unwrap().is_empty());
}
