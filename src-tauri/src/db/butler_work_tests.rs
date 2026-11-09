use super::*;

#[test]
fn test_butler_work_failed_exchange_returns_original_error_with_access_control() {
    let mut conn = fixture();
    conn.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES(100,10,'user','请写作写诗')", []).unwrap();
    let ex = crate::db::butler_peer::enqueue(&mut conn, crate::db::butler_peer::NewRequest {
        owner:10,target:20,sender:"总管家",recipient:"写作",key:"repair",body:"写诗",
        limit:6,tool_message_id:None,report_to_user:None,
    }).unwrap();
    crate::db::butler_peer::fail(&conn,ex.id,"HTTP 400: unsupported temperature; conversation=20").unwrap();
    let view = resolve(&conn,10,"exchange",ex.id).unwrap();
    assert_eq!(view.result.as_deref(),Some("HTTP 400: unsupported temperature; conversation=20"));
    assert!(resolve(&conn,30,"exchange",ex.id).is_err());
}

#[test]
fn test_butler_work_startup_failure_reads_persisted_detail_without_final_message() {
    let conn = fixture();
    conn.execute("UPDATE butler_task_result SET final_message_id=NULL, summary='启动失败', structured_output_json=?1 WHERE task_conversation_id=41",
        [r#"{"content":"配置读取失败：assistant=2 model=missing；原执行未启动"}"#]).unwrap();
    conn.execute("UPDATE conversation SET butler_task_status='failed' WHERE id=41",[]).unwrap();
    let view = resolve(&conn,10,"task",41).unwrap();
    assert_eq!(view.result.as_deref(),Some("配置读取失败：assistant=2 model=missing；原执行未启动"));
}

/// 联系人 20（写作，助手 2）执行了两项独立任务 41/42；联系人 30（审稿，助手 3）是另一位助手。
fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("
        CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,name TEXT,conversation_kind TEXT,updated_time TEXT,
            butler_task_status TEXT,butler_task_finalized_at TEXT,butler_task_summary TEXT,created_time TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,message_type TEXT,content TEXT,metadata_json TEXT,
            created_time TEXT DEFAULT CURRENT_TIMESTAMP,llm_model_id INTEGER,llm_model_name TEXT,token_count INTEGER,
            input_token_count INTEGER,output_token_count INTEGER,finish_time TEXT,tool_calls_json TEXT);
        CREATE TABLE conversation_summary(conversation_id INTEGER);
        CREATE TABLE butler_task_definition(id INTEGER PRIMARY KEY,task_conversation_id INTEGER,butler_conversation_id INTEGER,
            executor_assistant_id INTEGER,title TEXT,goal TEXT,created_time TEXT,result_handling_mode TEXT);
        CREATE TABLE butler_task_result(task_conversation_id INTEGER,created_time TEXT,structured_output_json TEXT,summary TEXT,
            final_message_id INTEGER,handoff_message_id INTEGER);
        INSERT INTO conversation(id,assistant_id,name,conversation_kind,butler_task_status,butler_task_finalized_at) VALUES
            (10,1,'总管家','butler_main',NULL,NULL),(20,2,'写作','butler_contact',NULL,NULL),(30,3,'审稿','butler_contact',NULL,NULL),
            (41,2,'周报','butler_task','succeeded','2026-11-07 01:00:00'),(42,2,'月报','butler_task','running',NULL),
            (50,2,'普通聊天','normal',NULL,NULL);
        INSERT INTO butler_task_definition VALUES
            (1,41,10,2,'周报','写本周周报','2026-11-07 00:00:00','direct_report'),
            (2,42,10,2,'月报','写本月月报','2026-11-07 00:10:00','direct_report');
        INSERT INTO message(id,conversation_id,message_type,content) VALUES
            (1,41,'user','写本周周报'),(2,41,'response','周报：完成 A、B'),(3,42,'user','写本月月报');
        INSERT INTO butler_task_result VALUES (41,'2026-11-07 01:00:00',NULL,'周报摘要',2,NULL);
    ").unwrap();
    crate::db::butler_peer::schema(&conn).unwrap();
    schema(&conn).unwrap();
    conn
}

#[test]
fn test_butler_work_two_results_of_same_assistant_resolve_to_their_own_work() {
    let conn = fixture();
    let weekly = resolve(&conn, 20, "task", 41).unwrap();
    let monthly = resolve(&conn, 20, "task", 42).unwrap();
    assert_eq!((weekly.work_ref.as_str(), weekly.finished), ("task:41", true));
    assert_eq!(weekly.result.as_deref(), Some("周报：完成 A、B"));
    assert_eq!((monthly.work_ref.as_str(), monthly.status.as_str(), monthly.finished), ("task:42", "进行中", false));
    assert!(monthly.result.is_none(), "不串到另一份结果");
    // 派出任务的总管家会话也可引用；其他助手的会话、普通聊天不能访问。
    assert!(resolve(&conn, 10, "task", 41).is_ok());
    assert!(resolve(&conn, 30, "task", 41).unwrap_err().to_string().contains("无权访问"));
    assert!(resolve(&conn, 50, "task", 41).unwrap_err().to_string().contains("无权访问"));
}

#[test]
fn test_butler_work_message_quote_follows_real_turn_and_reports_missing_records() {
    let mut conn = fixture();
    // 写作会话：先是用户私聊一轮，再是总管家交来的一项委托。
    conn.execute_batch("INSERT INTO message(id,conversation_id,message_type,content) VALUES
        (100,10,'user','让写作写诗'),(101,20,'user','私聊：帮我改标题'),(102,20,'response','新标题：秋');").unwrap();
    let ex = crate::db::butler_peer::enqueue(&mut conn, crate::db::butler_peer::NewRequest {
        owner: 10, target: 20, sender: "总管家", recipient: "写作", key: "poem", body: "写一首秋天的诗",
        limit: 6, tool_message_id: None, report_to_user: None,
    }).unwrap();
    assert!(crate::db::butler_peer::claim(&conn, &ex).unwrap());
    crate::db::butler_peer::write_input(&mut conn, ex.id, "request", 20, "写一首秋天的诗", 1, "m").unwrap();
    conn.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES (200,20,'response','秋风起')", []).unwrap();

    let poem = resolve(&conn, 20, "message", 200).unwrap();
    assert_eq!(poem.work_ref, format!("exchange:{}", ex.id));
    assert_eq!(poem.excerpt.as_deref(), Some("秋风起"));
    assert_eq!(poem.requester.as_deref(), Some("总管家"));
    let title = resolve(&conn, 20, "message", 102).unwrap();
    assert_eq!(title.work_ref, "turn:101", "私聊的回复归属私聊那一轮，不串到委托");

    assert!(resolve(&conn, 30, "message", 200).unwrap_err().to_string().contains("不在当前会话"));
    assert!(resolve(&conn, 20, "message", 9999).unwrap_err().to_string().contains("已删除"));
    assert!(resolve(&conn, 20, "message", 101).unwrap_err().to_string().contains("助手的回复"));
    assert!(resolve(&conn, 20, "task", 77).unwrap_err().to_string().contains("已删除"));
    assert!(parse_ref("task:abc").is_err());
    assert_eq!(parse_ref("task:41").unwrap(), ("task", 41));
}

#[test]
fn test_butler_work_recent_lists_each_work_of_the_assistant() {
    let conn = fixture();
    let refs: Vec<_> = recent(&conn, 20, 5).unwrap().into_iter().map(|v| v.work_ref).collect();
    assert_eq!(refs, vec!["task:42", "task:41"], "按发生先后排列");
    assert!(recent(&conn, 30, 5).unwrap().is_empty());
}

/// 续接：相同请求重试不重复执行；同一任务按序送达，前一条处理中时下一条不会同时开始；重启后不重复执行。
#[test]
fn test_butler_work_followups_are_deduplicated_ordered_and_not_rerun_after_restart() {
    let mut conn = fixture();
    let (first, created) = enqueue_followup(&mut conn, 20, 42, "k1", "再加一节", Some(7)).unwrap();
    assert!(created);
    let (again, created) = enqueue_followup(&mut conn, 20, 42, "k1", "再加一节", Some(7)).unwrap();
    assert_eq!((again.id, created), (first.id, false), "重试不重复执行");
    assert!(enqueue_followup(&mut conn, 20, 42, "k1", "换个要求", Some(7)).is_err());
    let (second, _) = enqueue_followup(&mut conn, 20, 42, "k2", "改结论", Some(7)).unwrap();
    let (other, _) = enqueue_followup(&mut conn, 20, 41, "k1", "续写周报", Some(7)).unwrap();

    let ready: Vec<_> = next_followups(&conn).unwrap().into_iter().map(|f| f.id).collect();
    assert_eq!(ready, vec![first.id, other.id], "每个任务只取最早一条");
    assert!(claim_followup(&conn, first.id).unwrap());
    assert!(!claim_followup(&conn, second.id).unwrap(), "同一任务前一条未完成时不能同时送达");
    assert!(next_followups(&conn).unwrap().iter().all(|f| f.task_conversation_id != 42));
    finish_followup(&conn, first.id, "delivered", None, None).unwrap();
    assert_eq!(next_followups(&conn).unwrap()[0].id, second.id);

    assert!(claim_followup(&conn, other.id).unwrap());
    assert_eq!(recover_followups(&conn).unwrap(), 1);
    let lost = conn.query_row(&format!("{FOLLOWUP} WHERE id=?1"), [other.id], followup_row).unwrap();
    assert_eq!(lost.state, "uncertain");
    assert!(lost.error.unwrap().contains("未重复执行"));
    schema(&conn).unwrap();
}

#[test]
fn test_butler_followup_waits_for_spawned_execution_and_uses_latest_result() {
    let mut conn = fixture();
    let (first, _) = enqueue_followup(&mut conn,20,41,"one","first change",None).unwrap();
    let (second, _) = enqueue_followup(&mut conn,20,41,"two","second change",None).unwrap();
    assert!(claim_followup(&conn,first.id).unwrap());
    conn.execute("INSERT INTO conversation(id,assistant_id,conversation_kind,butler_task_status) VALUES (43,2,'butler_task','running')",[]).unwrap();
    assert!(bind_followup_task(&conn,first.id,43).unwrap());
    finish_followup(&conn,first.id,"spawned",Some(43),None).unwrap();
    assert!(next_followups(&conn).unwrap().is_empty());
    assert!(!claim_followup(&conn,second.id).unwrap());
    let (third,_) = enqueue_followup(&mut conn,20,43,"three","quote child",None).unwrap();
    assert!(!claim_followup(&conn,third.id).unwrap());
    conn.execute("UPDATE conversation SET butler_task_status='succeeded' WHERE id=43",[]).unwrap();
    assert_eq!(next_followups(&conn).unwrap().iter().map(|f| f.id).collect::<Vec<_>>(),vec![second.id]);
    assert_eq!(followup_target(&conn,&second).unwrap(),43);
    assert!(claim_followup(&conn,second.id).unwrap());
    assert!(!claim_followup(&conn,third.id).unwrap());
}

#[test]
fn test_butler_followup_cancellation_fences_claim_bind_and_late_finish() {
    let mut conn = fixture();
    let (first,_) = enqueue_followup(&mut conn,20,41,"one","change",Some(7)).unwrap();
    let (second,_) = enqueue_followup(&mut conn,20,41,"two","again",Some(7)).unwrap();
    let (unrelated,_) = enqueue_followup(&mut conn,20,42,"other","unrelated",Some(8)).unwrap();
    assert!(claim_followup(&conn,first.id).unwrap());
    crate::db::butler_peer::stop_chain(&conn,7).unwrap();
    assert!(!followup_is_sending(&conn,first.id).unwrap());
    assert!(!bind_followup_task(&conn,first.id,43).unwrap());
    finish_followup(&conn,first.id,"spawned",Some(43),None).unwrap();
    assert!(!claim_followup(&conn,second.id).unwrap());
    assert_eq!(next_followups(&conn).unwrap().iter().map(|f| f.id).collect::<Vec<_>>(),vec![unrelated.id]);
}

#[test]
fn test_butler_followup_cancel_after_binding_stops_descendant_queue() {
    let mut conn = fixture();
    let (first,_) = enqueue_followup(&mut conn,20,41,"one","change",None).unwrap();
    assert!(claim_followup(&conn,first.id).unwrap());
    conn.execute("INSERT INTO conversation(id,assistant_id,conversation_kind,butler_task_status) VALUES (43,2,'butler_task','accepted')",[]).unwrap();
    assert!(bind_followup_task(&conn,first.id,43).unwrap());
    let (child,_) = enqueue_followup(&mut conn,20,43,"two","again",None).unwrap();
    crate::db::butler_peer::stop_owned(&conn,41).unwrap();
    assert_eq!(lineage_tasks(&conn,43).unwrap(),vec![41,43]);
    assert!(!followup_is_sending(&conn,first.id).unwrap());
    assert!(!claim_followup(&conn,child.id).unwrap());
    finish_followup(&conn,first.id,"spawned",Some(43),None).unwrap();
    assert!(next_followups(&conn).unwrap().is_empty());
}

#[test]
fn test_butler_followup_restart_keeps_known_child_and_blocks_unknown_admission() {
    let mut conn = fixture();
    let (first,_) = enqueue_followup(&mut conn,20,41,"one","change",None).unwrap();
    let (second,_) = enqueue_followup(&mut conn,20,41,"two","again",None).unwrap();
    assert!(claim_followup(&conn,first.id).unwrap());
    assert_eq!(recover_followups(&conn).unwrap(),1);
    assert!(!claim_followup(&conn,second.id).unwrap(),"unknown admission must not restart work automatically");
    stop_task_followups(&conn,41).unwrap();
    let (known,_) = enqueue_followup(&mut conn,20,42,"known","change",None).unwrap();
    assert!(claim_followup(&conn,known.id).unwrap());
    conn.execute("INSERT INTO conversation(id,assistant_id,conversation_kind,butler_task_status) VALUES (44,2,'butler_task','running')",[]).unwrap();
    assert!(bind_followup_task(&conn,known.id,44).unwrap());
    recover_followups(&conn).unwrap();
    let (later,_) = enqueue_followup(&mut conn,20,42,"later","again",None).unwrap();
    assert!(!claim_followup(&conn,later.id).unwrap());
    conn.execute("UPDATE conversation SET butler_task_status='succeeded' WHERE id=44",[]).unwrap();
    assert!(claim_followup(&conn,later.id).unwrap());
    assert_eq!(followup_target(&conn,&later).unwrap(),44);
}

#[test]
fn test_butler_followup_late_enqueue_after_stop_is_rejected() {
    let mut conn = fixture();
    crate::db::butler_peer::stop_owned(&conn,42).unwrap();
    assert!(enqueue_followup(&mut conn,20,42,"late-answer","answer",None).unwrap_err().to_string().contains("已停止"));
    assert!(enqueue_followup(&mut conn,20,41,"other","unrelated",None).is_ok());
}

#[test]
fn test_butler_followup_subtree_stop_preserves_sibling_requests() {
    let mut conn = fixture();
    conn.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES (100,10,'user','parallel help')",[]).unwrap();
    let a = crate::db::butler_peer::enqueue(&mut conn, crate::db::butler_peer::NewRequest {
        owner:10,target:20,sender:"总管家",recipient:"写作",key:"a",body:"change weekly",limit:6,tool_message_id:None,report_to_user:None
    }).unwrap();
    assert!(crate::db::butler_peer::claim(&conn,&a).unwrap());
    crate::db::butler_peer::write_input(&mut conn,a.id,"request",20,"change weekly",1,"m").unwrap();
    let (first,_) = enqueue_followup(&mut conn,20,41,"a","change weekly",Some(100)).unwrap();
    let b = crate::db::butler_peer::enqueue(&mut conn, crate::db::butler_peer::NewRequest {
        owner:10,target:30,sender:"总管家",recipient:"审稿",key:"b",body:"change monthly",limit:6,tool_message_id:None,report_to_user:None
    }).unwrap();
    assert!(crate::db::butler_peer::claim(&conn,&b).unwrap());
    crate::db::butler_peer::write_input(&mut conn,b.id,"request",30,"change monthly",1,"m").unwrap();
    let (sibling,_) = enqueue_followup(&mut conn,30,42,"b","change monthly",Some(100)).unwrap();
    crate::db::butler_peer::stop_subtree(&conn,a.id).unwrap();
    assert!(!claim_followup(&conn,first.id).unwrap());
    assert!(claim_followup(&conn,sibling.id).unwrap(),"parallel sibling must remain usable");
    conn.execute("UPDATE message SET metadata_json=?1 WHERE id=(SELECT max(id) FROM message WHERE conversation_id=20 AND message_type='user')",
        [format!("{{\"origin\":\"butler_peer\",\"root\":100,\"exchange_id\":{},\"leg\":\"request\"}}",a.id)]).unwrap();
    assert!(enqueue_followup(&mut conn,20,41,"late","late request",Some(100)).is_err());
}

#[test]
fn test_butler_followup_inplace_stop_tracks_exact_turn_not_later_task_work() {
    let mut conn = fixture();
    let (first,_) = enqueue_followup(&mut conn,20,42,"inplace","change",None).unwrap();
    conn.execute("UPDATE butler_work_followup SET origin_exchange=7 WHERE id=?1",[first.id]).unwrap();
    assert!(claim_followup(&conn,first.id).unwrap());
    conn.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES (300,42,'user','change')",[]).unwrap();
    assert!(bind_followup_input(&conn,first.id,42,300).unwrap());
    finish_followup(&conn,first.id,"delivered",None,None).unwrap();
    assert_eq!(exchange_followup_inputs(&conn,&[7]).unwrap(),vec![(42,300)]);
    conn.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES (301,42,'user','unrelated newer turn')",[]).unwrap();
    assert!(exchange_followup_inputs(&conn,&[7]).unwrap().is_empty(),"do not halt newer task activity");
    assert!(!followup_input_is_current(&conn,42,300).unwrap());
    stop_exchange_followups(&conn,&[7]).unwrap();
    assert!(!bind_followup_input(&conn,first.id,42,302).unwrap());
}
