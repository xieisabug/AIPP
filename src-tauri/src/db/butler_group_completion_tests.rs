use super::*;
use crate::db::butler_group as group;

fn setup()->(Connection,delivery::Packet,i64) {
    let mut c=Connection::open_in_memory().unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE conversation(id INTEGER PRIMARY KEY,name TEXT,assistant_id INTEGER,conversation_kind TEXT,parent_butler_conversation_id INTEGER,is_hidden_from_normal_chat_list INTEGER,updated_time TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,parent_id INTEGER,message_type TEXT,content TEXT,metadata_json TEXT,token_count INTEGER,input_token_count INTEGER,output_token_count INTEGER,finish_time TEXT,tool_calls_json TEXT);").unwrap();
    group::schema(&c).unwrap();
    let room=group::create(&mut c,"群","目标",(1,"甲".into()),&[]).unwrap();
    let m=&room.members[0];
    group::post(&mut c,room.group.id,None,&group::Post{request_key:"root".into(),kind:group::MessageKind::Message,body:"原工作".into(),reply_to:None,reference_only:false,recipients:vec![m.id]}).unwrap();
    let p=delivery::prepare_execution(&mut c,m.conversation_id).unwrap().unwrap();
    let input=delivery::activate(&mut c,p.id,m.conversation_id,m.assistant_id,&p.payload).unwrap();
    (c,p,input)
}
fn response(c:&Connection,p:&delivery::Packet,parent:Option<i64>,body:&str,finished:bool)->i64 {
    c.execute("INSERT INTO message(conversation_id,parent_id,message_type,content,finish_time) VALUES(?1,?2,'response',?3,CASE WHEN ?4 THEN CURRENT_TIMESTAMP END)",
        params![p.conversation_id,parent,body,finished]).unwrap();c.last_insert_rowid()
}

#[test]
fn test_group_completion_requires_original_tree_and_persisted_finish() {
    let (c,p,input)=setup();
    let unrelated=response(&c,&p,None,"无关错误",true);
    c.execute("UPDATE message SET message_type='error' WHERE id=?1",[unrelated]).unwrap();
    assert_eq!(evidence(&c,p.id).unwrap(),Evidence::Pending);
    let answer=response(&c,&p,Some(input),"尚未写完",false);
    assert_eq!(evidence(&c,p.id).unwrap(),Evidence::Pending);
    c.execute("UPDATE message SET finish_time=CURRENT_TIMESTAMP WHERE id=?1",[answer]).unwrap();
    assert_eq!(evidence(&c,p.id).unwrap(),Evidence::Answer(answer));
    // A newer unrelated terminal record cannot replace the original answer.
    response(&c,&p,None,"别的分支结束",true);
    assert_eq!(evidence(&c,p.id).unwrap(),Evidence::Answer(answer));
}

#[test]
fn test_group_completion_tool_continuation_is_not_final_delivery() {
    let (c,p,input)=setup();
    let answer=response(&c,&p,Some(input),"我会继续工具操作",true);
    c.execute("UPDATE message SET tool_calls_json='[{\"id\":\"call-1\"}]' WHERE id=?1",[answer]).unwrap();
    assert_eq!(evidence(&c,p.id).unwrap(),Evidence::Pending);
    let tool=response(&c,&p,Some(answer),"工具结果",true);
    c.execute("UPDATE message SET message_type='tool_result' WHERE id=?1",[tool]).unwrap();
    assert_eq!(evidence(&c,p.id).unwrap(),Evidence::Pending);
    let final_id=response(&c,&p,Some(tool),"完整交付",true);
    assert_eq!(evidence(&c,p.id).unwrap(),Evidence::Answer(final_id));
}

#[test]
fn test_group_completion_explicit_error_preserves_original_diagnostics() {
    let (c,p,input)=setup();
    let error=response(&c,&p,Some(input),"stage=stdout; run=r-17; child exited 23; stderr=transport closed",true);
    c.execute("UPDATE message SET message_type='error' WHERE id=?1",[error]).unwrap();
    let Evidence::Failed(reason)=evidence(&c,p.id).unwrap() else {panic!("missing explicit failure")};
    assert!(reason.contains("child exited 23"));assert!(reason.contains("run=r-17"));
}

#[test]
fn test_group_completion_silent_is_recorded_without_fake_failure() {
    let (mut c,p,input)=setup();
    let answer=response(&c,&p,Some(input),"[[NO_REPLY]]",true);
    delivery::finish_silent(&mut c,p.id,answer).unwrap();
    assert_eq!(delivery::packet(&c,p.id).unwrap().state,"completed");
    let group_id:i64=c.query_row("SELECT group_id FROM butler_group_member WHERE id=?1",[p.member_id],|r|r.get(0)).unwrap();
    assert!(group::events(&c,group_id,0,200).unwrap().iter().any(|e|e.body.contains("明确选择不发言")));
}

#[test]
fn test_group_completion_unknown_actor_fences_late_success_without_replay() {
    let (mut c,p,input)=setup();
    delivery::uncertain(&mut c,p.id,"run=r-17; actor missing; no terminal record").unwrap();
    assert_eq!(delivery::packet(&c,p.id).unwrap().state,"uncertain");
    let late=response(&c,&p,Some(input),"迟到成功",true);
    assert!(delivery::publish_final(&mut c,p.id,late).is_err());
    assert!(delivery::prepare_execution(&mut c,p.conversation_id).is_err());
}
