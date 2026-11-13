use super::*;

fn stream(event: Value) -> Value { json!({"type":"stream_event","event":event}) }

#[test]
fn test_claude_activity_thinking_stream_and_final_message_do_not_duplicate() {
    let mut tracker = ClaudeActivities::default();
    tracker.consume(&stream(json!({"type":"message_start"})), 0);
    tracker.consume(&stream(json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}})), 0);
    tracker.consume(&stream(json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"before"}})), 0);
    tracker.consume(&stream(json!({"type":"content_block_stop","index":0})), 0);
    tracker.consume(&json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":"before"},{"type":"tool_use","id":"shell","name":"Bash","input":{"command":"echo hi"}}]}}), 2);
    assert_eq!(tracker.items.len(), 2);
    let thought = tracker.items.values().find(|item|item["kind"] == "reasoning").unwrap();
    assert_eq!(thought["output"], "before");
    assert_eq!(thought["status"], "success");
    assert_eq!(thought["order"], 0);
    assert_eq!(tracker.items["shell"]["order"], 1);
    tracker.consume(&stream(json!({"type":"message_start"})), 2);
    tracker.consume(&stream(json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"after"}})), 2);
    assert_eq!(tracker.items.values().filter(|item|item["kind"] == "reasoning").count(), 2);
}

#[test]
fn test_claude_activity_final_only_thinking_and_terminal_tools_preserve_invocation() {
    let mut tracker = ClaudeActivities::default();
    tracker.consume(&json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":"native thought"},{"type":"tool_use","id":"read","name":"Read","input":{"file_path":"a.rs"}}]}}), 0);
    tracker.consume(&json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"read","content":[{"type":"text","text":"content"}]}]}}), 5);
    let item = &tracker.items["read"];
    assert_eq!(item["kind"], "file_read");
    assert_eq!(item["title"], "读取 a.rs");
    assert_eq!(item["input"]["file_path"], "a.rs");
    assert_eq!(item["output"], "content");
    assert_eq!(item["order"], 1);
    assert_eq!(item["content_offset"], 0);
    assert_eq!(item["status"], "success");
    assert!(item["metadata"]["rawResult"].is_array());
    tracker.consume(&json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"read","name":"Read","input":{"file_path":"a.rs"}}]}}), 5);
    assert_eq!(tracker.items["read"]["status"], "success");
    tracker.consume(&json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":"second thought"}]}}), 5);
    assert_eq!(tracker.items.values().filter(|item|item["kind"] == "reasoning").count(), 2);
}

#[test]
fn test_claude_activity_streamed_tool_arguments_and_partial_failures() {
    let mut tracker = ClaudeActivities::default();
    tracker.consume(&stream(json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"edit","name":"Edit","input":{}}})), 0);
    for partial in ["{\"file_path\":", "\"a.rs\"}"] {
        tracker.consume(&stream(json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":partial}})), 0);
    }
    tracker.consume(&stream(json!({"type":"content_block_stop","index":1})), 0);
    assert_eq!(tracker.items["edit"]["title"], "编辑 a.rs");
    tracker.consume(&json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"edit","is_error":true,"content":"ENOENT"}]}}), 0);
    assert_eq!(tracker.items["edit"]["error"], "ENOENT");
    tracker.finish(true, None);
    assert_eq!(tracker.items["edit"]["status"], "failed");
}

#[test]
fn test_claude_activity_cancel_settles_pending_operations_without_fake_success() {
    let mut tracker = ClaudeActivities::default();
    tracker.consume(&json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"shell","name":"Bash","input":{"command":"sleep 10"}}]}}), 0);
    tracker.finish(true, None);
    assert_eq!(tracker.items["shell"]["status"], "cancelled");
    assert!(tracker.items["shell"]["finished_at"].is_string());
}

#[test]
fn test_claude_activity_plans_keep_their_dedicated_renderer() {
    let mut tracker = ClaudeActivities::default();
    tracker.consume(&json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"plan","name":"TodoWrite","input":{"todos":[]}}]}}), 0);
    tracker.consume(&json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"plan","content":"ok"}]}}), 0);
    assert!(tracker.items.is_empty());
}

#[test]
fn test_claude_activity_full_thought_keeps_message_start_position_when_only_text_was_streamed() {
    let mut tracker = ClaudeActivities::default();
    tracker.consume(&stream(json!({"type":"message_start"})), 4);
    tracker.consume(&stream(json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"answer"}})), 4);
    tracker.consume(&json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":"before answer"},{"type":"text","text":"answer"},{"type":"tool_use","id":"read","name":"Read","input":{"file_path":"a"}}]}}), 10);
    let thought = tracker.items.values().find(|item|item["kind"] == "reasoning").unwrap();
    assert_eq!(thought["content_offset"], 4);
    assert_eq!(tracker.items["read"]["content_offset"], 10);
}
