use super::*;
fn fixture() -> Connection {
    let c=Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,conversation_kind TEXT,butler_task_status TEXT,butler_task_finalized_at TEXT,butler_task_summary TEXT);
    CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,parent_id INTEGER,message_type TEXT,content TEXT,metadata_json TEXT);
    CREATE TABLE butler_task_definition(task_conversation_id INTEGER,butler_conversation_id INTEGER,executor_assistant_id INTEGER,title TEXT,goal TEXT);
    CREATE TABLE butler_task_result(task_conversation_id INTEGER,final_message_id INTEGER,summary TEXT);
    INSERT INTO conversation VALUES(10,1,'butler_main',NULL,NULL,NULL),(20,2,'butler_contact',NULL,NULL,NULL),(30,3,'butler_contact',NULL,NULL,NULL),(40,2,'butler_task','running',NULL,NULL);
    INSERT INTO butler_task_definition VALUES(40,10,2,'报告','写报告');
    INSERT INTO message VALUES(1,20,NULL,'user','继续',NULL),(2,20,1,'response','调用工具',NULL),(3,10,NULL,'user','恢复',NULL),(4,30,NULL,'user','恢复',NULL),(5,40,NULL,'user','原任务',NULL);").unwrap();
    super::super::butler_goal::schema(&c).unwrap();
    c.execute_batch("INSERT INTO butler_goal VALUES(40,1,'写报告',0);INSERT INTO butler_goal_turn VALUES(5,40,1)").unwrap();
    super::super::butler_control::set(&c,40,"pause").unwrap(); c
}
#[test]
fn test_resume_contact_ancestor_and_idempotency_preserve_delegation() {
    let mut c=fixture(); super::super::butler_control::set(&c,40,"stop_delegation").unwrap();
    let r=resume_work(&mut c,20,40,2).unwrap();assert!(r.resumed);assert!(r.delegation_stopped);assert_eq!(r.goal_revision,2);
    assert!(!resume_work(&mut c,20,40,2).unwrap().resumed);
    assert_eq!(super::super::butler_goal::current_revision(&c,40).unwrap(),Some(2));
    assert!(super::super::butler_goal::assert_message_current(&c,40,Some(5)).is_err());
}
#[test]
fn test_resume_main_owner_succeeds_and_foreign_contact_rejected() {
    let mut c=fixture();assert!(resume_work(&mut c,30,40,4).is_err());assert!(resume_work(&mut c,10,40,3).unwrap().resumed);
}
#[test]
fn test_resume_internal_malformed_and_stale_user_sources_rejected() {
    for meta in ["{\"origin\":\"internal\"}","{\"origin\":\"butler_peer\"}","{bad"] {
        let mut c=fixture();c.execute("UPDATE message SET metadata_json=?1 WHERE id=1",[meta]).unwrap();assert!(resume_work(&mut c,20,40,2).is_err());
    }
    let mut c=fixture();c.execute_batch("INSERT INTO message VALUES(6,20,NULL,'user','新要求',NULL)").unwrap();assert!(resume_work(&mut c,20,40,2).is_err());
}
#[test]
fn test_resume_cancelled_finalized_stopped_and_task_sources_rejected() {
    let mut c=fixture();super::super::butler_control::set(&c,40,"cancel").unwrap();assert!(resume_work(&mut c,20,40,2).is_err());
    let mut c=fixture();c.execute("UPDATE conversation SET butler_task_finalized_at='done' WHERE id=40",[]).unwrap();assert!(resume_work(&mut c,20,40,2).is_err());
    let mut c=fixture();c.execute("UPDATE butler_goal SET stopped=1 WHERE conversation_id=40",[]).unwrap();assert!(resume_work(&mut c,20,40,2).is_err());
    let mut c=fixture();assert!(resume_work(&mut c,40,40,5).is_err());
}
#[test]
fn test_resume_pending_work_blocks_without_mutation() {
    for sql in [
        "CREATE TABLE queued_conversation_message(conversation_id INTEGER,status TEXT);INSERT INTO queued_conversation_message VALUES(40,'queued')",
        "CREATE TABLE butler_work_followup(task_conversation_id INTEGER,result_task INTEGER,state TEXT);INSERT INTO butler_work_followup VALUES(40,NULL,'sending')",
        "CREATE TABLE butler_peer_exchange(owner INTEGER,target INTEGER,state TEXT,receipt_state TEXT);INSERT INTO butler_peer_exchange VALUES(40,20,'waiting',NULL)",
    ] {
        let mut c=fixture();c.execute_batch(sql).unwrap();assert!(resume_work(&mut c,20,40,2).is_err());
        assert!(super::super::butler_control::get(&c,40).unwrap().paused);
        assert_eq!(super::super::butler_goal::current_revision(&c,40).unwrap(),Some(1));
    }
}
#[test]
fn test_resume_invalidates_old_questions_without_removing_history() {
    let mut c=fixture();c.execute_batch("INSERT INTO butler_goal_question VALUES('q',40,1,999999,'{}','waiting')").unwrap();
    resume_work(&mut c,20,40,2).unwrap();
    assert_eq!(c.query_row("SELECT state FROM butler_goal_question WHERE request_id='q'",[],|r|r.get::<_,String>(0)).unwrap(),"superseded");
    assert_eq!(c.query_row("SELECT count(*) FROM message",[],|r|r.get::<_,i64>(0)).unwrap(),5);
}

#[test]
fn test_resume_accepts_tool_result_ancestor_without_parsing_user_wording() {
    // This verifies the tool boundary is not a keyword classifier, NOT model intent accuracy.
    for text in ["继续弄刚才那份报告吧", "把那个暂停的工作接着做", "Resume the report please", "好，接着处理", "这里是任意文本"] {
        let mut c=fixture();c.execute("UPDATE message SET content=?1 WHERE id=1",[text]).unwrap();
        c.execute_batch("INSERT INTO message VALUES(6,20,2,'tool_result','已读到工作状态',NULL)").unwrap();
        assert!(resume_work(&mut c,20,40,6).unwrap().resumed);
    }
}

#[test]
fn test_resume_parallel_independent_work_does_not_change_other_control() {
    let c=fixture();
    c.execute_batch("INSERT INTO conversation VALUES(50,3,'butler_task','running',NULL,NULL);
        INSERT INTO butler_task_definition VALUES(50,10,3,'其他报告','写其他报告');
        INSERT INTO butler_goal VALUES(50,1,'写其他报告',0)").unwrap();
    super::super::butler_control::set(&c,50,"pause").unwrap();
    super::super::butler_control::set(&c,50,"stop_delegation").unwrap();
    let shared=std::sync::Arc::new(std::sync::Mutex::new(c));
    let threads:Vec<_>=[(20,40,2),(30,50,4)].into_iter().map(|(current,target,source)| {
        let shared=shared.clone();std::thread::spawn(move || {
            resume_work(&mut shared.lock().unwrap(),current,target,source).unwrap()
        })
    }).collect();
    for thread in threads { assert!(thread.join().unwrap().resumed); }
    let c=shared.lock().unwrap();
    assert!(!super::super::butler_control::get(&c,40).unwrap().delegation_stopped);
    assert!(super::super::butler_control::get(&c,50).unwrap().delegation_stopped);
    for id in [40,50] { assert_eq!(super::super::butler_goal::current_revision(&c,id).unwrap(),Some(2)); }
}

#[test]
fn test_resume_cancel_race_never_revives_cancelled_work() {
    // Real admission is serialized by the caller's task lock; model both competing orders.
    for _ in 0..20 {
        let shared=std::sync::Arc::new(std::sync::Mutex::new(fixture()));
        let ready=std::sync::Arc::new(std::sync::Barrier::new(2));
        let resume={let shared=shared.clone();let ready=ready.clone();std::thread::spawn(move || {
            ready.wait();let _=resume_work(&mut shared.lock().unwrap(),20,40,2);
        })};
        let cancel={let shared=shared.clone();let ready=ready.clone();std::thread::spawn(move || {
            ready.wait();super::super::butler_control::set(&shared.lock().unwrap(),40,"cancel").unwrap();
        })};
        resume.join().unwrap();cancel.join().unwrap();
        let mut c=shared.lock().unwrap();
        let control=super::super::butler_control::get(&c,40).unwrap();
        assert!(control.cancelled && control.paused);
        assert!(resume_work(&mut c,20,40,2).is_err());
    }
}
