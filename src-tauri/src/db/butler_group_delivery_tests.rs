use super::*;

fn setup() -> (Connection,group::GroupView) {
    let mut conn=Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE conversation(id INTEGER PRIMARY KEY,name TEXT,assistant_id INTEGER,conversation_kind TEXT,
            parent_butler_conversation_id INTEGER,is_hidden_from_normal_chat_list INTEGER,updated_time TEXT,butler_task_status TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,parent_id INTEGER,message_type TEXT,content TEXT,
            metadata_json TEXT,token_count INTEGER,input_token_count INTEGER,output_token_count INTEGER,finish_time TEXT,tool_calls_json TEXT);").unwrap();
    group::schema(&conn).unwrap();
    let room=group::create(&mut conn,"讨论","共同目标",(1,"总管家".into()),&[(2,"甲".into()),(3,"乙".into())]).unwrap();
    (conn,room)
}

fn post(key:&str,body:&str,recipients:Vec<i64>) -> group::Post {
    group::Post { request_key:key.into(),body:body.into(),kind:group::MessageKind::Message,reply_to:None,reference_only:false,recipients }
}

fn answer(conn:&Connection,member:&group::Member,input:i64,body:&str) -> i64 {
    conn.execute("INSERT INTO message(conversation_id,parent_id,message_type,content,finish_time) VALUES(?1,?2,'response',?3,CURRENT_TIMESTAMP)",
        params![member.conversation_id,input,body]).unwrap();
    conn.last_insert_rowid()
}

fn long_background(c:&mut Connection,room:&group::GroupView) {
    for index in 0..14 {
        let mut entry=post(&format!("background-{index}"),&format!("公开结果 {index}: {}","资料".repeat(6000)),vec![]);
        entry.kind=group::MessageKind::Result;
        group::post(c,room.group.id,Some(room.members[2].id),&entry).unwrap();
    }
}

#[test]
fn test_group_background_summary_freezes_sources_and_preserves_required_text() {
    let (mut c,room)=setup(); let a=&room.members[1];
    long_background(&mut c,&room);
    let mut request=post("summary-request","必须逐项核对，不改变原要求",vec![a.id]);
    request.reply_to=Some(1);
    group::post(&mut c,room.group.id,None,&request).unwrap();
    let frozen_upper=group::get(&c,room.group.id).unwrap().seq;
    let err=prepare(&mut c,a.id).unwrap_err();
    let preparation=err.downcast_ref::<SummaryRequired>().unwrap().0;
    group::post(&mut c,room.group.id,None,&post("after-freeze","准备期间新到达的要求",vec![a.id])).unwrap();
    let again=prepare(&mut c,a.id).unwrap_err();
    assert_eq!(again.downcast_ref::<SummaryRequired>().unwrap().0,preparation);
    for chunk in summary_chunks(&c,preparation).unwrap() {
        assert!(!chunk.source.contains("必须逐项核对"));
        assert!(!chunk.source.contains("准备期间新到达"));
        let fragments:Vec<serde_json::Value>=serde_json::from_str(&chunk.source).unwrap();
        assert!(fragments.iter().all(|f|f["seq"].as_i64()!=Some(1)),"指定引用不可被摘要替换");
        save_summary_chunk(&mut c,preparation,chunk.id,"来源公开结果的摘要","summary-model",3).unwrap();
        assert!(save_summary_chunk(&mut c,preparation,chunk.id,"覆盖已保存摘要","summary-model",3).is_err());
    }
    let p=finish_summary(&mut c,preparation).unwrap();
    assert_eq!(p.through_seq,frozen_upper);
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,0);
    assert!(p.payload.contains("必须逐项核对，不改变原要求"));
    assert!(p.payload.contains("source_sequences"));
    assert!(p.payload.contains("公开结果 0:"));
    assert!(!p.payload.contains("准备期间新到达的要求"));
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    assert_eq!(c.query_row("SELECT content FROM message WHERE id=?1",[input],|r|r.get::<_,String>(0)).unwrap(),p.payload);
    assert!(inbox(&c,room.group.id).unwrap().iter().any(|i|i.member_id==a.id && i.state=="queued"));
}

#[test]
fn test_group_background_summary_allocates_packet_budget_before_model_output() {
    let (mut c,room)=setup(); let a=&room.members[1];
    long_background(&mut c,&room);
    group::post(&mut c,room.group.id,None,&post("budget-request","保持用户要求原文",vec![a.id])).unwrap();
    let error=prepare(&mut c,a.id).unwrap_err();
    let id=error.downcast_ref::<SummaryRequired>().unwrap().0;
    let limit=summary_char_limit(&c,id,"summary-model",3).unwrap();
    assert!((256..4000).contains(&limit));
    let chunks=summary_chunks(&c,id).unwrap();
    assert!(save_summary_chunk(&mut c,id,chunks[0].id,&"字".repeat(limit+1),"summary-model",3).is_err());
    assert!(summary_chunks(&c,id).unwrap()[0].summary.is_none());
    // A control character expands to six JSON characters. Even that worst
    // case must fit without dropping required text or provenance metadata.
    for chunk in chunks {
        save_summary_chunk(&mut c,id,chunk.id,&"\u{0001}".repeat(limit),"summary-model",3).unwrap();
    }
    let packet=finish_summary(&mut c,id).unwrap();
    assert!(packet.payload.chars().count()<=120000);
    assert!(packet.payload.contains("保持用户要求原文"));
    assert!(packet.payload.contains("source_sequences"));
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,0);
}

#[test]
fn test_group_background_summary_stop_rejects_late_model_response() {
    let (mut c,room)=setup(); let a=&room.members[1];
    long_background(&mut c,&room);
    group::post(&mut c,room.group.id,None,&post("summary-stop","请处理",vec![a.id])).unwrap();
    let err=prepare(&mut c,a.id).unwrap_err();
    let id=err.downcast_ref::<SummaryRequired>().unwrap().0;
    let chunk=summary_chunks(&c,id).unwrap().remove(0);
    group::stop_member(&mut c,room.group.id,a.id).unwrap();
    assert!(save_summary_chunk(&mut c,id,chunk.id,"迟到的摘要","summary-model",3).is_err());
    assert!(finish_summary(&mut c,id).is_err());
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,0);
    assert_eq!(c.query_row("SELECT count(*) FROM message",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}

#[test]
fn test_group_background_summary_failure_ends_only_frozen_batch() {
    let (mut c,room)=setup(); let a=&room.members[1];
    long_background(&mut c,&room);
    group::post(&mut c,room.group.id,None,&post("summary-fail","原请求",vec![a.id])).unwrap();
    let err=prepare(&mut c,a.id).unwrap_err();
    let id=err.downcast_ref::<SummaryRequired>().unwrap().0;
    let later=group::post(&mut c,room.group.id,None,&post("later-request","后续请求",vec![a.id])).unwrap();
    fail_summary(&mut c,id,"stage=summary; provider=3; HTTP 401").unwrap();
    let items=inbox(&c,room.group.id).unwrap();
    assert!(items.iter().any(|i|i.member_id==a.id && i.state=="failed" && i.error.as_deref().unwrap().contains("HTTP 401")));
    assert!(items.iter().any(|i|i.event_seq==later.seq && i.state=="queued"));
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,0);
}

#[test]
fn test_group_tool_boundary_appends_without_rewriting_packet_or_crossing_members() {
    let (mut c,room)=setup(); let a=&room.members[1]; let b=&room.members[2];
    group::post(&mut c,room.group.id,None,&post("start","开始工作",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    let call=answer(&c,a,input,"工具调用");
    c.execute("INSERT INTO message(conversation_id,parent_id,message_type,content,finish_time) VALUES(?1,?2,'tool_result','private-result',CURRENT_TIMESTAMP)",params![a.conversation_id,call]).unwrap();
    let tool_result=c.last_insert_rowid();
    group::post(&mut c,room.group.id,None,&post("supplement","当前工作补充要求",vec![a.id])).unwrap();
    group::post(&mut c,room.group.id,None,&post("other-member","另一成员的独立请求",vec![b.id])).unwrap();
    let supplemental=append_tool_boundary(&mut c,a.conversation_id,call).unwrap().unwrap();
    assert!(supplemental>tool_result);
    let content:String=c.query_row("SELECT content FROM message WHERE id=?1",[supplemental],|r|r.get(0)).unwrap();
    let json:serde_json::Value=serde_json::from_str(content.lines().last().unwrap()).unwrap();
    assert_eq!(json["required"].as_array().unwrap().len(),1);
    assert_eq!(json["required"][0]["message"]["body"],"当前工作补充要求");
    assert!(!content.contains("private-result"));
    assert_eq!(packet(&c,p.id).unwrap().payload,p.payload);
    assert_eq!(c.query_row("SELECT content FROM message WHERE id=?1",[input],|r|r.get::<_,String>(0)).unwrap(),p.payload);
    assert!(append_tool_boundary(&mut c,a.conversation_id,call).unwrap().is_none());
    let output=answer(&c,a,supplemental,"处理完原请求及补充");
    publish_final(&mut c,p.id,output).unwrap();
    let items=inbox(&c,room.group.id).unwrap();
    assert!(items.iter().filter(|i|i.member_id==a.id).all(|i|i.state=="answered"));
    assert!(items.iter().filter(|i|i.member_id==b.id).all(|i|i.state=="queued"));
}

#[test]
fn test_group_independent_opinion_does_not_receive_mid_turn_supplement() {
    let (mut c,room)=setup(); let a=&room.members[1]; let b=&room.members[2];
    post_independent(&mut c,room.group.id,&post("independent-boundary","独立判断",vec![]),&[a.id,b.id]).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    group::post(&mut c,room.group.id,None,&post("late-opinion","其他人的新观点",vec![a.id])).unwrap();
    assert!(append_tool_boundary(&mut c,a.conversation_id,input).unwrap().is_none());
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,p.through_seq);
    assert!(inbox(&c,room.group.id).unwrap().iter().any(|i|i.member_id==a.id && i.state=="queued"));
}

#[test]
fn test_group_native_input_advances_only_after_matching_ack_once() {
    use crate::db::butler_group_native as native;
    let (mut c,room)=setup(); let a=&room.members[1];
    group::post(&mut c,room.group.id,None,&post("native-start","开始",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    group::post(&mut c,room.group.id,None,&post("native-supplement","补充",vec![a.id])).unwrap();
    let planned=native::reserve(&mut c,a.conversation_id,input,"run","thread","turn").unwrap().unwrap();
    group::post(&mut c,room.group.id,None,&post("native-later","更晚的要求",vec![a.id])).unwrap();
    assert!(!planned.payload.contains("更晚的要求"));
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,p.through_seq);
    assert!(native::acknowledge(&mut c,planned.id,"run","thread","wrong-turn").is_err());
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,p.through_seq);
    let message=native::acknowledge(&mut c,planned.id,"run","thread","turn").unwrap();
    assert_eq!(native::acknowledge(&mut c,planned.id,"run","thread","turn").unwrap(),message);
    assert_eq!(c.query_row("SELECT content FROM message WHERE id=?1",[message],|r|r.get::<_,String>(0)).unwrap(),planned.payload);
    assert_eq!(packet(&c,p.id).unwrap().payload,p.payload);
    assert_eq!(c.query_row("SELECT count(*) FROM butler_group_supplement",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    assert!(inbox(&c,room.group.id).unwrap().iter().any(|i|i.member_id==a.id && i.state=="queued"));
}

#[test]
fn test_group_native_rejection_fails_reserved_items_without_advancing_or_replay() {
    use crate::db::butler_group_native as native;
    let (mut c,room)=setup(); let a=&room.members[1];
    group::post(&mut c,room.group.id,None,&post("reject-start","开始",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    group::post(&mut c,room.group.id,None,&post("reject-supplement","补充",vec![a.id])).unwrap();
    let planned=native::reserve(&mut c,a.conversation_id,input,"run","thread","turn").unwrap().unwrap();
    let later=group::post(&mut c,room.group.id,None,&post("reject-later","之后的新要求",vec![a.id])).unwrap();
    native::rejected(&mut c,planned.id,"stage=codex_group_steer_rejected; active turn mismatch").unwrap();
    assert_eq!(packet(&c,p.id).unwrap().state,"failed");
    assert_eq!(packet(&c,p.id).unwrap().payload,p.payload);
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,p.through_seq);
    let items=inbox(&c,room.group.id).unwrap();
    assert!(items.iter().filter(|i|i.packet_id==Some(p.id)).all(|i|i.state=="failed"));
    assert!(items.iter().any(|i|i.event_seq==later.seq && i.member_id==a.id && i.state=="queued"));
    assert!(native::acknowledge(&mut c,planned.id,"run","thread","turn").is_err());
    native::uncertain(&mut c,planned.id,"late transport error").unwrap();
    native::rejected(&mut c,planned.id,"duplicate error").unwrap();
    assert_eq!(packet(&c,p.id).unwrap().state,"failed");
    assert_eq!(c.query_row("SELECT count(*) FROM butler_group_supplement",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(c.query_row("SELECT error FROM butler_group_native_input WHERE id=?1",[planned.id],|r|r.get::<_,String>(0)).unwrap(),"stage=codex_group_steer_rejected; active turn mismatch");
}

#[test]
fn test_group_native_lost_ack_is_fenced_across_restart_without_resend() {
    use crate::db::butler_group_native as native;
    let (mut c,room)=setup(); let a=&room.members[1];
    group::post(&mut c,room.group.id,None,&post("native-unknown","开始",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    group::post(&mut c,room.group.id,None,&post("native-queued","待确认的补充",vec![a.id])).unwrap();
    let planned=native::reserve(&mut c,a.conversation_id,input,"run","thread","turn").unwrap().unwrap();
    assert!(native::reserve(&mut c,a.conversation_id,input,"run","thread","turn").is_err());
    assert_eq!(recover(&mut c).unwrap(),1);
    assert_eq!(packet(&c,p.id).unwrap().state,"uncertain");
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,p.through_seq);
    assert!(native::acknowledge(&mut c,planned.id,"run","thread","turn").is_err());
    assert!(native::reserve(&mut c,a.conversation_id,input,"run","thread","turn").is_err());
    assert_eq!(c.query_row("SELECT count(*) FROM butler_group_supplement",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert!(inbox(&c,room.group.id).unwrap().iter().filter(|i|i.member_id==a.id).all(|i|i.state=="uncertain"));
}

#[test]
fn test_group_native_stop_rejects_late_ack_without_reviving_work() {
    use crate::db::butler_group_native as native;
    let (mut c,room)=setup(); let a=&room.members[1];
    group::post(&mut c,room.group.id,None,&post("native-cancel","开始",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    group::post(&mut c,room.group.id,None,&post("native-cancel-queued","补充",vec![a.id])).unwrap();
    let planned=native::reserve(&mut c,a.conversation_id,input,"run","thread","turn").unwrap().unwrap();
    group::stop_member(&mut c,room.group.id,a.id).unwrap();
    assert!(native::acknowledge(&mut c,planned.id,"run","thread","turn").is_err());
    native::uncertain(&mut c,planned.id,"late error").unwrap();
    assert_eq!(packet(&c,p.id).unwrap().state,"cancelled");
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,p.through_seq);
}

#[test]
fn test_group_round_later_member_sees_committed_answer_once() {
    let (mut c,room)=setup();
    let a=&room.members[1]; let b=&room.members[2];
    let request=post("round","依次说说观点",vec![]);
    post_round(&mut c,room.group.id,&request,&[a.id,b.id]).unwrap();
    assert!(prepare(&mut c,room.members[0].id).unwrap().is_none());
    assert!(prepare(&mut c,b.id).unwrap().is_none());
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    let output=answer(&c,a,input,"甲的完整观点");
    let first=publish_final(&mut c,p.id,output).unwrap();
    assert_eq!(publish_final(&mut c,p.id,output).unwrap().seq,first.seq);
    let q=prepare(&mut c,b.id).unwrap().unwrap();
    assert!(q.payload.contains("甲的完整观点"));
    assert!(prepare(&mut c,a.id).unwrap().is_none());
    post_round(&mut c,room.group.id,&request,&[a.id,b.id]).unwrap();
    assert_eq!(rounds(&c,room.group.id).unwrap().len(),1);
    assert_eq!(inbox(&c,room.group.id).unwrap().len(),2);
}

#[test]
fn test_group_three_ordered_seats_complete_once_without_background_reactivation() {
    let (mut c,room)=setup();
    let speakers=[&room.members[1],&room.members[2],&room.members[0]];
    let ids=speakers.iter().map(|member|member.id).collect::<Vec<_>>();
    let request=post("three-seats","三人按顺序给出意见",vec![]);
    post_round(&mut c,room.group.id,&request,&ids).unwrap();
    let mut opinions=Vec::<String>::new();
    let mut frozen=Vec::<(i64,String)>::new();
    for (index,member) in speakers.iter().enumerate() {
        for future in &speakers[index+1..] {assert!(prepare(&mut c,future.id).unwrap().is_none());}
        let packet=prepare(&mut c,member.id).unwrap().unwrap();
        for earlier in &opinions {assert!(packet.payload.contains(earlier));}
        assert!(!packet.payload.contains("私有工具结果"));
        let input=activate(&mut c,packet.id,member.conversation_id,member.assistant_id,&packet.payload).unwrap();
        frozen.push((input,packet.payload));
        c.execute("INSERT INTO message(conversation_id,parent_id,message_type,content,finish_time) VALUES(?1,?2,'tool_result','私有工具结果',CURRENT_TIMESTAMP)",params![member.conversation_id,input]).unwrap();
        let mut progress=post(&format!("progress-{index}"),"仍在核对",vec![]);
        progress.kind=group::MessageKind::Progress;
        group::post(&mut c,room.group.id,Some(member.id),&progress).unwrap();
        if index<2 {assert!(prepare(&mut c,speakers[index+1].id).unwrap().is_none());}
        let opinion=format!("第 {} 位完整意见",index+1);
        let output=answer(&c,member,input,&opinion);
        let event=publish_final(&mut c,packet.id,output).unwrap();
        assert_eq!(publish_final(&mut c,packet.id,output).unwrap().seq,event.seq);
        opinions.push(opinion);
        for earlier in &speakers[..=index] {assert!(prepare(&mut c,earlier.id).unwrap().is_none());}
    }
    post_round(&mut c,room.group.id,&request,&ids).unwrap();
    let seats=rounds(&c,room.group.id).unwrap();
    assert_eq!(seats.len(),1);
    assert_eq!(seats[0].state,"completed");
    assert!(inbox(&c,room.group.id).unwrap().iter().all(|item|item.state=="answered"));
    assert_eq!(inbox(&c,room.group.id).unwrap().len(),3);
    for (input,payload) in frozen {
        assert_eq!(c.query_row("SELECT content FROM message WHERE id=?1",[input],|r|r.get::<_,String>(0)).unwrap(),payload);
    }
}

#[test]
fn test_group_packet_frozen_and_claimed_without_duplicate_input() {
    let (mut c,room)=setup(); let a=&room.members[1];
    group::post(&mut c,room.group.id,None,&post("a","first",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    group::post(&mut c,room.group.id,None,&post("b","later",vec![a.id])).unwrap();
    assert_eq!(prepare(&mut c,a.id).unwrap().unwrap().payload,p.payload);
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,0);
    assert!(activate(&mut c,p.id,a.conversation_id,a.assistant_id,"changed").is_err());
    assert_eq!(group::member(&c,a.id).unwrap().delivered_through,0);
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    assert!(activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).is_err());
    ensure_execution(&c,a.conversation_id,Some(input)).unwrap();
    assert!(ensure_execution(&c,room.members[2].conversation_id,Some(input)).is_err());
    assert_eq!(c.query_row("SELECT count(*) FROM message",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    assert!(!p.payload.contains("later"));
}

#[test]
fn test_group_same_assistant_private_chat_and_two_groups_have_separate_inputs_and_search() {
    let (mut c,first)=setup();
    let second=group::create(&mut c,"第二群","第二群独有目标",(1,"总管家".into()),&[(2,"甲".into()),(3,"乙".into())]).unwrap();
    let a=&first.members[1]; let b=&second.members[1];
    assert_eq!(a.assistant_id,b.assistant_id);
    assert_ne!(a.conversation_id,b.conversation_id);
    c.execute("INSERT INTO conversation(name,assistant_id,conversation_kind) VALUES('私聊',?1,'normal')",[a.assistant_id]).unwrap();
    let private=c.last_insert_rowid();
    c.execute("INSERT INTO message(conversation_id,message_type,content,finish_time) VALUES(?1,'user','私聊限定内容',CURRENT_TIMESTAMP)",[private]).unwrap();
    let one=group::post(&mut c,first.group.id,None,&post("same-key","第一群专属资料",vec![])).unwrap();
    let two=group::post(&mut c,second.group.id,None,&post("same-key","第二群专属资料",vec![])).unwrap();
    for (room,member,source) in [(&first,a,&one),(&second,b,&two)] {
        let mut request=post("work","请结合本群引用处理",vec![member.id]);
        request.reply_to=Some(source.seq);
        request.reference_only=true;
        group::post(&mut c,room.group.id,None,&request).unwrap();
    }
    let pa=prepare(&mut c,a.id).unwrap().unwrap();
    let pb=prepare(&mut c,b.id).unwrap().unwrap();
    for (packet,own,foreign) in [(&pa,"第一群专属资料","第二群专属资料"),(&pb,"第二群专属资料","第一群专属资料")] {
        assert!(packet.payload.contains(own));
        assert!(!packet.payload.contains(foreign));
        assert!(!packet.payload.contains("私聊限定内容"));
    }
    let ia=activate(&mut c,pa.id,a.conversation_id,a.assistant_id,&pa.payload).unwrap();
    let ib=activate(&mut c,pb.id,b.conversation_id,b.assistant_id,&pb.payload).unwrap();
    assert!(ensure_execution(&c,a.conversation_id,Some(ib)).is_err());
    assert!(ensure_execution(&c,b.conversation_id,Some(ia)).is_err());
    for (member,foreign) in [(a,"第二群专属资料"),(b,"第一群专属资料")] {
        assert!(execution::search_public(&c,member.conversation_id,0,100,foreign).unwrap().0.is_empty());
        assert!(execution::search_public(&c,member.conversation_id,0,100,"私聊限定内容").unwrap().0.is_empty());
    }
    let output=answer(&c,a,ia,"第一群独有交付");
    assert!(publish_final(&mut c,pb.id,output).is_err());
    publish_final(&mut c,pa.id,output).unwrap();
    assert_eq!(packet(&c,pb.id).unwrap().state,"running");
    assert!(execution::search_public(&c,b.conversation_id,0,100,"第一群独有交付").unwrap().0.is_empty());
    assert_eq!(c.query_row("SELECT count(*) FROM message WHERE conversation_id=?1",[private],|r|r.get::<_,i64>(0)).unwrap(),1);
}

#[test]
fn test_group_new_member_only_reads_explicitly_shared_old_messages() {
    let (mut c,room)=setup();
    let old=group::post(&mut c,room.group.id,None,&post("old","private-to-old-members",vec![])).unwrap();
    let m=group::add_member(&mut c,room.group.id,4,"新成员").unwrap();
    assert!(group::read_as_member(&c,m.id,old.seq).is_err());
    let mut quote=post("quote","请评价",vec![m.id]); quote.reply_to=Some(old.seq);
    assert!(group::post(&mut c,room.group.id,None,&quote).is_err());
    group::share(&mut c,room.group.id,m.id,&[old.seq],"share").unwrap();
    group::post(&mut c,room.group.id,None,&quote).unwrap();
    assert!(prepare(&mut c,m.id).unwrap().unwrap().payload.contains("private-to-old-members"));
    group::remove_member(&mut c,room.group.id,m.id).unwrap();
    let fresh=group::add_member(&mut c,room.group.id,4,"新成员").unwrap();
    assert_ne!(fresh.id,m.id);
    assert!(group::read_as_member(&c,fresh.id,old.seq).is_err());
}

#[test]
fn test_group_tools_and_stopped_late_results_are_never_published() {
    let (mut c,room)=setup(); let a=&room.members[1];
    group::post(&mut c,room.group.id,None,&post("ask","做事",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    let output=answer(&c,a,input,"calling tool");
    c.execute("UPDATE message SET tool_calls_json='[{\"id\":\"secret-tool\"}]' WHERE id=?1",[output]).unwrap();
    assert!(publish_final(&mut c,p.id,output).is_err());
    assert!(!group::events(&c,room.group.id,0,100).unwrap().iter().any(|e|e.body.contains("calling tool")));
    group::stop(&mut c,room.group.id).unwrap();
    let late=answer(&c,a,input,"late answer");
    assert!(publish_final(&mut c,p.id,late).is_err());
    assert!(ensure_execution(&c,a.conversation_id,Some(input)).is_err());
}

#[test]
fn test_group_waiting_releases_one_seat_without_completing_work() {
    let (mut c,room)=setup(); let a=&room.members[1]; let b=&room.members[2];
    post_round(&mut c,room.group.id,&post("round","意见",vec![]),&[a.id,b.id]).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    assert!(waiting(&mut c,p.id,"approval").unwrap());
    assert!(!waiting(&mut c,p.id,"approval").unwrap());
    assert_eq!(packet(&c,p.id).unwrap().state,"running");
    assert_eq!(inbox(&c,room.group.id).unwrap()[0].state,"waiting");
    let q=prepare(&mut c,b.id).unwrap().unwrap();
    let output=answer(&c,a,input,"晚到的观点");
    publish_final(&mut c,p.id,output).unwrap();
    assert_eq!(rounds(&c,room.group.id).unwrap()[0].next_slot,1);
    assert!(!q.payload.contains("晚到的观点"));
}

#[test]
fn test_group_recovery_fences_unknown_run_without_replay() {
    let (mut c,room)=setup(); let a=&room.members[1];
    group::post(&mut c,room.group.id,None,&post("ask","执行一次",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    assert_eq!(recover(&mut c).unwrap(),1);
    assert_eq!(recover(&mut c).unwrap(),0);
    assert!(prepare(&mut c,a.id).is_err());
    assert!(ensure_execution(&c,a.conversation_id,Some(input)).is_err());
    assert_eq!(packet(&c,p.id).unwrap().state,"uncertain");
}

#[test]
fn test_group_reference_is_material_without_waking_quoted_author() {
    let (mut c,room)=setup(); let author=&room.members[1];
    let original=group::post(&mut c,room.group.id,Some(author.id),&post("view","原观点",vec![])).unwrap();
    let mut citation=post("cite","结合这个观点给我建议",vec![]);
    citation.reply_to=Some(original.seq); citation.reference_only=true;
    let saved=group::post(&mut c,room.group.id,None,&citation).unwrap();
    assert!(saved.reference_only);
    let items=inbox(&c,room.group.id).unwrap();
    assert_eq!(items.len(),1); assert_eq!(items[0].member_id,room.members[0].id);
    assert!(prepare(&mut c,author.id).unwrap().is_none());
    let p=prepare(&mut c,room.members[0].id).unwrap().unwrap();
    assert!(p.payload.contains("原观点"));
    assert!(p.payload.contains("\"reference_only\":true"));
}

#[test]
fn test_group_stop_member_keeps_other_group_and_members_active() {
    let (mut c,room)=setup(); let a=&room.members[1];
    let other=group::create(&mut c,"另一个群","独立目标",(1,"总管家".into()),&[(2,"甲".into())]).unwrap();
    group::post(&mut c,room.group.id,None,&post("a","first",vec![a.id])).unwrap();
    group::post(&mut c,other.group.id,None,&post("b","other",vec![other.members[1].id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    group::stop_member(&mut c,room.group.id,a.id).unwrap();
    assert!(group::member(&c,a.id).unwrap().active);
    assert_eq!(packet(&c,p.id).unwrap().state,"cancelled");
    assert!(prepare(&mut c,other.members[1].id).unwrap().is_some());
}

#[test]
fn test_group_dependency_only_wakes_waiter_and_restores_original_request() {
    use crate::db::butler_group_dependency as dep;
    let (mut c,room)=setup(); let a=&room.members[1]; let b=&room.members[2];
    group::post(&mut c,room.group.id,None,&post("task","完成用户的原工作",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    let request=post("help","请找证据",vec![b.id]);
    dep::request(&mut c,a.conversation_id,Some(input),&request).unwrap();
    dep::request(&mut c,a.conversation_id,Some(input),&request).unwrap();
    let silent=answer(&c,a,input,"[[NO_REPLY]]");
    dep::suspend(&mut c,p.id,silent).unwrap();
    assert_eq!(inbox(&c,room.group.id).unwrap()[0].state,"waiting");
    let q=prepare(&mut c,b.id).unwrap().unwrap();
    let b_input=activate(&mut c,q.id,b.conversation_id,b.assistant_id,&q.payload).unwrap();
    let result=answer(&c,b,b_input,"找到证据");
    publish_final(&mut c,q.id,result).unwrap();
    publish_final(&mut c,q.id,result).unwrap();
    assert!(prepare(&mut c,room.members[0].id).unwrap().is_none());
    let resumed=prepare(&mut c,a.id).unwrap().unwrap();
    assert!(resumed.payload.contains("完成用户的原工作"));
    assert!(resumed.payload.contains("找到证据"));
    assert!(resumed.payload.contains("resumed_work"));
    let resumed_input=activate(&mut c,resumed.id,a.conversation_id,a.assistant_id,&resumed.payload).unwrap();
    let final_answer=answer(&c,a,resumed_input,"已完成原工作");
    publish_final(&mut c,resumed.id,final_answer).unwrap();
    assert_eq!(inbox(&c,room.group.id).unwrap()[0].state,"answered");
    assert!(prepare(&mut c,b.id).unwrap().is_none());
}

#[test]
fn test_group_dependency_cycle_is_rejected_and_cancelled_target_returns_receipt() {
    use crate::db::butler_group_dependency as dep;
    let (mut c,room)=setup(); let a=&room.members[1]; let b=&room.members[2];
    group::post(&mut c,room.group.id,None,&post("task","原工作",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    dep::request(&mut c,a.conversation_id,Some(input),&post("help","找材料",vec![b.id])).unwrap();
    let silent=answer(&c,a,input,"[[NO_REPLY]]"); dep::suspend(&mut c,p.id,silent).unwrap();
    let q=prepare(&mut c,b.id).unwrap().unwrap();
    let b_input=activate(&mut c,q.id,b.conversation_id,b.assistant_id,&q.payload).unwrap();
    assert!(dep::request(&mut c,b.conversation_id,Some(b_input),&post("loop","请上游来做",vec![a.id])).is_err());
    group::stop_member(&mut c,room.group.id,b.id).unwrap();
    let resumed=prepare(&mut c,a.id).unwrap().unwrap();
    assert!(resumed.payload.contains("未完成"));
    assert!(resumed.payload.contains("原工作"));
}

#[test]
fn test_group_dependency_partial_results_do_not_complete_original_work() {
    use crate::db::butler_group_dependency as dep;
    let (mut c,room)=setup(); let a=&room.members[0]; let b=&room.members[1]; let d=&room.members[2];
    group::post(&mut c,room.group.id,None,&post("task","分别找两份证据",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    dep::request(&mut c,a.conversation_id,Some(input),&post("help","各找一份",vec![b.id,d.id])).unwrap();
    let silent=answer(&c,a,input,"[[NO_REPLY]]"); dep::suspend(&mut c,p.id,silent).unwrap();
    let q=prepare(&mut c,b.id).unwrap().unwrap();
    let b_input=activate(&mut c,q.id,b.conversation_id,b.assistant_id,&q.payload).unwrap();
    let result=answer(&c,b,b_input,"第一份证据"); publish_final(&mut c,q.id,result).unwrap();
    let r=prepare(&mut c,a.id).unwrap().unwrap();
    let r_input=activate(&mut c,r.id,a.conversation_id,a.assistant_id,&r.payload).unwrap();
    let progress=answer(&c,a,r_input,"已分析第一份，仍等待另一份"); publish_final(&mut c,r.id,progress).unwrap();
    assert_eq!(inbox(&c,room.group.id).unwrap()[0].state,"waiting");
}

#[test]
fn test_group_independent_tools_cannot_read_future_or_delegate_around_snapshot() {
    let (mut c,room)=setup(); let a=&room.members[1]; let b=&room.members[2];
    post_independent(&mut c,room.group.id,&post("read-snapshot","独立判断",vec![]),&[a.id,b.id]).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    let later=group::post(&mut c,room.group.id,Some(b.id),&post("future-opinion","不应提前看到的意见",vec![])).unwrap();
    assert_eq!(execution::snapshot_limit(&c,a.conversation_id).unwrap(),Some(p.through_seq));
    assert!(execution::read_public(&c,a.conversation_id,later.seq).is_err());
    assert!(execution::search_public(&c,a.conversation_id,0,100,"不应提前看到").unwrap().0.is_empty());
    assert!(execution::read_public(&c,a.conversation_id,1).is_ok());
    assert!(start_agent_discussion(&mut c,a.conversation_id,Some(input),1,&[b.id],"bypass-discussion",false).is_err());
    let request=execution::TaskRequest {request_key:"bypass-task".into(),member_id:b.id,title:"代读".into(),prompt:"查看后续意见".into(),source_seqs:vec![]};
    assert!(execution::start_task(&mut c,room.group.id,&request,Some((a.conversation_id,input))).is_err());
    assert!(crate::db::butler_group_dependency::post_without_wait(&mut c,a.conversation_id,Some(input),&post("bypass-post","请告诉我其他人意见",vec![b.id])).is_err());
    let message=answer(&c,a,input,"我的独立意见");
    publish_final(&mut c,p.id,message).unwrap();
    assert_eq!(execution::snapshot_limit(&c,a.conversation_id).unwrap(),None);
    assert!(execution::read_public(&c,a.conversation_id,later.seq).is_ok());
}

#[test]
fn test_group_independent_snapshot_excludes_earlier_finishing_peers() {
    let (mut c,room)=setup(); let a=&room.members[1]; let b=&room.members[2];
    post_independent(&mut c,room.group.id,&post("independent","各自判断",vec![]),&[a.id,b.id]).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    let output=answer(&c,a,input,"甲的答案不应影响乙"); publish_final(&mut c,p.id,output).unwrap();
    let q=prepare(&mut c,b.id).unwrap().unwrap();
    assert_eq!(p.through_seq,q.through_seq);
    assert!(!q.payload.contains("甲的答案不应影响乙"));
    assert!(q.payload.contains("各自判断"));
}

#[test]
fn test_group_goal_replacement_fences_execution_approval_and_question_without_rewriting_prefix() {
    let (mut c,room)=setup(); let a=&room.members[1];
    group::post(&mut c,room.group.id,None,&post("old","先做旧工作",vec![a.id])).unwrap();
    let p=prepare(&mut c,a.id).unwrap().unwrap();
    let input=activate(&mut c,p.id,a.conversation_id,a.assistant_id,&p.payload).unwrap();
    let request=crate::db::butler_approval::ApprovalRequest{action_id:"write".into(),args:serde_json::json!({"path":"file"})};
    crate::db::butler_approval::binding_from_connection(&c,a.conversation_id,&request,1000,Some(input)).unwrap();
    crate::db::butler_goal::save_question(&c,a.conversation_id,"question","{}",Some(input),100).unwrap();
    assert_eq!(crate::db::butler_goal::validate_question(&c,"question",101).unwrap(),Some(a.conversation_id));
    let ids=room.members.iter().map(|m|m.id).collect::<Vec<_>>();
    assert!(group::replace_goal(&mut c,room.group.id,"新的目标","goal-change",&ids).unwrap());
    let seq=group::get(&c,room.group.id).unwrap().seq;
    assert!(!group::replace_goal(&mut c,room.group.id,"新的目标","goal-change",&ids).unwrap());
    assert_eq!(seq,group::get(&c,room.group.id).unwrap().seq);
    assert_eq!(packet(&c,p.id).unwrap().payload,p.payload);
    assert_eq!(c.query_row("SELECT content FROM message WHERE id=?1",[input],|r|r.get::<_,String>(0)).unwrap(),p.payload);
    assert!(ensure_execution(&c,a.conversation_id,Some(input)).is_err());
    assert!(crate::db::butler_approval::binding_from_connection(&c,a.conversation_id,&request,1000,Some(input)).is_err());
    assert!(crate::db::butler_goal::validate_question(&c,"question",101).is_err());
    let next=prepare(&mut c,room.members[0].id).unwrap().unwrap(); assert!(next.payload.contains("新的目标"));
}
