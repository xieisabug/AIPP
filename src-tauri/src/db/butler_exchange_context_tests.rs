use super::*;
use crate::db::{butler_peer, butler_timeline};

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("
        CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,conversation_kind TEXT,butler_task_status TEXT,butler_task_finalized_at TEXT,butler_task_summary TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,message_type TEXT,metadata_json TEXT,created_time TEXT,content TEXT);
        CREATE TABLE butler_task_definition(id INTEGER PRIMARY KEY,task_conversation_id INTEGER,butler_conversation_id INTEGER,executor_assistant_id INTEGER,goal TEXT,created_time TEXT,result_handling_mode TEXT);
        CREATE TABLE butler_task_result(task_conversation_id INTEGER,created_time TEXT,structured_output_json TEXT,summary TEXT,final_message_id INTEGER,handoff_message_id INTEGER);
        INSERT INTO conversation VALUES (10,1,'butler_main',NULL,NULL,NULL),(20,2,'butler_contact',NULL,NULL,NULL),(30,3,'butler_contact',NULL,NULL,NULL),(40,2,'butler_task','failed','2026-11-05 01:00:03','401 Unauthorized');
        INSERT INTO message VALUES (1,10,'user',NULL,'2026-11-05 01:00:00','请迭代两轮'),(2,10,'user',NULL,'2026-11-05 01:00:10','另一次委托');
    ").unwrap();
    butler_peer::schema(&conn).unwrap();
    conn.execute_batch("INSERT INTO butler_peer_exchange(id,root,owner,target,sender,recipient,request_key,body,state,reply,round_no,max_rounds) VALUES
        (1,1,10,20,'总管家','审稿','a','第一稿','delivered','请修改结尾',1,2),
        (2,1,10,20,'总管家','审稿','b','第二稿','delivered','通过',2,2),
        (3,2,10,20,'总管家','审稿','c','不相关正文','failed',NULL,1,2);").unwrap();
    butler_timeline::record(&conn,1,10,"request",true,1).unwrap();
    butler_timeline::record(&conn,2,10,"reply",false,1).unwrap();
    butler_timeline::record(&conn,2,20,"reply",true,1).unwrap();
    butler_timeline::record(&conn,3,10,"request",true,2).unwrap();
    conn
}

#[test]
fn test_butler_exchange_context_exact_root_from_either_side() {
    let conn = fixture();
    for (conversation, post) in [(10,2),(20,3)] {
        let episodes = context(&conn,&HashMap::new(),conversation,&[post]).unwrap();
        assert_eq!(episodes.len(),1);
        assert_eq!(episodes[0].prompt.as_deref(),Some("请迭代两轮"));
        assert_eq!(episodes[0].messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),vec!["第一稿","请修改结尾","第二稿","通过"]);
        assert_eq!(episodes[0].messages[3].sender,"审稿");
    }
    assert!(context(&conn,&HashMap::new(),30,&[2]).is_err());
    assert!(context(&conn,&HashMap::new(),10,&[999]).is_err());
}

#[test]
fn test_butler_exchange_context_task_prompt_and_errors_stay_in_records() {
    let conn = fixture();
    conn.execute_batch("INSERT INTO message VALUES(3,10,'user','{\"origin\":\"internal\"}','2026-11-05 01:00:01','内部通知');
        INSERT INTO butler_task_definition VALUES(1,40,10,2,'写个早安','2026-11-05 01:00:02',NULL);").unwrap();
    let names = HashMap::from([(1,"总管家".into()),(2,"审稿".into())]);
    for conversation in [10,20] {
        let episodes = context(&conn,&names,conversation,&[(1_i64<<40)+40*2+1]).unwrap();
        assert_eq!(episodes[0].prompt.as_deref(),Some("请迭代两轮"));
        assert_eq!(episodes[0].messages.len(),1);
        assert_eq!(episodes[0].messages[0].text,"写个早安");
        assert_eq!(episodes[0].status,"failed");
        assert!(!serde_json::to_string(&episodes).unwrap().contains("401"));
    }
    let diagnostic: String = conn.query_row("SELECT butler_task_summary FROM conversation WHERE id=40",[],|r| r.get(0)).unwrap();
    assert_eq!(diagnostic,"401 Unauthorized");
}
