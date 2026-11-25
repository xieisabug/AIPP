use super::*;
use crate::api::ai::agent_driver::{registry::registry, response_message};
use crate::db::conversation_db::{MessageRepository, Repository};

#[allow(dead_code)]
#[path = "../../../db/tests/test_helpers.rs"]
mod database_fixture;

#[test]
fn test_background_wake_keeps_driver_and_parent_in_original_conversation() {
    let repo = MessageRepository::new(database_fixture::create_test_db());
    for (driver, origin) in [
        ("claude_sdk", "claude_background_wake"), ("codex_app_server", "native_background_wake"),
        ("pi", "native_background_wake"), ("opencode", "native_background_wake"),
    ] {
        let metadata = registry().driver(driver).unwrap().metadata();
        let identity = AgentEventIdentity::new(driver, 42);
        let mut previous = response_message(42, metadata);
        previous.content = "等待后台任务".into();
        previous.finish_time = Some(chrono::Utc::now());
        previous.parent_id = Some(5);
        let parent = repo.create(&previous).unwrap();
        let id = insert_reply(&repo, &identity, parent.id, metadata).unwrap();
        let wake = repo.read(id).unwrap().unwrap();
        assert_ne!(id, parent.id);
        assert_eq!(wake.parent_id, Some(parent.id));
        assert_eq!(wake.conversation_id, 42);
        assert!(wake.finish_time.is_none());
        assert_eq!(wake.llm_model_name, parent.llm_model_name);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&wake.metadata_json.unwrap()).unwrap()["origin"], origin);
        assert_eq!(repo.read(parent.id).unwrap().unwrap().content, previous.content);
        let foreign = AgentEventIdentity::new(driver, 43);
        assert!(insert_reply(&repo, &foreign, parent.id, metadata).is_err());
    }
}
