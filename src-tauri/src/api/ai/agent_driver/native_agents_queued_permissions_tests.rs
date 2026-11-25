use super::*;
use crate::api::ai::agent_driver::{next_permissions, queued_permission_fixture::Queue};

async fn observe(lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>) -> Value {
    let line = timeout(Duration::from_secs(10), lines.next_line()).await.expect("native peer timeout")
        .unwrap().expect("native peer closed before reporting the request");
    serde_json::from_str(&line).unwrap()
}

async fn two_turns(driver: &str, key: &str, old: &str, intermediate: &str, final_value: &str, restart: bool, reject: bool) {
    let queue = Queue::new();
    let provider = 3;
    let mut config = configuration(&[], &[], None).unwrap();
    config.load_dispatch_permissions(&queue.conn, queue.cid, driver, provider).unwrap();
    let mut first_signature = config.wire.clone();
    config.live_permission(driver, &mut first_signature);

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/api/ai/agent_driver/native_agents/queued-permissions-fixture.mjs");
    let mut child = tokio::process::Command::new("node").arg(fixture).arg(driver)
        .env_remove("OPENCODE_PERMISSION").env_remove("OPENCODE_CONFIG_CONTENT")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).kill_on_drop(true).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    write_frame(&mut input, &json!({"config":config.wire,"text":"first"})).await.unwrap();
    let first = observe(&mut lines).await;
    let session = first["session"].as_str().unwrap();
    let first_prompt = json!([[session, "first", old]]);
    assert_eq!(first["sent"], first_prompt);
    queue.db.upsert_agent_session_id_for_instance(queue.cid, driver, Some(provider), session).unwrap();
    let id = queue.enqueue("{}");
    for value in [intermediate, final_value] {
        next_permissions::save(&queue.conn, queue.cid, driver, provider, session, key, value).unwrap();
    }
    write_frame(&mut input, &json!({"type":"finish"})).await.unwrap();
    assert_eq!(observe(&mut lines).await, json!({"done":true,"sent":first_prompt}));

    let second = queue.dequeue(id);
    let mut config = configuration(&[], &[], None).unwrap();
    config.load_dispatch_permissions(&queue.conn, second.conversation_id, driver, provider).unwrap();
    let mut signature = config.wire.clone();
    let control = config.live_permission(driver, &mut signature);
    let needs_restart = signature != first_signature;
    assert_eq!(needs_restart, restart, "{driver} {final_value}");
    if needs_restart {
        config.wire["session"] = json!(queue.db.get_agent_session_id_for_instance(queue.cid, driver, Some(provider)).unwrap());
    }
    write_frame(&mut input, &json!({"restart":needs_restart,"control":control,"config":config.wire,"text":second.prompt,"reject":reject})).await.unwrap();
    let observed = observe(&mut lines).await;
    if reject {
        let reason = observed["error"].as_str().expect("a rejected configuration must fail");
        assert!(reason.contains("fixture rejected") || reason.contains("session file missing") || reason.contains("did not apply"), "{reason}");
        assert_eq!(observed["sent"], first_prompt);
    } else {
        assert_eq!(observed["sent"], json!([[session, "first", old], [session, "second", final_value]]), "{driver}: {observed}");
    }
    drop(input);
    assert!(timeout(Duration::from_secs(5), child.wait()).await.unwrap().unwrap().success());
}

async fn check(driver: &str, key: &str, old: &str, intermediate: &str, final_value: &str, restart: bool) {
    for reject in [false, true] { two_turns(driver, key, old, intermediate, final_value, restart, reject).await; }
}

#[tokio::test]
async fn test_queued_permissions_pi() { check("pi", "pi_approval", "all", "none", "read_only", true).await; }
#[tokio::test]
async fn test_queued_permissions_cursor() { check("cursor", "cursor_auto_review", "on", "on", "off", true).await; }
#[tokio::test]
async fn test_queued_permissions_opencode() { check("opencode", "opencode_permission", "default", "ask", "deny", true).await; }
#[tokio::test]
async fn test_queued_permissions_kiro() { check("kiro", "autopilot", "off", "off", "on", false).await; }
#[tokio::test]
async fn test_queued_permissions_antigravity() {
    check("antigravity", "antigravity_mode", "default", "plan", "auto", false).await;
    check("antigravity", "antigravity_mode", "default", "auto", "yolo", true).await;
}
