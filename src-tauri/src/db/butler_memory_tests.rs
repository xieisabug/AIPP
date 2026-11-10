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

fn plan_for(conn: &Connection, text: &str, injected: &Injected, budget: usize) -> Delta {
    let viewer = Viewer::Assistant(1);
    plan(recent_for_prompt(conn, viewer).unwrap(), &visible_ids(conn, viewer).unwrap(), count(conn, viewer).unwrap(), text, injected, budget)
}

#[test]
fn test_memory_plan_initial_prefers_relevant_within_budget() {
    let conn = db();
    let relevant = add(&conn, Scope::Assistant(1), &format!("部署流程：{}", "先".repeat(250)));
    let filler: Vec<_> = (0..6).map(|i| add(&conn, Scope::Assistant(1), &format!("{i}{}", "无".repeat(199)))).collect();
    let short = add(&conn, Scope::Global, "回答使用中文");
    // 最旧的短记录排在所有长记录之后：长记录放不下被跳过后，它仍能放入。
    conn.execute("UPDATE butler_memory SET updated_at=0 WHERE id=?1", [short.id]).unwrap();
    let delta = plan_for(&conn, "今天的部署流程是什么", &Injected::default(), 600);
    assert!(delta.initial);
    assert_eq!(delta.listed[0].id, relevant.id, "relevant memory goes first even though it is oldest");
    let used: usize = delta.listed.iter().map(|m| m.content.chars().count()).sum();
    assert!(used <= 600);
    assert!(delta.listed.iter().any(|m| m.id == short.id), "a short memory still fits after a long one is skipped");
    assert_eq!(delta.omitted, filler.len() + 2 - delta.listed.len(), "首次附带放不下的计入 omitted");
}

#[test]
fn test_memory_plan_empty_when_nothing_visible() {
    let conn = db();
    add(&conn, Scope::Assistant(2), "别人的记忆");
    let delta = plan_for(&conn, "任意", &Injected::default(), PROMPT_BUDGET_CHARS);
    assert!(delta.is_empty() && delta.omitted == 0, "{delta:?}");
}

#[test]
fn test_memory_remember_duplicate_hits_unique_index_and_returns_existing() {
    // 先插入再查重：重复内容由唯一索引拦下（并发保存同一句话也走这条路），返回已有记录而不是约束错误。
    let conn = db();
    let first = add(&conn, Scope::Global, "所有助手使用中文");
    let (again, created) = remember(&conn, Scope::Global, "所有助手使用中文", None, Some(9)).unwrap();
    assert!(!created);
    assert_eq!(again, first, "已有记录保持原样（作者、来源不被覆盖）");
    assert_eq!(count(&conn, Viewer::Assistant(1)).unwrap(), 1);
}

#[test]
fn test_memory_plan_tops_up_relevant_old_memories_left_out_by_budget() {
    let conn = db();
    let deploy = add(&conn, Scope::Assistant(1), &format!("部署流程：{}", "先".repeat(150)));
    let style = add(&conn, Scope::Assistant(1), &format!("回答风格：{}", "简".repeat(150)));
    let first = plan_for(&conn, "你好", &Injected::default(), 200);
    assert!(first.initial);
    assert_eq!(first.listed.len(), 1, "预算只放得下一条");
    assert_eq!(first.omitted, 1);
    let left_out = if first.listed[0].id == deploy.id { &style } else { &deploy };
    let injected = Injected {
        initial: true,
        as_of: now() + 1_000,
        revisions: HashMap::from([(first.listed[0].id, Some(first.listed[0].revision))]),
    };
    // 没附带过、也不相关：不补带，避免每轮把无关记忆逐条塞进历史。
    let unrelated = plan_for(&conn, "今天天气", &injected, 200);
    assert!(unrelated.is_empty() && unrelated.omitted == 1, "{unrelated:?}");
    // 和本轮相关时补带上。
    let topic = if left_out.id == deploy.id { "部署流程怎么走" } else { "回答风格是什么" };
    let relevant = plan_for(&conn, topic, &injected, 200);
    assert_eq!(relevant.listed.iter().map(|m| m.id).collect::<Vec<_>>(), vec![left_out.id]);
    assert!(!relevant.initial && relevant.omitted == 0);
}

#[test]
fn test_memory_plan_updates_ignore_budget_and_report_removals() {
    let conn = db();
    let long = add(&conn, Scope::Assistant(1), &"长".repeat(250));
    let injected = Injected {
        initial: true,
        as_of: now() + 1_000,
        revisions: HashMap::from([(long.id, Some(long.revision)), (999, Some(1)), (998, None)]),
    };
    update(&conn, Some(Viewer::Assistant(1)), long.id, None, &"新".repeat(250), None).unwrap();
    let delta = plan_for(&conn, "任意", &injected, 10);
    assert_eq!(delta.updated.iter().map(|m| (m.id, m.revision)).collect::<Vec<_>>(), vec![(long.id, 2)], "更新必须替换旧版本，不受预算限制");
    assert_eq!(delta.removed, vec![999], "已经告知删除过的（998）不再重复");
}

#[test]
fn test_memory_plan_initial_injection_skips_memories_already_seen_at_current_revision() {
    let conn = db();
    let seen = add(&conn, Scope::Assistant(1), "回答先给结论");
    let stale = add(&conn, Scope::Assistant(1), "周报用表格");
    let other = add(&conn, Scope::Assistant(1), "用户在做 AIPP 项目");
    update(&conn, None, stale.id, None, "周报用表格和图", None).unwrap();
    // 还没有首次附带，但模型已经从工具结果里看到 seen 的当前版本、stale 的旧版本。
    let mut injected = Injected::default();
    injected.saw(seen.id, Some(seen.revision));
    injected.saw(stale.id, Some(stale.revision));
    let delta = plan_for(&conn, "你好", &injected, 800);
    assert!(delta.initial);
    assert_eq!(delta.updated.iter().map(|m| m.id).collect::<Vec<_>>(), vec![stale.id]);
    assert_eq!(delta.listed.iter().map(|m| m.id).collect::<Vec<_>>(), vec![other.id]);
    assert_eq!(delta.omitted, 0, "看到过的不算没附带");
    // 全部都看到过：什么都不附带。
    injected.saw(stale.id, Some(2));
    injected.saw(other.id, Some(other.revision));
    assert!(plan_for(&conn, "你好", &injected, 800).is_empty());
}

#[test]
fn test_memory_injected_saw_merges_independent_of_order() {
    let mut forward = Injected::default();
    let mut backward = Injected::default();
    let events = [(1, Some(1)), (1, Some(3)), (1, Some(2)), (2, Some(1)), (2, None), (3, Some(4))];
    events.iter().for_each(|(id, revision)| forward.saw(*id, *revision));
    events.iter().rev().for_each(|(id, revision)| backward.saw(*id, *revision));
    assert_eq!(forward, backward);
    assert_eq!(forward.revisions, HashMap::from([(1, Some(3)), (2, None), (3, Some(4))]), "删除优先，否则取较大版本");
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
