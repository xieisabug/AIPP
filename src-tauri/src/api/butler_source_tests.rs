use super::resolve_continuation_source;
use crate::db::mcp_db::MCPToolCall;
fn call(id:i64,conversation:i64,native:Option<&str>,source:Option<i64>)->MCPToolCall {
    serde_json::from_value(serde_json::json!({"id":id,"conversation_id":conversation,"message_id":source,
        "assistant_message_id":source.map(|id|id+1),"server_id":1,"server_name":"test","tool_name":"read",
        "parameters":"{}","status":"success","created_time":"now","llm_call_id":native})).unwrap()
}
#[test]
fn public_continuation_uses_exact_call_not_newest_turn() {
    let calls=vec![call(1,10,Some("old-call"),Some(11)),call(2,10,Some("new-call"),Some(21))];
    assert_eq!(resolve_continuation_source(&calls,10,"old-call").unwrap(),12);
    assert!(resolve_continuation_source(&calls,20,"old-call").is_err());
    assert!(resolve_continuation_source(&calls,10,"unknown").is_err());
}
#[test]
fn public_continuation_rejects_reused_ids_and_missing_anchors() {
    let calls=vec![call(1,10,Some("same"),Some(11)),call(2,10,Some("same"),Some(21))];
    assert!(resolve_continuation_source(&calls,10,"same").is_err());
    assert!(resolve_continuation_source(&[call(1,10,None,None)],10,"mcp_tool_call_1").is_err());
    assert_eq!(resolve_continuation_source(&[call(1,10,None,Some(11))],10,"mcp_tool_call_1").unwrap(),12);
}
