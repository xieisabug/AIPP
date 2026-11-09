use super::*;
use crate::db::{butler_group_delivery as delivery,butler_group_dependency as dependency};

fn setup()->(Connection,group::GroupView) {
    let mut c=Connection::open_in_memory().unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE conversation(id INTEGER PRIMARY KEY,name TEXT,assistant_id INTEGER,conversation_kind TEXT,parent_butler_conversation_id INTEGER,is_hidden_from_normal_chat_list INTEGER,updated_time TEXT,butler_task_status TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,parent_id INTEGER,message_type TEXT,content TEXT,metadata_json TEXT,token_count INTEGER,input_token_count INTEGER,output_token_count INTEGER,finish_time TEXT,tool_calls_json TEXT);").unwrap();
    group::schema(&c).unwrap();
    let room=group::create(&mut c,"群","目标",(1,"甲".into()),&[(2,"乙".into())]).unwrap();
    (c,room)
}
fn request(member:i64,key:&str)->TaskRequest {
    TaskRequest {request_key:key.into(),member_id:member,title:format!("task-{key}"),prompt:format!("独立完成 {key}"),source_seqs:vec![]}
}
fn post(key:&str,body:&str,recipient:i64)->group::Post {
    group::Post {request_key:key.into(),body:body.into(),kind:group::MessageKind::Message,reply_to:None,reference_only:false,recipients:vec![recipient]}
}
fn activate(c:&mut Connection,conversation:i64)->(delivery::Packet,i64) {
    let p=delivery::prepare_execution(c,conversation).unwrap().unwrap();
    let e=scope(c,conversation).unwrap();
    let input=delivery::activate(c,p.id,conversation,e.assistant_id,&p.payload).unwrap();
    (p,input)
}
fn answer(c:&Connection,conversation:i64,parent:i64,body:&str)->i64 {
    c.execute("INSERT INTO message(conversation_id,parent_id,message_type,content,finish_time) VALUES(?1,?2,'response',?3,CURRENT_TIMESTAMP)",params![conversation,parent,body]).unwrap();c.last_insert_rowid()
}

#[test]
fn test_group_tasks_same_member_parallel_contexts_and_return_to_original_execution() {
    let (mut c,room)=setup(); let m=&room.members[0];
    group::post(&mut c,room.group.id,None,&post("main","原始总请求",m.id)).unwrap();
    let (main,input)=activate(&mut c,m.conversation_id);
    c.execute("INSERT INTO message(conversation_id,parent_id,message_type,content) VALUES(?1,?2,'tool_result','main-private-secret')",params![m.conversation_id,input]).unwrap();
    let a=start_task(&mut c,room.group.id,&request(m.id,"a"),Some((m.conversation_id,input))).unwrap();
    let b=start_task(&mut c,room.group.id,&request(m.id,"b"),Some((m.conversation_id,input))).unwrap();
    assert_ne!(a.conversation_id,b.conversation_id);
    assert_eq!(group::members(&c,room.group.id).unwrap().len(),2);
    let (pa,ia)=activate(&mut c,a.conversation_id);
    let (pb,ib)=activate(&mut c,b.conversation_id);
    assert!(!pa.payload.contains("main-private-secret"));
    assert!(!pb.payload.contains("main-private-secret"));
    assert!(delivery::ensure_execution(&c,a.conversation_id,Some(ib)).is_err());
    assert!(delivery::validate_dispatch(&c,pa.id,b.conversation_id,m.assistant_id,&pa.payload).is_err());
    assert_eq!(c.query_row("SELECT count(*) FROM butler_group_packet WHERE member_id=?1 AND state='running'",[m.id],|r|r.get::<_,i64>(0)).unwrap(),3);
    // Reopening the schema cannot reinstall the old per-member active index.
    group::schema(&c).unwrap();
    let waiting=answer(&c,m.conversation_id,input,"等待独立任务结果");
    dependency::suspend(&mut c,main.id,waiting).unwrap();
    let output=answer(&c,a.conversation_id,ia,"task-a-public-result");
    let event=delivery::publish_final(&mut c,pa.id,output).unwrap();
    assert_eq!(event.sender_member,Some(m.id));
    assert_eq!(event.sender_conversation,Some(a.conversation_id));
    let resumed=delivery::prepare_execution(&mut c,m.conversation_id).unwrap().unwrap();
    assert!(resumed.payload.contains("task-a-public-result"));
    assert!(resumed.payload.contains("原始总请求"));
    assert_eq!(delivery::packet(&c,pb.id).unwrap().state,"running");
    assert!(delivery::inbox(&c,room.group.id).unwrap().iter().any(|i|i.packet_id==Some(main.id)&&i.state=="waiting"));
}

#[test]
fn test_group_task_stop_fences_descendants_but_not_siblings_or_main() {
    let (mut c,room)=setup(); let m=&room.members[0]; let other=&room.members[1];
    group::post(&mut c,room.group.id,None,&post("main","工作",m.id)).unwrap();
    let (main,input)=activate(&mut c,m.conversation_id);
    let a=start_task(&mut c,room.group.id,&request(m.id,"a"),Some((m.conversation_id,input))).unwrap();
    let (pa,ia)=activate(&mut c,a.conversation_id);
    let child=start_task(&mut c,room.group.id,&request(other.id,"child"),Some((a.conversation_id,ia))).unwrap();
    let (pc,ic)=activate(&mut c,child.conversation_id);
    let sibling=start_task(&mut c,room.group.id,&request(m.id,"sibling"),None).unwrap();
    let (ps,is)=activate(&mut c,sibling.conversation_id);
    let stopped=stop_task(&mut c,room.group.id,a.conversation_id).unwrap();
    assert_eq!(stopped.len(),2);
    for id in [pa.id,pc.id] {assert_eq!(delivery::packet(&c,id).unwrap().state,"cancelled");}
    for id in [main.id,ps.id] {assert_eq!(delivery::packet(&c,id).unwrap().state,"running");}
    assert!(delivery::ensure_execution(&c,child.conversation_id,Some(ic)).is_err());
    delivery::ensure_execution(&c,sibling.conversation_id,Some(is)).unwrap();
    assert!(group::member(&c,m.id).unwrap().active);
    assert!(group::member(&c,other.id).unwrap().active);
}

#[test]
fn test_group_task_quotes_and_reply_keep_execution_and_approval_identity() {
    let (mut c,room)=setup(); let m=&room.members[0];
    let selected=group::post(&mut c,room.group.id,None,&post("source","选中的公开证据",m.id)).unwrap();
    group::post(&mut c,room.group.id,None,&post("not-selected","不应默认继承的旧资料",m.id)).unwrap();
    let mut req=request(m.id,"evidence");req.source_seqs=vec![selected.seq];
    let task=start_task(&mut c,room.group.id,&req,None).unwrap();
    assert_eq!(start_task(&mut c,room.group.id,&req,None).unwrap().conversation_id,task.conversation_id);
    let (p,input)=activate(&mut c,task.conversation_id);
    assert!(p.payload.contains("选中的公开证据"));
    assert!(!p.payload.contains("不应默认继承的旧资料"));
    let approval=crate::db::butler_approval::ApprovalRequest {action_id:"test".into(),args:serde_json::json!({})};
    let bound=crate::db::butler_approval::binding_from_connection(&c,task.conversation_id,&approval,1000,Some(input)).unwrap();
    assert_eq!(bound.goal_revision,Some(p.id));
    assert!(crate::db::butler_approval::binding_from_connection(&c,m.conversation_id,&approval,1000,Some(input)).is_err());
    crate::db::butler_goal::save_question(&c,task.conversation_id,"task-question","{}",Some(input),100).unwrap();
    assert_eq!(crate::db::butler_goal::validate_question(&c,"task-question",101).unwrap(),Some(task.conversation_id));
    let output=answer(&c,task.conversation_id,input,"task-complete");
    let event=delivery::publish_final(&mut c,p.id,output).unwrap();
    let mut reply=post("reply","继续这个独立任务",m.id);reply.reply_to=Some(event.seq);
    let reply_event=group::post(&mut c,room.group.id,None,&reply).unwrap();
    let queued=delivery::inbox(&c,room.group.id).unwrap().into_iter().find(|i|i.event_seq==reply_event.seq).unwrap();
    assert_eq!(queued.conversation_id,task.conversation_id);
    assert!(delivery::prepare_execution(&mut c,task.conversation_id).unwrap().unwrap().payload.contains("继续这个独立任务"));
}

#[test]
fn test_group_chain_budget_is_shared_by_task_siblings_and_resumed_owner() {
    let (mut c,room)=setup();let m=&room.members[0];
    group::post(&mut c,room.group.id,None,&post("root","整体工作",m.id)).unwrap();
    let (root,input)=activate(&mut c,m.conversation_id);
    let a=start_task(&mut c,room.group.id,&request(m.id,"a"),Some((m.conversation_id,input))).unwrap();
    let b=start_task(&mut c,room.group.id,&request(m.id,"b"),Some((m.conversation_id,input))).unwrap();
    let (pa,ia)=activate(&mut c,a.conversation_id);
    let (_,ib)=activate(&mut c,b.conversation_id);
    for index in 0..30 {
        start_task(&mut c,room.group.id,&request(m.id,&format!("child-{index}")),Some((a.conversation_id,ia))).unwrap();
    }
    let before=list(&c,room.group.id).unwrap().len();
    assert!(start_task(&mut c,room.group.id,&request(m.id,"over-budget"),Some((b.conversation_id,ib)))
        .unwrap_err().to_string().contains("预算已用尽"));
    assert_eq!(list(&c,room.group.id).unwrap().len(),before);
    // A retry returns the same persisted child without consuming another unit.
    start_task(&mut c,room.group.id,&request(m.id,"a"),Some((m.conversation_id,input))).unwrap();
    let waiting=answer(&c,m.conversation_id,input,"等待任务");
    dependency::suspend(&mut c,root.id,waiting).unwrap();
    let output=answer(&c,a.conversation_id,ia,"阶段结果");
    delivery::publish_final(&mut c,pa.id,output).unwrap();
    let (_,resumed)=activate(&mut c,m.conversation_id);
    assert!(start_task(&mut c,room.group.id,&request(m.id,"resumed-over-budget"),Some((m.conversation_id,resumed)))
        .unwrap_err().to_string().contains("预算已用尽"));
}

#[test]
fn test_group_chain_task_inherits_original_expiry_without_resetting_clock() {
    let (mut c,room)=setup();let m=&room.members[0];
    group::post(&mut c,room.group.id,None,&post("root","整体工作",m.id)).unwrap();
    let (root,input)=activate(&mut c,m.conversation_id);
    let a=start_task(&mut c,room.group.id,&request(m.id,"a"),Some((m.conversation_id,input))).unwrap();
    let (_,ia)=activate(&mut c,a.conversation_id);
    c.execute("UPDATE butler_group_packet SET created_at=datetime('now','-121 minutes') WHERE id=?1",[root.id]).unwrap();
    assert!(start_task(&mut c,room.group.id,&request(m.id,"too-late"),Some((a.conversation_id,ia)))
        .unwrap_err().to_string().contains("有效期已到"));
    // An independent user request has its own scope; it is not charged to this chain.
    start_task(&mut c,room.group.id,&request(m.id,"new-user-work"),None).unwrap();
}

#[test]
fn test_group_inbox_same_event_and_reason_are_independent_per_execution() {
    let (mut c,room)=setup();let m=&room.members[0];
    let a=start_task(&mut c,room.group.id,&request(m.id,"a"),None).unwrap();
    let b=start_task(&mut c,room.group.id,&request(m.id,"b"),None).unwrap();
    let event=group::post(&mut c,room.group.id,None,&post("shared","共同补充",m.id)).unwrap();
    for conversation in [a.conversation_id,b.conversation_id,a.conversation_id] {
        enqueue_in(&c,conversation,event.seq,"same-obligation",None).unwrap();
    }
    let count:i64=c.query_row("SELECT count(*) FROM butler_group_inbox WHERE event_seq=?1 AND reason='same-obligation'",[event.seq],|r|r.get(0)).unwrap();
    assert_eq!(count,2);
}

#[test]
fn test_group_chain_nonwaiting_posts_and_discussion_share_budget_and_ancestry() {
    let (mut c,room)=setup();let a=&room.members[0];let b=&room.members[1];
    let topic=group::post(&mut c,room.group.id,None,&post("root","整体工作",a.id)).unwrap();
    let (_,ia)=activate(&mut c,a.conversation_id);
    let req=post("direct-0","请处理",b.id);
    let first=dependency::post_without_wait(&mut c,a.conversation_id,Some(ia),&req).unwrap();
    assert_eq!(dependency::post_without_wait(&mut c,a.conversation_id,Some(ia),&req).unwrap().seq,first.seq);
    let (_,ib)=activate(&mut c,b.conversation_id);
    let cycle=dependency::post_without_wait(&mut c,b.conversation_id,Some(ib),&post("cycle","回派上游",a.id)).unwrap_err();
    assert!(cycle.to_string().contains("上游助手"));
    for index in 1..31 {
        dependency::post_without_wait(&mut c,a.conversation_id,Some(ia),&post(&format!("direct-{index}"),"继续处理",b.id)).unwrap();
    }
    let round=delivery::start_agent_discussion(&mut c,a.conversation_id,Some(ia),topic.seq,&[b.id],"last-slot",false).unwrap();
    assert_eq!(delivery::start_agent_discussion(&mut c,a.conversation_id,Some(ia),topic.seq,&[b.id],"last-slot",false).unwrap(),round);
    let before=group::get(&c,room.group.id).unwrap().seq;
    assert!(dependency::post_without_wait(&mut c,a.conversation_id,Some(ia),&post("over-budget","超额",b.id))
        .unwrap_err().to_string().contains("预算已用尽"));
    assert_eq!(group::get(&c,room.group.id).unwrap().seq,before);
    assert!(delivery::start_agent_discussion(&mut c,a.conversation_id,Some(ia),topic.seq,&[b.id],"over-budget-independent",true)
        .unwrap_err().to_string().contains("预算已用尽"));
    let independent_count:i64=c.query_row("SELECT count(*) FROM butler_group_independent",[],|r|r.get(0)).unwrap();
    assert_eq!(independent_count,0);
}

#[test]
fn test_group_upstream_reply_keeps_original_task_and_does_not_create_cycle() {
    let (mut c,room)=setup();let a=&room.members[0];let b=&room.members[1];
    let task=start_task(&mut c,room.group.id,&request(a.id,"owner-task"),None).unwrap();
    let (pa,ia)=activate(&mut c,task.conversation_id);
    let request_event=dependency::post_without_wait(&mut c,task.conversation_id,Some(ia),&post("delegate","请确认资料",b.id)).unwrap();
    let (_,ib)=activate(&mut c,b.conversation_id);
    let mut reply=post("reply","这是所需资料",a.id);reply.reply_to=Some(request_event.seq);
    let event=dependency::post_without_wait(&mut c,b.conversation_id,Some(ib),&reply).unwrap();
    let queued=delivery::inbox(&c,room.group.id).unwrap().into_iter().find(|i|i.event_seq==event.seq).unwrap();
    assert_eq!(queued.conversation_id,task.conversation_id);
    let origin:i64=c.query_row("SELECT return_to_packet FROM butler_group_dispatch WHERE event_seq=?1 AND target_conversation_id=?2",
        params![event.seq,task.conversation_id],|r|r.get(0)).unwrap();
    assert_eq!(origin,pa.id);
    delivery::append_tool_boundary(&mut c,task.conversation_id,ia).unwrap().unwrap();
    // Appending a child reply to its still-running parent must not turn the
    // persisted work graph into A -> B -> A.
    super::super::butler_group_chain::admit(&c,pa.id,1).unwrap();
    assert!(dependency::request(&mut c,b.conversation_id,Some(ib),&reply).is_err());
    let mut citation=reply.clone();citation.request_key="citation".into();citation.reference_only=true;
    assert!(dependency::post_without_wait(&mut c,b.conversation_id,Some(ib),&citation).is_err());
    // Completing this turn and processing a later reply still inherits A's
    // original root instead of treating B as a new parent delegation.
    let output=answer(&c,task.conversation_id,ia,"阶段完成");
    delivery::publish_final(&mut c,pa.id,output).unwrap();
    reply.request_key="reply-later".into();
    dependency::post_without_wait(&mut c,b.conversation_id,Some(ib),&reply).unwrap();
    let (next,_)=activate(&mut c,task.conversation_id);
    c.execute("UPDATE butler_group_packet SET created_at=datetime('now','-121 minutes') WHERE id=?1",[pa.id]).unwrap();
    assert!(super::super::butler_group_chain::admit(&c,next.id,1).unwrap_err().to_string().contains("有效期已到"));
}

#[test]
fn test_group_stop_queued_delegation_keeps_recipient_unrelated_run_and_cancels_future_seats() {
    let (mut c,room)=setup();let a=&room.members[0];let b=&room.members[1];
    let third=group::add_member(&mut c,room.group.id,3,"丙").unwrap();
    group::post(&mut c,room.group.id,None,&post("unrelated","乙自己的工作",b.id)).unwrap();
    let (unrelated,_)=activate(&mut c,b.conversation_id);
    let owner=start_task(&mut c,room.group.id,&request(a.id,"owner"),None).unwrap();
    let (_,input)=activate(&mut c,owner.conversation_id);
    let event=dependency::post_without_wait(&mut c,owner.conversation_id,Some(input),&post("delegated","派生工作",b.id)).unwrap();
    let round=delivery::start_agent_discussion(&mut c,owner.conversation_id,Some(input),event.seq,&[b.id,third.id],"round",false).unwrap();
    let stopped=stop_task(&mut c,room.group.id,owner.conversation_id).unwrap();
    assert!(!stopped.iter().any(|s|s.conversation_id==b.conversation_id));
    assert_eq!(delivery::packet(&c,unrelated.id).unwrap().state,"running");
    let inbox=delivery::inbox(&c,room.group.id).unwrap();
    assert!(inbox.iter().filter(|i|i.event_seq==event.seq).all(|i|i.state=="cancelled"));
    assert!(!inbox.iter().any(|i|i.member_id==third.id));
    let state:String=c.query_row("SELECT state FROM butler_group_round WHERE id=?1",[round],|r|r.get(0)).unwrap();
    assert_eq!(state,"cancelled");
}

#[test]
fn test_group_stop_prepared_mixed_packet_requeues_only_unrelated_obligation() {
    let (mut c,room)=setup();let a=&room.members[0];let b=&room.members[1];
    let owner=start_task(&mut c,room.group.id,&request(a.id,"owner"),None).unwrap();
    let (_,input)=activate(&mut c,owner.conversation_id);
    let delegated=dependency::post_without_wait(&mut c,owner.conversation_id,Some(input),&post("delegated","派生工作",b.id)).unwrap();
    let unrelated=group::post(&mut c,room.group.id,None,&post("unrelated","独立用户要求",b.id)).unwrap();
    let prepared=delivery::prepare_execution(&mut c,b.conversation_id).unwrap().unwrap();
    assert!(prepared.input_message_id.is_none());
    stop_task(&mut c,room.group.id,owner.conversation_id).unwrap();
    let inbox=delivery::inbox(&c,room.group.id).unwrap();
    assert_eq!(inbox.iter().find(|i|i.event_seq==delegated.seq).unwrap().state,"cancelled");
    let preserved=inbox.iter().find(|i|i.event_seq==unrelated.seq).unwrap();
    assert_eq!(preserved.state,"queued");assert!(preserved.packet_id.is_none());
    let next=delivery::prepare_execution(&mut c,b.conversation_id).unwrap().unwrap();
    assert_ne!(next.id,prepared.id);
    assert_eq!(delivery::packet(&c,prepared.id).unwrap().state,"cancelled");
}

#[test]
fn test_group_stop_running_delegation_reaches_nested_tasks_but_keeps_unrelated_queue() {
    let (mut c,room)=setup();let a=&room.members[0];let b=&room.members[1];
    let owner=start_task(&mut c,room.group.id,&request(a.id,"owner"),None).unwrap();
    let (_,input)=activate(&mut c,owner.conversation_id);
    dependency::post_without_wait(&mut c,owner.conversation_id,Some(input),&post("delegated","派生工作",b.id)).unwrap();
    let (pb,ib)=activate(&mut c,b.conversation_id);
    let child=start_task(&mut c,room.group.id,&request(b.id,"nested"),Some((b.conversation_id,ib))).unwrap();
    let (pc,_)=activate(&mut c,child.conversation_id);
    let unrelated=group::post(&mut c,room.group.id,None,&post("unrelated","稍后处理",b.id)).unwrap();
    assert!(super::super::butler_group_stop::owns_task(&c,owner.conversation_id,child.conversation_id).unwrap());
    let stopped=stop_task(&mut c,room.group.id,owner.conversation_id).unwrap();
    assert!(stopped.iter().any(|s|s.conversation_id==b.conversation_id));
    assert!(stopped.iter().any(|s|s.conversation_id==child.conversation_id));
    for p in [pb.id,pc.id] {assert_eq!(delivery::packet(&c,p).unwrap().state,"cancelled");}
    assert_eq!(scope(&c,b.conversation_id).unwrap().state,"active");
    assert_eq!(delivery::inbox(&c,room.group.id).unwrap().iter().find(|i|i.event_seq==unrelated.seq).unwrap().state,"queued");
}

#[test]
fn test_group_stop_mixed_resumption_releases_unsubmitted_binding_or_finishes_delivered_wait() {
    for delivered in [false,true] {
        let (mut c,room)=setup();let a=&room.members[0];let b=&room.members[1];
        let original=group::post(&mut c,room.group.id,None,&post("original","乙的原工作",b.id)).unwrap();
        let (pb,ib)=activate(&mut c,b.conversation_id);
        let child=start_task(&mut c,room.group.id,&request(b.id,"child"),Some((b.conversation_id,ib))).unwrap();
        let (pc,ic)=activate(&mut c,child.conversation_id);
        let waiting=answer(&c,b.conversation_id,ib,"等待资料");
        dependency::suspend(&mut c,pb.id,waiting).unwrap();
        let owner=start_task(&mut c,room.group.id,&request(a.id,"owner"),None).unwrap();
        let (_,ia)=activate(&mut c,owner.conversation_id);
        dependency::post_without_wait(&mut c,owner.conversation_id,Some(ia),&post("delegated","甲派出的工作",b.id)).unwrap();
        let result=answer(&c,child.conversation_id,ic,"乙原工作所需资料");
        let result=delivery::publish_final(&mut c,pc.id,result).unwrap();
        let mixed=delivery::prepare_execution(&mut c,b.conversation_id).unwrap().unwrap();
        if delivered {delivery::activate(&mut c,mixed.id,b.conversation_id,b.assistant_id,&mixed.payload).unwrap();}
        stop_task(&mut c,room.group.id,owner.conversation_id).unwrap();
        let inbox=delivery::inbox(&c,room.group.id).unwrap();
        let original=inbox.iter().find(|i|i.event_seq==original.seq).unwrap();
        let receipt=inbox.iter().find(|i|i.event_seq==result.seq).unwrap();
        if delivered {
            assert_eq!(original.state,"cancelled");assert_eq!(receipt.state,"cancelled");
            assert!(original.error.as_deref().unwrap().contains("续接已停止"));
        } else {
            assert_eq!(original.state,"waiting");assert_eq!(receipt.state,"queued");
            let next=delivery::prepare_execution(&mut c,b.conversation_id).unwrap().unwrap();
            assert_ne!(next.id,mixed.id);
            assert!(next.payload.contains("乙的原工作"));
        }
    }
}
