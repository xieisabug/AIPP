use super::*;
#[test]
fn test_choice_archive_keeps_source_assets_and_build_after_history_is_removed() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY); INSERT INTO conversation VALUES(1);").unwrap();
    super::super::schema(&conn).unwrap();
    let request = serde_json::json!({"title":"test","candidates":[{"type":"artifact","id":"x","title":"site","entry_file":"index.html","content":"<img>"}]});
    conn.execute("INSERT INTO present_choices(request_id,conversation_id,boot,expires,request) VALUES('r',1,'old',0,?1)", [request.to_string()]).unwrap();
    conn.execute("INSERT INTO choice_project VALUES('r','x',?1)", [r#"{"index.html":"PGltZz4=","img.png":"YQ=="}"#]).unwrap();
    conn.execute("INSERT INTO choice_build VALUES('r','x',?1)", [r#"{"index.html":"b2s="}"#]).unwrap();
    let marker = |cid| format!("{REFERENCE}{}", serde_json::json!({"conversationId":cid,"requestId":"r","candidateId":"x"}));
    assert!(capture_from(&conn, &marker(2)).is_err());
    let code = capture_from(&conn, &marker(1)).unwrap();
    conn.execute("DELETE FROM present_choices", []).unwrap();
    let archive = Archive::decode(&code).unwrap();
    assert_eq!(archive.candidate.content.as_deref(), Some("<img>"));
    assert_eq!(archive.files.unwrap()["img.png"], "YQ==");
    assert_eq!(archive.build.unwrap()["index.html"], "b2s=");
    assert!(!code.contains('<'));
}
