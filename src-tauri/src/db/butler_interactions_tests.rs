use super::*;

#[test]
fn test_list_keeps_native_previews_and_legacy_answer_at_the_question_time() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY);
        CREATE TABLE message(id INTEGER PRIMARY KEY);
        INSERT INTO conversation VALUES(1),(2); INSERT INTO message VALUES(3),(4);").unwrap();
    super::super::butler_goal::schema(&conn).unwrap();
    super::super::butler_question_history::schema(&conn).unwrap();
    conn.execute("INSERT INTO butler_goal_question VALUES('q',1,0,86500,?1,'answered')",
        [r#"{"request_id":"q","questions":[]}"#]).unwrap();
    conn.execute("INSERT INTO butler_task_question_answer(task_conversation_id,question_key,record_json)
        VALUES(1,'request:q',?1)", [r#"{"answers":{"format":"PDF"},"delivery_status":"delivered"}"#]).unwrap();
    save_preview(&conn,1,3,"p2",110_000.5,r#"{"files":[2]}"#).unwrap();
    save_preview(&conn,1,3,"p1",110_000.1,r#"{"files":[1]}"#).unwrap();
    save_preview(&conn,2,4,"foreign",120_000.0,"{}").unwrap();
    let rows = list(&conn,1).unwrap();
    assert_eq!(rows.iter().map(|row| row.request_id.as_str()).collect::<Vec<_>>(), ["q","p1","p2"]);
    assert_eq!(rows[0].occurred_ms,100_000.0);
    assert_eq!(rows[0].answers.as_ref().unwrap()["format"],"PDF");
    assert_eq!(rows[1].anchor,Some(3));
    assert!(rows[1].request.is_null());
    assert_eq!(preview_parameters(&conn,1,"p1").unwrap(),r#"{"files":[1]}"#);
    assert!(preview_parameters(&conn,2,"p1").is_err());
}
