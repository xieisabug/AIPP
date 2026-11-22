use super::*;
fn request() -> Request {
    serde_json::from_value(json!({"title":"方案","candidates":[{"id":"a","title":"A","type":"text","content":"第一版"},{"id":"b","title":"B","type":"html","content":"第二版"}]})).unwrap()
}
#[test]
fn test_choices_rejects_duplicate_ids_and_conflicting_sources() {
    let mut req = request();
    req.candidates[0].url = Some("https://example.com".into());
    assert!(req.validate().is_err());
    req.candidates[0].url = None;
    req.candidates[1].id = "a".into();
    assert!(req.validate().is_err());
}
#[test]
fn test_choices_validates_decision_against_original_options() {
    let req = request();
    let select = |ids: Vec<&str>| Decision {
        status: "selected".into(),
        selected_ids: ids.into_iter().map(String::from).collect(),
        feedback: None,
    };
    assert!(validate(&req, "q", select(vec!["a", "b"])).is_err());
    assert!(validate(&req, "q", select(vec!["x"])).is_err());
    assert_eq!(validate(&req, "q", select(vec!["b"])).unwrap()["selected_titles"], json!(["B"]));
}
#[test]
fn test_choices_snapshots_bind_history_to_the_original_call() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation(id INTEGER PRIMARY KEY); INSERT INTO conversation VALUES(7)",
    )
    .unwrap();
    schema(&conn).unwrap();
    conn.execute("INSERT INTO present_choices(request_id,conversation_id,call_id,boot,expires,request) VALUES('q',7,10,'previous',0,?1)",[serde_json::to_string(&request()).unwrap()]).unwrap();
    assert!(card(&conn, 8, Some("q"), None).unwrap().is_none());
    let restored = card(&conn, 7, None, Some(10)).unwrap().unwrap();
    assert!(restored.error.is_some());
    assert_eq!(restored.request.candidates[0].content.as_deref(), Some("第一版"));
}

#[test]
fn test_choices_recovered_answers_enqueue_once_and_respect_goal_and_expiry() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY, assistant_id INTEGER);
        INSERT INTO conversation VALUES(7,3);
        CREATE TABLE queued_conversation_message(conversation_id, queue_kind, status, request_json, prompt, assistant_id);").unwrap();
    schema(&conn).unwrap();
    butler_goal::schema(&conn).unwrap();
    conn.execute(
        "INSERT INTO butler_goal(conversation_id,revision,goal) VALUES(7,2,'当前工作')",
        [],
    )
    .unwrap();
    for (id, revision, ttl) in
        [("active", 2, 60), ("cancel", 2, 60), ("stale", 1, 60), ("expired", 2, -1)]
    {
        conn.execute("INSERT INTO present_choices(request_id,conversation_id,boot,revision,expires,request) VALUES(?1,7,'old',?2,unixepoch()+?3,?4)",params![id,revision,ttl,serde_json::to_string(&request()).unwrap()]).unwrap();
    }
    let selected = || Decision {
        status: "selected".into(),
        selected_ids: vec!["a".into()],
        feedback: Some("保留 <vault:键>".into()),
    };
    assert!(complete(&mut conn, 7, "stale", selected()).is_err());
    assert!(complete(&mut conn, 7, "expired", selected()).is_err());
    assert_eq!(
        complete(
            &mut conn,
            7,
            "cancel",
            Decision { status: "cancelled".into(), selected_ids: vec![], feedback: None }
        )
        .unwrap(),
        (true, false)
    );
    assert_eq!(complete(&mut conn, 7, "active", selected()).unwrap(), (true, true));
    assert!(complete(&mut conn, 7, "active", selected()).is_err());
    let (count, prompt): (i64, String) = conn
        .query_row("SELECT COUNT(*),prompt FROM queued_conversation_message", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    let recovered: String = conn.query_row(
        "SELECT json_extract(request_json, '$.recovered_question_id') FROM queued_conversation_message",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(recovered, "active");
    assert_eq!(count, 1);
    assert!(prompt.contains("保留 <vault:键>"));
}

#[test]
fn test_choices_project_snapshot_survives_source_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/App.vue"), "original").unwrap();
    fs::write(root.join("logo.png"), [255, 0]).unwrap();
    let mut files = Project::new();
    let mut budget = snapshot::LIMIT;
    snapshot::collect_tree(&root, &root, &mut files, &mut budget, 0).unwrap();
    fs::write(root.join("src/App.vue"), "changed").unwrap();
    dir.close().unwrap();
    assert_eq!(STANDARD.decode(&files["src/App.vue"]).unwrap(), b"original");
    assert_eq!(STANDARD.decode(&files["logo.png"]).unwrap(), [255, 0]);
}

#[test]
#[cfg(unix)]
fn test_choices_snapshot_rejects_literal_backslash_instead_of_losing_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("nested/index.html"), "actual nested file").unwrap();
    fs::write(root.join(r"nested\index.html"), "literal backslash filename").unwrap();

    let mut files = Project::new();
    let mut budget = snapshot::LIMIT;
    let err = snapshot::collect_tree(&root, &root, &mut files, &mut budget, 0).unwrap_err();
    assert!(err.to_string().contains("文件名包含反斜杠"));
}

#[test]
fn test_choices_two_active_requests_deliver_to_the_original_waiter() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation(id INTEGER PRIMARY KEY); INSERT INTO conversation VALUES(1);",
    )
    .unwrap();
    schema(&conn).unwrap();
    let mut receivers = Vec::new();
    for (call, id) in [(7, "live-first"), (8, "live-second")] {
        conn.execute(
            "INSERT INTO present_choices(request_id,conversation_id,call_id,boot,expires,request)
            VALUES(?1,1,?2,?3,unixepoch()+600,?4)",
            params![id, call, &*BOOT, serde_json::to_string(&request()).unwrap()],
        )
        .unwrap();
        let (sender, receiver) = oneshot::channel();
        WAITING.lock().unwrap().insert(id.into(), sender);
        receivers.push(receiver);
    }
    let choose =
        || Decision { status: "selected".into(), selected_ids: vec!["b".into()], feedback: None };
    assert_eq!(card(&conn, 1, None, Some(8)).unwrap().unwrap().request_id, "live-second");
    complete(&mut conn, 1, "live-second", choose()).unwrap();
    assert!(matches!(receivers[0].try_recv(), Err(oneshot::error::TryRecvError::Empty)));
    assert_eq!(receivers[1].try_recv().unwrap()["selected_ids"], json!(["b"]));
    assert!(complete(&mut conn, 1, "live-second", choose()).is_err());
    complete(
        &mut conn,
        1,
        "live-first",
        Decision { status: "cancelled".into(), selected_ids: vec![], feedback: None },
    )
    .unwrap();
    assert_eq!(receivers[0].try_recv().unwrap()["status"], "cancelled");
}
