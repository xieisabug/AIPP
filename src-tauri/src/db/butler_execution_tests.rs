use super::*;

#[test]
fn test_agent_final_retains_only_the_last_output_with_unicode_offsets() {
    let body = "先查🔎再看。答复";
    for answer in ["答复", "", "执行完成时单独返回的答复"] {
        let mut meta = serde_json::json!({});
        record_agent_final(&mut meta, body, answer);
        assert_eq!(agent_final_text(body, Some(&meta.to_string())).as_deref(), Some(answer));
    }
    let legacy = serde_json::json!({"agent_activities":[
        {"kind":"tool","content_offset":6}, {"kind":"tool","content_offset":3},
        {"kind":"status","content_offset":8}
    ]});
    assert_eq!(agent_final_text(body, Some(&legacy.to_string())).as_deref(), Some("答复"));
    let mut meta = serde_json::json!({"agent_activities":legacy["agent_activities"]});
    record_agent_final(&mut meta, body, body);
    assert_eq!(agent_final_text(body, Some(&meta.to_string())), None);
    assert_eq!(agent_final_text(body, Some(r#"{"agent_final":{"start":999}}"#)), None);
    assert_eq!(agent_final_text(body, None), None);
}
#[test] fn test_execution_three_adapters_share_completion_and_cancel_contract(){
 for identity in [Identity::task(3),Identity::peer(7,4),Identity::scheduled(5,"r")] {
  assert_eq!(Outcome::finish(identity.clone(),"success","finished",None,false,"pending").end,End::Unknown);
  assert_eq!(Outcome::finish(identity.clone(),"success","finished",Some("artifact".into()),true,"pending").end,End::Completed);
  assert_eq!(Outcome::finish(identity,"success","cancelled",Some("late".into()),true,"pending").end,End::Cancelled);
 }
}
#[test] fn test_execution_identity_is_stable_scoped_and_report_is_not_execution_retry(){
 assert_eq!(Identity::scheduled(3,"r"),Identity::scheduled(3,"r"));
 assert_ne!(Identity::scheduled(3,"r").execution_id,Identity::scheduled(4,"r").execution_id);
 assert_ne!(Identity::task(3).execution_id,Identity::peer(3,3).execution_id);
 assert!(!may_retry_report(Some("done")));assert!(!may_retry_report(Some("uncertain")));assert!(may_retry_report(Some("retry")));
}
#[test] fn test_execution_does_not_treat_partial_tool_or_silent_response_as_result(){
 assert!(is_completed_content("response","done",true,None));
 for (role,body,finished,calls) in [("response","partial",false,None),("tool_result","ok",true,None),("response","[[NO_REPLY]]",true,None),("response","tool",true,Some("[{\"id\":1}]"))] {
  assert!(!is_completed_content(role,body,finished,calls));
 }
}

#[test] fn test_dispatch_reservation_does_not_replay_uncertain_or_changed_requests(){
 let c=rusqlite::Connection::open_in_memory().unwrap();
 assert_eq!(reserve_dispatch(&c,1,"key","args").unwrap(),None);
 assert!(reserve_dispatch(&c,1,"key","args").is_err());
 bind_dispatch(&c,1,"key","response").unwrap();
 assert_eq!(reserve_dispatch(&c,1,"key","args").unwrap(),Some("response".into()));
 assert!(reserve_dispatch(&c,1,"key","changed").is_err());
 assert_eq!(reserve_dispatch(&c,2,"key","args").unwrap(),None);
}
