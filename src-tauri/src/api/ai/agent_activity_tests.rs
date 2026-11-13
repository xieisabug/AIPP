use super::*;
#[allow(dead_code)]
#[path = "../../db/tests/test_helpers.rs"]
mod database_fixture;

#[test]
fn test_agent_activity_terminal_updates_preserve_arguments_and_order() {
    let first = merge_activity(None, &json!({"id":"a","kind":"tool","status":"running","title":"Bash","input":{"command":"echo hi"},"metadata":{"tool":"Bash"}}), 2, 0);
    let mut done = merge_activity(Some(&first), &json!({"id":"a","kind":"tool","status":"success","title":null,"input":null,"output":"hi"}), 99, 8);
    normalize_tool_activity("claude_sdk", &mut done);
    assert_eq!(done["kind"], "command");
    assert_eq!(done["title"], "echo hi");
    assert_eq!(done["input"]["command"], "echo hi");
    assert_eq!(done["content_offset"], 2);
    assert_eq!(done["order"], 0);
    assert_eq!(done["output"], "hi");
    assert!(done["finished_at"].is_string());
}

#[test]
fn test_agent_activity_partial_metadata_updates_preserve_running_state() {
    let first = merge_activity(None, &json!({"id":"a","kind":"tool","status":"running","input":{"path":"a"}}), 0, 0);
    let next = merge_activity(Some(&first), &json!({"id":"a","metadata":{"native_kind":"read"}}), 9, 4);
    assert_eq!(next["status"], "executing");
    assert_eq!(next["input"]["path"], "a");
}

#[test]
fn test_agent_activity_late_progress_does_not_reopen_a_finished_operation() {
    let done = merge_activity(None, &json!({"id":"a","kind":"command","status":"success","input":{"command":"pwd"}}), 0, 0);
    let late = merge_activity(Some(&done), &json!({"id":"a","status":"running","output":"late output"}), 9, 3);
    assert_eq!(late["status"], "success");
    assert_eq!(late["finished_at"], done["finished_at"]);
    assert_eq!(late["order"], 0);
}

#[test]
fn test_agent_activity_classification_uses_native_identity_and_real_diffs() {
    for (driver, tool, input, expected) in [
        ("claude_sdk", "Bash", json!({"command":"pwd"}), "command"),
        ("claude_sdk", "Read", json!({"file_path":"a.rs"}), "file_read"),
        ("cursor", "shell", json!({"command":"pwd"}), "command"),
        ("cursor", "delete", json!({"path":"a"}), "file_delete"),
        ("cursor", "glob", json!({"targetDirectory":"src","globPattern":"*.rs"}), "file_search"),
        ("opencode", "read", json!({"filePath":"a.rs"}), "file_read"),
        ("opencode", "write", json!({"filePath":"existing.rs"}), "file_write"),
        ("opencode", "task", json!({"prompt":"investigate"}), "sub_agent"),
    ] {
        let mut item = json!({"kind":"tool","title":tool,"input":input,"metadata":{"tool":tool}});
        normalize_tool_activity(driver, &mut item);
        assert_eq!(item["kind"], expected, "{driver}/{tool}");
        assert!(item["metadata"].get("changes").is_none());
    }
    let mut edit = json!({"kind":"tool","input":{"path":"a.rs"},"metadata":{"tool":"edit","diff":"+actual"}});
    normalize_tool_activity("cursor", &mut edit);
    assert_eq!(edit["kind"], "patch");
    assert_eq!(edit["metadata"]["changes"][0]["diff"], "+actual");
    for driver in ["kiro", "antigravity"] {
        for (native, expected) in [("execute","command"),("read","file_read"),("search","file_search"),("edit","file_write"),("delete","file_delete")] {
            let mut item = json!({"kind":"tool","title":"arbitrary localized name","input":{"path":"a","command":"pwd"},"metadata":{"native_kind":native}});
            normalize_tool_activity(driver, &mut item);
            assert_eq!(item["kind"], expected);
        }
    }
}

#[test]
fn test_agent_activity_mcp_and_unknown_tools_are_not_misclassified() {
    for (tool, server) in [("read",Some("remote")),("mcp__server__read",None),("my_custom_read",None)] {
        let mut item = json!({"kind":"tool","title":tool,"input":{"path":"remote"},"metadata":{"tool":tool}});
        if let Some(server) = server { item["metadata"]["server"] = json!(server); }
        normalize_tool_activity("claude_sdk", &mut item);
        assert_eq!(item["kind"], "tool");
    }
    let mut extension = json!({"kind":"tool","title":"read_file","input":{"path":"remote"},"metadata":{"tool":"read_file"}});
    normalize_tool_activity("opencode", &mut extension);
    assert_eq!(extension["kind"], "tool");
}

#[test]
fn test_agent_activity_history_round_trip_preserves_other_metadata_and_order() {
    let repo = MessageRepository::new(database_fixture::create_test_db());
    let mut message = database_fixture::create_test_message(1, "response", "answer", None, None);
    message.metadata_json = Some(json!({"usage_source":"reported","other":{"keep":true}}).to_string());
    let message = repo.create(&message).unwrap();
    let mut activities = HashMap::new();
    activities.insert("later".into(), json!({"item_id":"later","kind":"command","order":1,"sequence":2}));
    activities.insert("first".into(), json!({"item_id":"first","kind":"reasoning","order":0,"sequence":500}));
    persist_activities(&repo, message.id, &activities).unwrap();
    let saved = repo.read(message.id).unwrap().unwrap();
    let metadata: Value = serde_json::from_str(saved.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(saved.content, "answer");
    assert_eq!(metadata["usage_source"], "reported");
    assert_eq!(metadata["other"]["keep"], true);
    assert_eq!(metadata["agent_activities"][0]["item_id"], "first");
    assert_eq!(metadata["agent_activities"][1]["item_id"], "later");
}
