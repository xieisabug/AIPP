use chrono::{TimeZone, Utc};

use crate::api::ai::summary::get_latest_branch_messages;
use crate::db::conversation_db::Message;

fn build_message(
    id: i64,
    message_type: &str,
    created_time: chrono::DateTime<Utc>,
    generation_group_id: Option<&str>,
    parent_group_id: Option<&str>,
) -> Message {
    Message {
        id,
        parent_id: None,
        conversation_id: 1,
        message_type: message_type.to_string(),
        content: format!("message-{}", id),
        llm_model_id: None,
        llm_model_name: None,
        created_time,
        start_time: None,
        finish_time: None,
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: generation_group_id.map(str::to_string),
        parent_group_id: parent_group_id.map(str::to_string),
        tool_calls_json: None,
        first_token_time: None,
        ttft_ms: None,
    }
}

/// Test that the latest branch keeps the full flow when there is no regeneration.
#[test]
fn test_latest_branch_without_regenerate_keeps_all_messages() {
    let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();
    let t3 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 2).unwrap();
    let t4 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 3).unwrap();
    let t5 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 4).unwrap();

    let messages = vec![
        (build_message(4, "user", t4, None, None), None),
        (build_message(1, "system", t1, None, None), None),
        (build_message(5, "response", t5, Some("g2"), None), None),
        (build_message(2, "user", t2, None, None), None),
        (build_message(3, "response", t3, Some("g1"), None), None),
    ];

    let result = get_latest_branch_messages(&messages);
    let ids: Vec<i64> = result.iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![1, 2, 3, 4, 5]);
}

/// Test that a regenerated response replaces the original branch tail.
#[test]
fn test_latest_branch_truncates_on_regenerate_response() {
    let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();
    let t3 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 2).unwrap();
    let t4 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 3).unwrap();
    let t5 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 4).unwrap();
    let t6 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 5).unwrap();

    let messages = vec![
        (build_message(1, "system", t1, None, None), None),
        (build_message(2, "user", t2, None, None), None),
        (build_message(3, "response", t3, Some("g1"), None), None),
        (build_message(4, "user", t4, None, None), None),
        (build_message(5, "response", t5, Some("g2"), None), None),
        (build_message(6, "response", t6, Some("g2b"), Some("g2")), None),
    ];

    let result = get_latest_branch_messages(&messages);
    let ids: Vec<i64> = result.iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![1, 2, 3, 4, 6]);
}

/// Test that regenerating an earlier response truncates later messages.
#[test]
fn test_latest_branch_truncates_after_regenerate_earlier_response() {
    let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();
    let t3 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 2).unwrap();
    let t4 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 3).unwrap();
    let t5 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 4).unwrap();
    let t6 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 5).unwrap();
    let t7 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 6).unwrap();
    let t8 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 7).unwrap();

    let messages = vec![
        (build_message(1, "system", t1, None, None), None),
        (build_message(2, "user", t2, None, None), None),
        (build_message(3, "response", t3, Some("g1"), None), None),
        (build_message(4, "user", t4, None, None), None),
        (build_message(5, "response", t5, Some("g2"), None), None),
        (build_message(6, "user", t6, None, None), None),
        (build_message(7, "response", t7, Some("g3"), None), None),
        (build_message(8, "response", t8, Some("g1b"), Some("g1")), None),
    ];

    let result = get_latest_branch_messages(&messages);
    let ids: Vec<i64> = result.iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![1, 2, 8]);
}
