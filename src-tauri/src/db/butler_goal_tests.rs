use super::*;
fn db()->Connection {
 let c=Connection::open_in_memory().unwrap();
 c.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,conversation_kind TEXT,assistant_id INTEGER);INSERT INTO conversation VALUES(1,'butler_contact',7),(2,'normal',7),(3,'butler_task',7);CREATE TABLE message(id INTEGER PRIMARY KEY,parent_id INTEGER,conversation_id INTEGER,message_type TEXT,content TEXT,finish_time TEXT);CREATE TABLE queued_conversation_message(id INTEGER PRIMARY KEY,conversation_id INTEGER,queue_kind TEXT,status TEXT,request_json TEXT,prompt TEXT,assistant_id INTEGER);").unwrap(); c
}
fn save_test_question(c:&Connection,conversation:i64,id:&str,event:&str,now:i64)->Result<()> {
 let source=100+conversation;
 c.execute("INSERT OR IGNORE INTO message(id,conversation_id,message_type,content) VALUES(?1,?2,'user','fixture source')",params![source,conversation])?;
 bind_turn(c,conversation,source,current_revision(c,conversation)?.unwrap_or(0))?;
 save_question(c,conversation,id,event,Some(source),now)
}
#[test] fn test_goal_replace_supersedes_twice_and_preserves_evidence(){
 let c=db();let a=apply(&c,1,"replace","old").unwrap().unwrap();
 c.execute("INSERT INTO message(id,parent_id,conversation_id,message_type,content) VALUES(1,NULL,1,'user','old'),(2,1,1,'response','partial'),(3,1,1,'tool_result','valid evidence'),(4,NULL,1,'system','<!-- compacted_ids: {} --> old')",[]).unwrap();bind_turn(&c,1,1,a.revision).unwrap();
 apply(&c,1,"replace","new").unwrap();let z=apply(&c,1,"replace","latest").unwrap().unwrap();assert_eq!(z.text,"latest");assert!(assert_message_current(&c,1,Some(2)).is_err());
 assert_eq!(c.query_row("SELECT content FROM message WHERE id=2",[],|r|r.get::<_,String>(0)).unwrap(),"partial");
 assert_eq!(c.query_row("SELECT count(*) FROM butler_goal_stale_message WHERE message_id=2",[],|r|r.get::<_,i64>(0)).unwrap(),1);
 assert_eq!(c.query_row("SELECT content FROM message WHERE id=3",[],|r|r.get::<_,String>(0)).unwrap(),"valid evidence");assert_eq!(c.query_row("SELECT count(*) FROM message WHERE id=4",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}
#[test] fn test_goal_aside_does_not_change_authority_and_stop_invalidates(){let c=db();let a=apply(&c,1,"replace","goal").unwrap().unwrap();assert_eq!(apply(&c,1,"aside","why?").unwrap().unwrap(),a);apply(&c,1,"stop","").unwrap();assert!(assert_current(&c,1,a.revision).is_err());assert!(apply(&c,2,"replace","normal").unwrap().is_none());}
#[test] fn test_question_recovery_is_bound_expiring_and_idempotent(){let mut c=db();apply(&c,1,"replace","goal").unwrap();let event=r#"{"questions":[{"header":"format","question":"format?"}]}"#;save_test_question(&c,1,"q",event,100).unwrap();assert_eq!(waiting_questions(&c,1,101).unwrap().len(),1);let a=std::collections::HashMap::from([("format".into(),"markdown".into())]);recover_answer(&mut c,"q",&a,false,101).unwrap();assert!(recover_answer(&mut c,"q",&a,false,101).is_err());assert_eq!(c.query_row("SELECT count(*) FROM queued_conversation_message",[],|r|r.get::<_,i64>(0)).unwrap(),1);
 save_test_question(&c,1,"expired",event,100).unwrap();assert!(validate_question(&c,"expired",86500).is_err());save_test_question(&c,1,"old",event,100).unwrap();apply(&c,1,"replace","new goal").unwrap();assert!(recover_answer(&mut c,"old",&a,false,101).is_err());
}
#[test] fn test_question_recovery_rejects_secret_and_wrong_answer_keys(){let mut c=db();apply(&c,1,"replace","goal").unwrap();save_test_question(&c,1,"q",r#"{"questions":[{"header":"password","isSecret":true}]}"#,100).unwrap();let a=std::collections::HashMap::from([("password".into(),"secret".into())]);assert!(recover_answer(&mut c,"q",&a,false,101).is_err());save_test_question(&c,1,"q2",r#"{"questions":[{"header":"format"}]}"#,100).unwrap();assert!(recover_answer(&mut c,"q2",&a,false,101).is_err());assert_eq!(waiting_questions(&c,1,101).unwrap().len(),2);}
#[test] fn test_question_other_task_survives_goal_change(){let c=db();apply(&c,1,"replace","one").unwrap();apply(&c,3,"replace","two").unwrap();save_test_question(&c,3,"q",r#"{"questions":[]}"#,100).unwrap();apply(&c,1,"replace","new").unwrap();assert!(validate_question(&c,"q",101).unwrap().is_some());}

#[test] fn test_old_turn_cannot_publish_question_after_goal_change(){
 let c=db();let g=apply(&c,1,"replace","first").unwrap().unwrap();
 bind_turn(&c,1,11,g.revision).unwrap();apply(&c,1,"replace","second").unwrap();
 assert!(save_question(&c,1,"late",r#"{"questions":[]}"#,Some(11),100).is_err());
 assert!(waiting_questions(&c,1,101).unwrap().is_empty());
}

#[test] fn test_checkpoint_read_rejects_changed_or_missing_authority(){
 let c=db();apply(&c,1,"replace","goal").unwrap();let before=prompt(&c,1).unwrap().unwrap();
 let content=format!("<!-- compacted_ids: {} -->\n<context_summary>summary</context_summary>",serde_json::json!({"ids":[1],"digest":"x","goal_snapshot":before}));
 assert!(checkpoint_is_current(&content,Some(&before)));
 assert!(!checkpoint_is_current("<!-- compacted_range: 1..2 -->old",Some(&before)));
 apply(&c,1,"replace","changed").unwrap();assert!(!checkpoint_is_current(&content,prompt(&c,1).unwrap().as_deref()));
 assert!(checkpoint_is_current(&content,None));
}

#[test] fn test_failed_queue_transaction_does_not_publish_new_goal(){
 let mut c=db();let original=apply(&c,1,"replace","old").unwrap();
 {let tx=c.transaction().unwrap();apply_in_transaction(&tx,1,"replace","new").unwrap();
 assert!(tx.execute("INSERT INTO missing_queue VALUES(1)",[]).is_err());}
 assert_eq!(current(&c,1).unwrap(),original);
}
#[test] fn test_completed_reply_is_not_labelled_stale(){
 let c=db();let goal=apply(&c,1,"replace","old").unwrap().unwrap();
 c.execute("INSERT INTO message VALUES(1,NULL,1,'user','old',NULL),(2,1,1,'response','finished report','2026-01-01')",[]).unwrap();
 bind_turn(&c,1,1,goal.revision).unwrap();apply(&c,1,"replace","new").unwrap();
 assert_eq!(c.query_row("SELECT count(*) FROM butler_goal_stale_message",[],|r|r.get::<_,i64>(0)).unwrap(),0);
 assert_eq!(c.query_row("SELECT content FROM message WHERE id=2",[],|r|r.get::<_,String>(0)).unwrap(),"finished report");
}

#[test]
fn test_goal_output_without_parent_is_not_mistaken_for_current_goal() {
    let c=db();
    c.execute("ALTER TABLE message ADD COLUMN metadata_json TEXT",[]).unwrap();
    let first=apply(&c,1,"replace","old").unwrap().unwrap();
    c.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES(1,1,'user','old')",[]).unwrap();
    bind_turn(&c,1,1,first.revision).unwrap();
    apply(&c,1,"replace","new").unwrap();
    // An unidentified legacy/plugin output must not acquire the current goal
    // just because its row was created later. Production drivers now pass a source.
    c.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES(2,1,'response','useful evidence')",[]).unwrap();
    bind_output(&c,1,2).unwrap();
    assert!(assert_message_current(&c,1,Some(2)).is_err());
    mark_obsolete_output(&c,2).unwrap();
    let (text,metadata):(String,String)=c.query_row("SELECT content,metadata_json FROM message WHERE id=2",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(text,"useful evidence");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&metadata).unwrap()["end_reason"],"goal_superseded");
    // Native usage persistence may replace metadata later. The write boundary
    // re-applies the marker without replacing the original content.
    c.execute("UPDATE message SET metadata_json='{}',content='late complete' WHERE id=2",[]).unwrap();
    mark_obsolete_output(&c,2).unwrap();
    assert_eq!(c.query_row("SELECT content FROM message WHERE id=2",[],|r|r.get::<_,String>(0)).unwrap(),"late complete");
}
#[test]
fn test_goal_output_new_turn_and_normal_chat_are_unchanged() {
    let c=db();c.execute("ALTER TABLE message ADD COLUMN metadata_json TEXT",[]).unwrap();
    let goal=apply(&c,1,"replace","current").unwrap().unwrap();
    c.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES(1,1,'user','current'),(2,1,'response','answer'),(3,2,'response','ordinary')",[]).unwrap();
    c.execute("UPDATE message SET parent_id=1 WHERE id=2",[]).unwrap();
    bind_turn(&c,1,1,goal.revision).unwrap();bind_output(&c,1,2).unwrap();bind_output(&c,2,3).unwrap();
    for id in [2,3] {mark_obsolete_output(&c,id).unwrap();}
    assert!(assert_message_current(&c,1,Some(2)).is_ok());
    assert_eq!(c.query_row("SELECT count(*) FROM message WHERE metadata_json IS NOT NULL",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}

#[test]
fn test_goal_late_output_and_question_never_inherit_newer_user_identity() {
 let c=db();let old=apply(&c,1,"replace","old").unwrap().unwrap();
 c.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES(1,1,'user','old')",[]).unwrap();
 bind_turn(&c,1,1,old.revision).unwrap();
 let new=apply(&c,1,"replace","new").unwrap().unwrap();
 c.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES(2,1,'user','new')",[]).unwrap();
 bind_turn(&c,1,2,new.revision).unwrap();
 // The old actor arrives only AFTER the new user is persisted.
 c.execute("INSERT INTO message(id,parent_id,conversation_id,message_type,content) VALUES(3,1,1,'response','late old'),(4,2,1,'response','new answer'),(5,NULL,1,'response','unidentified')",[]).unwrap();
 for id in [3,4,5] {bind_output(&c,1,id).unwrap();}
 assert_eq!(message_revision(&c,1,3).unwrap(),Some(old.revision));
 assert_eq!(message_revision(&c,1,4).unwrap(),Some(new.revision));
 assert!(assert_message_current(&c,1,Some(3)).is_err());
 assert!(assert_message_current(&c,1,Some(4)).is_ok());
 assert!(assert_message_current(&c,1,Some(5)).is_err());
 assert!(save_question(&c,1,"old-question",r#"{"questions":[]}"#,Some(3),100).is_err());
 assert!(save_question(&c,1,"missing-source",r#"{"questions":[]}"#,None,100).is_err());
 assert!(save_question(&c,1,"new-question",r#"{"questions":[]}"#,Some(4),100).is_ok());
}

#[test]
fn test_recovered_task_answer_history_is_atomic_and_survives_refresh() {
    let mut c = db();
    apply(&c, 3, "replace", "make report").unwrap();
    let event = r#"{"questions":[{"header":"format","question":"Which format?"}],"metadata":{"purpose":"report"}}"#;
    save_test_question(&c, 3, "recovered", event, 100).unwrap();
    let answers = std::collections::HashMap::from([("format".into(), "Markdown".into())]);
    recover_answer(&mut c, "recovered", &answers, false, 101).unwrap();
    assert!(waiting_questions(&c, 3, 102).unwrap().is_empty());
    let records = crate::db::butler_question_history::list(&c, 3).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].key, "request:recovered");
    assert_eq!(records[0].questions[0]["question"], "Which format?");
    assert_eq!(records[0].metadata["purpose"], "report");
    assert_eq!(records[0].answers, answers);
    assert_eq!(records[0].delivery_status, "delivered");
    assert!(!records[0].cancelled);
    assert!(crate::db::butler_question_history::list(&c, 1).unwrap().is_empty());
    assert!(recover_answer(&mut c, "recovered", &answers, false, 102).is_err());
    assert_eq!(crate::db::butler_question_history::list(&c, 3).unwrap().len(), 1);
    assert_eq!(c.query_row("SELECT count(*) FROM queued_conversation_message", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn test_recovered_question_history_failure_rolls_back_answer_and_queue() {
    let mut c = db();
    apply(&c, 3, "replace", "report").unwrap();
    save_test_question(&c, 3, "retry", r#"{"questions":[{"header":"format"}]}"#, 100).unwrap();
    crate::db::butler_question_history::list(&c, 3).unwrap();
    c.execute_batch("CREATE TRIGGER fail_history BEFORE INSERT ON butler_task_question_answer BEGIN SELECT RAISE(ABORT, 'history write failed'); END;").unwrap();
    let answers = std::collections::HashMap::from([("format".into(), "Markdown".into())]);
    assert!(recover_answer(&mut c, "retry", &answers, false, 101).is_err());
    assert_eq!(waiting_questions(&c, 3, 102).unwrap().len(), 1);
    assert_eq!(c.query_row("SELECT count(*) FROM queued_conversation_message", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert!(crate::db::butler_question_history::list(&c, 3).unwrap().is_empty());
    c.execute_batch("DROP TRIGGER fail_history;").unwrap();
    recover_answer(&mut c, "retry", &answers, false, 102).unwrap();
    assert_eq!(crate::db::butler_question_history::list(&c, 3).unwrap().len(), 1);
}

#[test]
fn test_recovered_question_cancel_history_does_not_queue_or_store_answers() {
    let mut c = db();
    apply(&c, 3, "replace", "report").unwrap();
    save_test_question(&c, 3, "cancel", r#"{"questions":[{"header":"secret","isSecret":true}]}"#, 100).unwrap();
    let answers = std::collections::HashMap::from([("secret".into(), "must not persist".into())]);
    recover_answer(&mut c, "cancel", &answers, true, 101).unwrap();
    let records = crate::db::butler_question_history::list(&c, 3).unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].cancelled);
    assert!(records[0].answers.is_empty());
    assert_eq!(records[0].delivery_status, "delivered");
    assert_eq!(c.query_row("SELECT count(*) FROM queued_conversation_message", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert!(recover_answer(&mut c, "cancel", &answers, false, 102).is_err());
}

#[test]
fn test_recovered_secret_alias_is_not_queued_or_saved() {
    let mut c = db();
    apply(&c, 3, "replace", "report").unwrap();
    save_test_question(&c, 3, "secret", r#"{"questions":[{"header":"password","is_secret":true}]}"#, 100).unwrap();
    let answers = std::collections::HashMap::from([("password".into(), "not stored".into())]);
    assert!(recover_answer(&mut c, "secret", &answers, false, 101).is_err());
    assert!(crate::db::butler_question_history::list(&c, 3).unwrap().is_empty());
}
