use super::*;
use crate::db::butler_peer;

fn database() -> Connection {
    let conn=Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,conversation_kind TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,message_type TEXT,content TEXT,metadata_json TEXT);
        INSERT INTO conversation VALUES (10,1,'butler_main'),(20,2,'butler_contact'),(30,3,'butler_contact');").unwrap();
    butler_peer::schema(&conn).unwrap();
    conn.execute_batch("INSERT INTO butler_peer_exchange(id,root,owner,target,sender,recipient,request_key,body,state,round_no,max_rounds)
        VALUES (1,100,10,20,'总管家','写作','one','初稿','delivered',1,6),
        (2,100,10,20,'总管家','写作','two','修订','waiting',2,6),
        (3,200,20,30,'写作','审稿','three','另一个任务','queued',1,6);").unwrap();
    conn
}

#[test]
fn test_assistant_exchange_query_is_read_only_scoped_and_paginated() {
    let conn=database();
    conn.execute_batch("PRAGMA query_only=ON").unwrap();
    let first=exchange_tool(&conn,10,"get_assistant_exchange",&json!({"limit":1})).unwrap();
    assert_eq!(first["exchanges"][0]["exchange_id"],2);
    assert!(first["exchanges"][0]["request"].is_null());
    assert_eq!(first["next_before_id"],2);
    let next=exchange_tool(&conn,10,"get_assistant_exchange",&json!({"before_id":2,"include_context":true})).unwrap();
    assert_eq!(next["exchanges"].as_array().unwrap().len(),1);
    assert_eq!(next["exchanges"][0]["request"],"初稿");
    assert!(exchange_tool(&conn,30,"get_assistant_exchange",&json!({"exchange_id":1})).is_err());
    assert!(exchange_tool(&conn,10,"get_assistant_exchange",&json!({"exchange_id":999})).is_err());
}

#[test]
fn test_assistant_exchange_stop_is_idempotent_and_preserves_other_work() {
    let conn=database();
    let args=json!({"exchange_id":2,"reason":"用户要求停止本轮审稿"});
    assert!(exchange_tool(&conn,30,"stop_assistant_exchange",&args).is_err());
    let result=exchange_tool(&conn,10,"stop_assistant_exchange",&args).unwrap();
    assert_eq!(result["execution_cancelled"],false);
    exchange_tool(&conn,10,"stop_assistant_exchange",&args).unwrap();
    let records=butler_peer::list(&conn,Some(20),true).unwrap();
    assert_eq!(records.iter().find(|r|r.id==1).unwrap().state,"delivered");
    assert_eq!(records.iter().find(|r|r.id==2).unwrap().state,"stopped");
    assert_eq!(records.iter().find(|r|r.id==3).unwrap().state,"queued");
    assert!(!butler_peer::claim(&conn,records.iter().find(|r|r.id==2).unwrap()).unwrap());
    let count:i64=conn.query_row("SELECT count(*) FROM butler_peer_stop",[],|r|r.get(0)).unwrap();
    assert_eq!(count,1);
    let details=exchange_tool(&conn,10,"get_assistant_exchange",&json!({"exchange_id":2})).unwrap();
    assert_eq!(details["stop_reason"],args["reason"]);
}

#[test]
fn test_assistant_details_validate_sections_and_bounds() {
    assert_eq!(sections(&json!({})).unwrap(),vec!["profile","summary","runtime"]);
    assert!(sections(&json!({"sections":["credentials"]})).is_err());
    assert!(sections(&json!({"sections":"prompt"})).is_err());
    assert!(limit(&json!({"limit":0})).is_err());
    assert!(limit(&json!({"limit":500})).is_err());
    assert!(number(&json!({"offset":-1}),"offset",0,100).is_err());
    let result=page(vec![json!("a"),json!("b"),json!("c")],1,1);
    assert_eq!(result["items"],json!(["b"]));assert_eq!(result["next_offset"],2);
}
