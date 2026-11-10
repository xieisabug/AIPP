use super::*;

fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema(&conn).unwrap();
    conn
}

fn add(conn: &Connection, scope: Scope, text: &str) -> Memory {
    remember(conn, scope, text, Some(Source::Message(1)), Some(1)).unwrap().0
}

#[test]
fn test_memory_schema_idempotent_keeps_records() {
    let conn = db();
    let value = add(&conn, Scope::Assistant(1), "回答简短");
    schema(&conn).unwrap();
    assert_eq!(get_any(&conn, value.id).unwrap().unwrap(), value);
}

#[test]
fn test_memory_schema_rejects_invalid_scope_and_source() {
    let conn = db();
    for sql in [
        "INSERT INTO butler_memory(content,scope_type,scope_id,created_at,updated_at) VALUES('a','global',1,0,0)",
        "INSERT INTO butler_memory(content,scope_type,scope_id,created_at,updated_at) VALUES('a','assistant',NULL,0,0)",
        "INSERT INTO butler_memory(content,scope_type,scope_id,created_at,updated_at) VALUES('a','group',0,0,0)",
        "INSERT INTO butler_memory(content,scope_type,scope_id,created_at,updated_at) VALUES('a','unknown',1,0,0)",
        "INSERT INTO butler_memory(content,scope_type,scope_id,created_at,updated_at) VALUES('  ','global',NULL,0,0)",
        "INSERT INTO butler_memory(content,scope_type,scope_id,source_message_id,source_group_id,source_group_seq,created_at,updated_at) VALUES('a','global',NULL,1,2,3,0,0)",
        "INSERT INTO butler_memory(content,scope_type,scope_id,source_group_id,created_at,updated_at) VALUES('a','global',NULL,2,0,0)",
    ] {
        assert!(conn.execute(sql, []).is_err(), "{sql}");
    }
}

#[test]
fn test_memory_remember_validates_content() {
    let conn = db();
    for text in ["   ".to_owned(), "字".repeat(MAX_CONTENT_CHARS + 1), "a\0b".to_owned()] {
        assert!(remember(&conn, Scope::Global, &text, None, None).is_err());
    }
    assert!(remember(&conn, Scope::Assistant(0), "内容", None, None).is_err());
    assert!(remember(&conn, Scope::Global, &"字".repeat(MAX_CONTENT_CHARS), None, None).is_ok());
}

#[test]
fn test_memory_viewers_see_own_scope_and_global_only() {
    let conn = db();
    let a = add(&conn, Scope::Assistant(1), "助手一的偏好");
    let b = add(&conn, Scope::Assistant(2), "助手二的偏好");
    let group = add(&conn, Scope::Group(10), "本群周五交付");
    let global = add(&conn, Scope::Global, "所有助手使用中文");
    assert!(get(&conn, Viewer::Assistant(2), a.id).unwrap().is_none());
    assert!(get(&conn, Viewer::Assistant(1), group.id).unwrap().is_none());
    assert!(get(&conn, Viewer::Group(10), a.id).unwrap().is_none(), "assistant memory must not enter a group");
    assert!(get(&conn, Viewer::Group(11), group.id).unwrap().is_none());
    assert!(get(&conn, Viewer::Group(10), global.id).unwrap().is_some());
    let ids = |viewer| list(&conn, viewer).unwrap().into_iter().map(|m| m.id).collect::<Vec<_>>();
    assert_eq!(ids(Viewer::Assistant(1)), vec![a.id, global.id]);
    assert_eq!(ids(Viewer::Assistant(2)), vec![b.id, global.id]);
    assert_eq!(ids(Viewer::Group(10)), vec![group.id, global.id]);
    assert_eq!(count(&conn, Viewer::Group(10)).unwrap(), 2);
}

#[test]
fn test_memory_deduplicates_within_scope_only() {
    let conn = db();
    let (first, created) = remember(&conn, Scope::Assistant(1), "  中文偏好\r\n简短  ", None, None).unwrap();
    assert!(created);
    let (again, created) = remember(&conn, Scope::Assistant(1), "中文偏好\n简短", None, None).unwrap();
    assert!(!created);
    assert_eq!(first.id, again.id);
    assert_ne!(first.id, add(&conn, Scope::Assistant(2), "中文偏好\n简短").id);
    assert_ne!(first.id, add(&conn, Scope::Group(3), "中文偏好\n简短").id);
}

#[test]
fn test_memory_terms_split_chinese_bigrams_and_words() {
    assert_eq!(terms("回复偏好"), vec!["偏好", "回复", "复偏"]);
    assert_eq!(terms("用 TypeScript, 不用 JS"), vec!["js", "typescript", "不用", "用"]);
    assert!(terms("我们，一下").is_empty(), "stop bigrams are ignored");
}

#[test]
fn test_memory_recall_matches_partial_chinese_wording() {
    let conn = db();
    let style = add(&conn, Scope::Assistant(1), "用户希望回复简短，先给结论");
    add(&conn, Scope::Assistant(1), "项目使用 pnpm 管理依赖");
    let found = recall(&conn, Viewer::Assistant(1), "回复风格").unwrap();
    assert_eq!(found, vec![style]);
    assert!(recall(&conn, Viewer::Assistant(1), "数据库").unwrap().is_empty());
    assert_eq!(recall(&conn, Viewer::Assistant(1), "PNPM").unwrap().len(), 1);
    assert!(recall(&conn, Viewer::Assistant(1), &"字".repeat(MAX_QUERY_CHARS + 1)).is_err());
}

#[test]
fn test_memory_recall_ranks_by_overlap_then_recency_and_limits() {
    let conn = db();
    for i in 0..15 { add(&conn, Scope::Assistant(1), &format!("备注{i}")); }
    let best = add(&conn, Scope::Assistant(1), "周报格式：先写结论再写风险");
    conn.execute("UPDATE butler_memory SET updated_at=10", []).unwrap();
    conn.execute("UPDATE butler_memory SET updated_at=5 WHERE id=?1", [best.id]).unwrap();
    let found = recall(&conn, Viewer::Assistant(1), "周报结论").unwrap();
    assert_eq!(found[0].id, best.id, "more matched terms wins over recency");
    let recent = recall(&conn, Viewer::Assistant(1), "").unwrap();
    assert_eq!(recent.len(), RECALL_LIMIT);
    assert!(recent.windows(2).all(|pair| pair[0].id > pair[1].id), "same timestamp falls back to id");
}

#[test]
fn test_memory_prompt_selection_prefers_relevant_within_budget() {
    let conn = db();
    let relevant = add(&conn, Scope::Assistant(1), &format!("部署流程：{}", "先".repeat(250)));
    let filler: Vec<_> = (0..6).map(|i| add(&conn, Scope::Assistant(1), &format!("{i}{}", "无".repeat(199)))).collect();
    let short = add(&conn, Scope::Global, "回答使用中文");
    // 最旧的短记录排在所有长记录之后：长记录放不下被跳过后，它仍能放入。
    conn.execute("UPDATE butler_memory SET updated_at=0 WHERE id=?1", [short.id]).unwrap();
    let selection = select_for_prompt(&conn, Viewer::Assistant(1), "今天的部署流程是什么", 600).unwrap();
    assert_eq!(selection.memories[0].id, relevant.id, "relevant memory goes first even though it is oldest");
    let used: usize = selection.memories.iter().map(|m| m.content.chars().count()).sum();
    assert!(used <= 600);
    assert!(selection.memories.iter().any(|m| m.id == short.id), "a short memory still fits after a long one is skipped");
    assert_eq!(selection.omitted, filler.len() + 2 - selection.memories.len());
}

#[test]
fn test_memory_prompt_selection_empty_when_nothing_visible() {
    let conn = db();
    add(&conn, Scope::Assistant(2), "别人的记忆");
    let selection = select_for_prompt(&conn, Viewer::Assistant(1), "任意", PROMPT_BUDGET_CHARS).unwrap();
    assert_eq!(selection, Selection { memories: vec![], omitted: 0 });
}

#[test]
fn test_memory_update_checks_revision_scope_and_duplicates() {
    let conn = db();
    let old = add(&conn, Scope::Assistant(1), "原偏好");
    add(&conn, Scope::Assistant(1), "已有记录");
    assert!(update(&conn, Some(Viewer::Assistant(2)), old.id, None, "越权", None).is_err());
    assert!(update(&conn, Some(Viewer::Assistant(1)), old.id, None, "已有记录", None).is_err());
    let new = update(&conn, Some(Viewer::Assistant(1)), old.id, Some(1), "新偏好", Some(Source::Message(9))).unwrap();
    assert_eq!((new.revision, new.source_message_id, new.created_at, new.scope_type.as_str()), (2, Some(9), old.created_at, "assistant"));
    assert!(update(&conn, Some(Viewer::Assistant(1)), old.id, Some(1), "过期版本", None).is_err());
    let edited = update(&conn, None, old.id, None, "界面修改", None).unwrap();
    assert_eq!((edited.revision, edited.source_message_id), (3, Some(9)), "UI edits keep the source");
}

#[test]
fn test_memory_forget_respects_scope_and_revision() {
    let conn = db();
    let value = add(&conn, Scope::Group(5), "群里的约定");
    assert!(!forget(&conn, Some(Viewer::Group(6)), value.id, None).unwrap());
    assert!(!forget(&conn, Some(Viewer::Assistant(1)), value.id, None).unwrap());
    assert!(forget(&conn, Some(Viewer::Group(5)), value.id, Some(2)).is_err());
    assert!(forget(&conn, Some(Viewer::Group(5)), value.id, Some(1)).unwrap());
    assert!(!forget(&conn, Some(Viewer::Group(5)), value.id, None).unwrap());
    assert!(update(&conn, Some(Viewer::Group(5)), value.id, None, "复活", None).is_err());
}

#[test]
fn test_memory_source_round_trips() {
    let conn = db();
    let (group, _) = remember(&conn, Scope::Group(5), "群约定", Some(Source::GroupMessage { group: 5, seq: 12 }), Some(3)).unwrap();
    assert_eq!(group.source(), Some(Source::GroupMessage { group: 5, seq: 12 }));
    let (plain, _) = remember(&conn, Scope::Global, "无来源", None, None).unwrap();
    assert_eq!(plain.source(), None);
}
