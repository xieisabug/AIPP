use super::*;
use crate::db::butler_group_delivery as delivery;

fn setup()->(Connection,group::GroupView) {
    let mut c=Connection::open_in_memory().unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE conversation(id INTEGER PRIMARY KEY,name TEXT,assistant_id INTEGER,conversation_kind TEXT,parent_butler_conversation_id INTEGER,is_hidden_from_normal_chat_list INTEGER,updated_time TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,parent_id INTEGER,message_type TEXT,content TEXT,metadata_json TEXT,token_count INTEGER,input_token_count INTEGER,output_token_count INTEGER,finish_time TEXT,tool_calls_json TEXT);").unwrap();
    group::schema(&c).unwrap();
    let room=group::create(&mut c,"群","目标",(1,"甲".into()),&[(2,"乙".into())]).unwrap();(c,room)
}
fn profile()->Profile {
    Profile {dynamic_loading:false,native_toolcall:true,error:None,servers:vec![Server {id:7,name:"文件".into(),summary:"读取授权文件".into(),tools:vec![
        Tool {name:"read".into(),description:"读取文件".into(),auto_run:false,parameters:serde_json::json!({"type":"object"})},
        Tool {name:"list".into(),description:"列出文件".into(),auto_run:true,parameters:serde_json::json!({"type":"object"})},
    ]}]}
}

#[test]
fn test_group_capabilities_append_without_wakeup_or_rewriting_prepared_prefix() {
    let (mut c,room)=setup();let m=&room.members[0];
    assert!(record(&mut c,m.id,profile()).unwrap());
    let original=group::events(&c,room.group.id,0,200).unwrap();
    assert!(delivery::inbox(&c,room.group.id).unwrap().is_empty());
    let mut reordered=profile();reordered.servers[0].tools.reverse();
    assert!(!record(&mut c,m.id,reordered).unwrap());
    group::post(&mut c,room.group.id,None,&group::Post {request_key:"work".into(),kind:group::MessageKind::Message,body:"处理工作".into(),reply_to:None,reference_only:false,recipients:vec![m.id]}).unwrap();
    let packet=delivery::prepare_execution(&mut c,m.conversation_id).unwrap().unwrap();
    let mut changed=profile();changed.servers[0].tools.pop();
    assert!(record(&mut c,m.id,changed).unwrap());
    assert_eq!(delivery::packet(&c,packet.id).unwrap().payload,packet.payload);
    let events=group::events(&c,room.group.id,0,200).unwrap();
    for old in original {assert_eq!(events.iter().find(|e|e.seq==old.seq).unwrap().body,old.body);}
    assert!(events.last().unwrap().body.contains("已变更"));
    assert_eq!(delivery::inbox(&c,room.group.id).unwrap().len(),1);
}

#[test]
fn test_group_capabilities_failure_and_recovery_are_explicit_and_deduplicated() {
    let (mut c,room)=setup();let m=&room.members[0];
    let broken=Profile {error:Some("stage=tool_schema; server=7; malformed JSON".into()),dynamic_loading:false,native_toolcall:false,servers:vec![]};
    assert!(record(&mut c,m.id,broken.clone()).unwrap());
    assert!(!record(&mut c,m.id,broken).unwrap());
    let events=group::events(&c,room.group.id,0,200).unwrap();
    assert!(events.last().unwrap().body.contains("能力未确认"));
    assert!(!events.last().unwrap().body.contains("当前没有启用"));
    assert!(record(&mut c,m.id,profile()).unwrap());
    assert!(group::events(&c,room.group.id,0,200).unwrap().last().unwrap().body.contains("已变更"));
}

#[test]
fn test_group_capabilities_rejoin_has_new_identity_and_cannot_read_old_profile_event() {
    let (mut c,room)=setup();let m=&room.members[1];
    record(&mut c,m.id,profile()).unwrap();
    let old=group::get(&c,room.group.id).unwrap().seq;
    group::remove_member(&mut c,room.group.id,m.id).unwrap();
    let joined=group::add_member(&mut c,room.group.id,m.assistant_id,&m.name).unwrap();
    assert!(record(&mut c,joined.id,profile()).unwrap());
    assert!(record(&mut c,m.id,profile()).is_err());
    assert!(group::read_as_member(&c,joined.id,old).is_err());
}
