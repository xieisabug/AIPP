use super::*;

fn conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE conversation(id INTEGER PRIMARY KEY);
         INSERT INTO conversation VALUES (1), (2);",
    )
    .unwrap();
    conn
}

fn sample(id: &str, conversation_id: Option<i64>, kind: &str) -> OfficeFileRef {
    OfficeFileRef {
        id: id.to_string(),
        conversation_id,
        kind: kind.to_string(),
        file_path: format!("C:/docs/{id}.docx"),
        file_name: format!("{id}.docx"),
        format: "docx".into(),
        sha256: "abc".into(),
        size_bytes: 10,
        origin_ref: None,
        origin_path: None,
        created_at: 1,
    }
}

#[test]
fn test_office_ref_resolve_only_within_owning_conversation() {
    let conn = conn();
    insert(&conn, &sample("ofr_a", Some(1), KIND_SELECTION)).unwrap();
    assert_eq!(resolve_for_conversation(&conn, "ofr_a", Some(1)).unwrap().unwrap().id, "ofr_a");
    // 其他会话、无会话、不存在的引用都按“找不到”处理
    assert!(resolve_for_conversation(&conn, "ofr_a", Some(2)).unwrap().is_none());
    assert!(resolve_for_conversation(&conn, "ofr_a", None).unwrap().is_none());
    assert!(resolve_for_conversation(&conn, "ofr_missing", Some(1)).unwrap().is_none());
}

#[test]
fn test_office_ref_unbound_selection_binds_to_first_conversation() {
    let conn = conn();
    insert(&conn, &sample("ofr_new", None, KIND_SELECTION)).unwrap();
    let bound = resolve_for_conversation(&conn, "ofr_new", Some(2)).unwrap().unwrap();
    assert_eq!(bound.conversation_id, Some(2));
    assert_eq!(get(&conn, "ofr_new").unwrap().unwrap().conversation_id, Some(2));
    // 绑定后其他会话不能再用
    assert!(resolve_for_conversation(&conn, "ofr_new", Some(1)).unwrap().is_none());
}

#[test]
fn test_office_ref_cascade_on_conversation_delete() {
    let conn = conn();
    insert(&conn, &sample("ofr_sel", Some(1), KIND_SELECTION)).unwrap();
    insert(&conn, &sample("ofr_out", Some(1), KIND_OUTPUT)).unwrap();
    insert(&conn, &sample("ofr_other", Some(2), KIND_OUTPUT)).unwrap();
    conn.execute("DELETE FROM conversation WHERE id = 1", []).unwrap();
    assert!(get(&conn, "ofr_sel").unwrap().is_none());
    assert!(get(&conn, "ofr_out").unwrap().is_none());
    assert!(get(&conn, "ofr_other").unwrap().is_some());
}

#[test]
fn test_office_ref_ids_are_prefixed_and_unique() {
    let a = new_ref_id();
    let b = new_ref_id();
    assert!(a.starts_with("ofr_") && a.len() == 20);
    assert_ne!(a, b);
}
