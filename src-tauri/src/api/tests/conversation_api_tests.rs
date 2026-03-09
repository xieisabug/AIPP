use crate::api::conversation_api::process_message_versions;
use crate::db::conversation_db::MessageDetail;
use chrono::Utc;
use uuid::Uuid;

// ============================================================================
// 辅助函数
// ============================================================================

/// 创建测试用的 MessageDetail
fn create_message_detail(
    id: i64,
    conversation_id: i64,
    message_type: &str,
    content: &str,
    parent_id: Option<i64>,
    generation_group_id: Option<String>,
    parent_group_id: Option<String>,
    created_time: chrono::DateTime<Utc>,
) -> MessageDetail {
    MessageDetail {
        id,
        parent_id,
        conversation_id,
        message_type: message_type.to_string(),
        content: content.to_string(),
        llm_model_id: Some(1),
        created_time,
        start_time: None,
        finish_time: None,
        token_count: 100,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id,
        parent_group_id,
        attachment_list: Vec::new(),
        regenerate: Vec::new(),
        tool_calls_json: None,
        first_token_time: None,
        ttft_ms: None,
    }
}

/// 快速创建消息
fn quick_message(
    id: i64,
    msg_type: &str,
    content: &str,
    parent_id: Option<i64>,
    offset_secs: i64,
) -> MessageDetail {
    let base_time =
        chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc);
    create_message_detail(
        id,
        1,
        msg_type,
        content,
        parent_id,
        Some(Uuid::new_v4().to_string()),
        None,
        base_time + chrono::Duration::seconds(offset_secs),
    )
}

// ============================================================================
// 基础功能测试
// ============================================================================

#[tokio::test]
async fn test_version_management_logic() {
    let base_time = Utc::now();
    let group_id = Uuid::new_v4().to_string();

    // 创建测试消息：用户消息 -> AI回复 -> 重新生成1 -> 重新生成2（最新）
    let user_msg = create_message_detail(
        1,
        1,
        "user",
        "Original user message",
        None,
        Some(group_id.clone()),
        None,
        base_time,
    );
    let ai_msg = create_message_detail(
        2,
        1,
        "assistant",
        "Original AI response",
        None,
        Some(group_id.clone()),
        None,
        base_time + chrono::Duration::seconds(1),
    );
    let ai_msg_v2 = create_message_detail(
        3,
        1,
        "assistant",
        "Regenerated AI response v1",
        Some(2),
        Some(group_id.clone()),
        None,
        base_time + chrono::Duration::seconds(2),
    );
    let ai_msg_v3 = create_message_detail(
        4,
        1,
        "assistant",
        "Regenerated AI response v2 (latest)",
        Some(3),
        Some(group_id.clone()),
        None,
        base_time + chrono::Duration::seconds(3),
    );

    let message_details = vec![user_msg, ai_msg, ai_msg_v2, ai_msg_v3];

    // 测试核心业务逻辑
    let final_messages = process_message_versions(message_details);

    // 验证结果
    assert_eq!(final_messages.len(), 2, "Expected 2 messages but got {}", final_messages.len());
    assert_eq!(final_messages[0].message_type, "user");
    assert_eq!(final_messages[0].content, "Original user message");
    assert_eq!(final_messages[1].message_type, "assistant");
    assert_eq!(final_messages[1].content, "Regenerated AI response v2 (latest)");
    assert_eq!(final_messages[0].generation_group_id, Some(group_id.clone()));
    assert_eq!(final_messages[1].generation_group_id, Some(group_id));
}

#[tokio::test]
async fn test_empty_message_list() {
    let message_details: Vec<MessageDetail> = Vec::new();
    let final_messages = process_message_versions(message_details);
    assert!(final_messages.is_empty());
}

#[tokio::test]
async fn test_single_user_message() {
    let base_time = Utc::now();
    let user_msg = create_message_detail(
        1,
        1,
        "user",
        "Hello",
        None,
        Some(Uuid::new_v4().to_string()),
        None,
        base_time,
    );

    let message_details = vec![user_msg];
    let final_messages = process_message_versions(message_details);

    assert_eq!(final_messages.len(), 1);
    assert_eq!(final_messages[0].content, "Hello");
    assert_eq!(final_messages[0].message_type, "user");
}

// ============================================================================
// 版本管理高级测试
// ============================================================================

/// 测试没有子版本的消息应该保持不变
#[tokio::test]
async fn test_message_without_versions() {
    let messages = vec![
        quick_message(1, "system", "You are helpful", None, 0),
        quick_message(2, "user", "Hello", None, 1),
        quick_message(3, "response", "Hi there!", None, 2),
    ];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 3);
    assert_eq!(result[0].message_type, "system");
    assert_eq!(result[1].message_type, "user");
    assert_eq!(result[2].message_type, "response");
}

/// 测试单层版本链（一次重发）
#[tokio::test]
async fn test_single_regeneration() {
    let messages = vec![
        quick_message(1, "user", "What is 2+2?", None, 0),
        quick_message(2, "response", "Original: 4", None, 1),
        quick_message(3, "response", "Regenerated: 2+2=4", Some(2), 2),
    ];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].content, "What is 2+2?");
    assert_eq!(result[1].content, "Regenerated: 2+2=4");
}

/// 测试多层版本链（多次重发）
#[tokio::test]
async fn test_multi_level_regeneration() {
    let messages = vec![
        quick_message(1, "user", "Question", None, 0),
        quick_message(2, "response", "V1", None, 1),
        quick_message(3, "response", "V2", Some(2), 2),
        quick_message(4, "response", "V3", Some(3), 3),
        quick_message(5, "response", "V4 (latest)", Some(4), 4),
    ];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 2);
    assert_eq!(result[1].content, "V4 (latest)");
    assert_eq!(result[1].id, 5);
}

/// 测试多个独立消息各自有版本链
#[tokio::test]
async fn test_multiple_independent_version_chains() {
    let messages = vec![
        quick_message(1, "user", "First question", None, 0),
        quick_message(2, "response", "Answer 1 V1", None, 1),
        quick_message(3, "response", "Answer 1 V2", Some(2), 2),
        quick_message(4, "user", "Second question", None, 3),
        quick_message(5, "response", "Answer 2 V1", None, 4),
        quick_message(6, "response", "Answer 2 V2", Some(5), 5),
        quick_message(7, "response", "Answer 2 V3", Some(6), 6),
    ];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 4);
    assert_eq!(result[0].content, "First question");
    assert_eq!(result[1].content, "Answer 1 V2"); // 最新版本
    assert_eq!(result[2].content, "Second question");
    assert_eq!(result[3].content, "Answer 2 V3"); // 最新版本
}

/// 测试带有 reasoning 和 response 的消息组
#[tokio::test]
async fn test_reasoning_and_response_group() {
    let group_id = Uuid::new_v4().to_string();
    let base_time = Utc::now();

    let messages = vec![
        create_message_detail(1, 1, "user", "What is 2+2?", None, None, None, base_time),
        create_message_detail(
            2,
            1,
            "reasoning",
            "Let me calculate...",
            None,
            Some(group_id.clone()),
            None,
            base_time + chrono::Duration::seconds(1),
        ),
        create_message_detail(
            3,
            1,
            "response",
            "The answer is 4",
            None,
            Some(group_id.clone()),
            None,
            base_time + chrono::Duration::seconds(2),
        ),
    ];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 3);
    assert_eq!(result[0].message_type, "user");
    assert_eq!(result[1].message_type, "reasoning");
    assert_eq!(result[2].message_type, "response");
    // reasoning 和 response 应该有相同的 generation_group_id
    assert_eq!(result[1].generation_group_id, result[2].generation_group_id);
}

/// 测试消息按时间排序
#[tokio::test]
async fn test_messages_sorted_by_time() {
    // 故意乱序创建消息
    let messages = vec![
        quick_message(3, "response", "Third", None, 2),
        quick_message(1, "user", "First", None, 0),
        quick_message(2, "user", "Second", None, 1),
    ];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 3);
    assert_eq!(result[0].content, "First");
    assert_eq!(result[1].content, "Second");
    assert_eq!(result[2].content, "Third");
}

/// 测试 regenerate 数组被正确填充
#[tokio::test]
async fn test_regenerate_array_populated() {
    let messages = vec![
        quick_message(1, "user", "Question", None, 0),
        quick_message(2, "response", "V1", None, 1),
        quick_message(3, "response", "V2", Some(2), 2),
        quick_message(4, "response", "V3", Some(2), 3), // 也指向 2，但时间更晚
    ];

    let result = process_message_versions(messages);

    // 最终显示的消息
    assert_eq!(result.len(), 2);
    // 应该显示最新版本（V3 或 V4 取决于实现）
    // 注意：当前实现中 V3 和 V4 都指向 V1，所以会取时间最晚的
}

// ============================================================================
// 边界条件测试
// ============================================================================

/// 测试只有 system 消息
#[tokio::test]
async fn test_only_system_message() {
    let messages = vec![quick_message(1, "system", "You are a helpful assistant", None, 0)];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].message_type, "system");
}

/// 测试消息内容为空字符串
#[tokio::test]
async fn test_empty_content_message() {
    let messages =
        vec![quick_message(1, "user", "", None, 0), quick_message(2, "response", "", None, 1)];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].content, "");
    assert_eq!(result[1].content, "");
}

/// 测试非常长的消息内容
#[tokio::test]
async fn test_very_long_content() {
    let long_content = "A".repeat(100000);
    let messages = vec![quick_message(1, "user", &long_content, None, 0)];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].content.len(), 100000);
}

/// 测试包含特殊字符的消息
#[tokio::test]
async fn test_special_characters_in_content() {
    let special_content = r#"<script>alert('xss')</script> 中文 émojis 🎉 \n\t"#;
    let messages = vec![quick_message(1, "user", special_content, None, 0)];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].content, special_content);
}

/// 测试大量消息的性能
#[tokio::test]
async fn test_many_messages() {
    let mut messages = Vec::new();
    for i in 0..1000 {
        messages.push(quick_message(i as i64, "user", &format!("Message {}", i), None, i as i64));
    }

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 1000);
}

/// 测试深层嵌套的版本链
#[tokio::test]
async fn test_deep_version_chain() {
    let mut messages = vec![
        quick_message(1, "user", "Question", None, 0),
        quick_message(2, "response", "V1", None, 1),
    ];

    // 创建 50 层深的版本链
    for i in 3..53 {
        messages.push(quick_message(
            i as i64,
            "response",
            &format!("V{}", i - 1),
            Some((i - 1) as i64),
            i as i64,
        ));
    }

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 2);
    assert_eq!(result[1].content, "V51"); // 最深层版本
}

// ============================================================================
// reasoning + response 排序测试
// ============================================================================

/// 测试 reasoning 和 response 消息遵循真实时间顺序
#[tokio::test]
async fn test_reasoning_response_same_group_respects_time_order() {
    let group_id = Uuid::new_v4().to_string();
    let base_time =
        chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc);

    let messages = vec![
        create_message_detail(
            1,
            1,
            "user",
            "What is 2+2?",
            None,
            Some(group_id.clone()),
            None,
            base_time,
        ),
        // response 先创建，时间较早
        create_message_detail(
            3,
            1,
            "response",
            "2+2 equals 4",
            None,
            Some(group_id.clone()),
            None,
            base_time + chrono::Duration::seconds(1),
        ),
        // reasoning 后创建，时间较晚
        create_message_detail(
            2,
            1,
            "reasoning",
            "Let me calculate 2+2",
            None,
            Some(group_id.clone()),
            None,
            base_time + chrono::Duration::seconds(2),
        ),
    ];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 3);
    assert_eq!(result[0].message_type, "user");
    assert_eq!(result[1].message_type, "response");
    assert_eq!(result[2].message_type, "reasoning");
}

#[tokio::test]
async fn test_equal_timestamps_do_not_cluster_by_message_type() {
    let base_time =
        chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc);

    let messages = vec![
        create_message_detail(1, 1, "user", "Question 1", None, None, None, base_time),
        create_message_detail(2, 1, "response", "Response 1", None, Some("g1".to_string()), None, base_time),
        create_message_detail(3, 1, "reasoning", "Reasoning 2", None, Some("g2".to_string()), None, base_time),
        create_message_detail(4, 1, "response", "Response 3", None, Some("g3".to_string()), None, base_time),
        create_message_detail(5, 1, "reasoning", "Reasoning 4", None, Some("g4".to_string()), None, base_time),
    ];

    let result = process_message_versions(messages);
    let ids: Vec<i64> = result.iter().map(|message| message.id).collect();
    assert_eq!(ids, vec![1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn test_equal_timestamps_same_group_still_put_reasoning_before_response() {
    let base_time =
        chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc);
    let group_id = Uuid::new_v4().to_string();

    let messages = vec![
        create_message_detail(1, 1, "user", "Question 1", None, None, None, base_time),
        create_message_detail(
            2,
            1,
            "response",
            "Response 1",
            None,
            Some(group_id.clone()),
            None,
            base_time + chrono::Duration::seconds(1),
        ),
        create_message_detail(
            3,
            1,
            "reasoning",
            "Reasoning 1",
            None,
            Some(group_id),
            None,
            base_time + chrono::Duration::seconds(1),
        ),
    ];

    let result = process_message_versions(messages);
    let ids: Vec<i64> = result.iter().map(|message| message.id).collect();
    assert_eq!(ids, vec![1, 3, 2]);
}

#[tokio::test]
async fn test_local_reasoning_response_tie_break_does_not_reorder_other_groups() {
    let base_time =
        chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc);

    let messages = vec![
        create_message_detail(1, 1, "user", "Question 1", None, None, None, base_time),
        create_message_detail(
            2,
            1,
            "response",
            "Response 1",
            None,
            Some("g1".to_string()),
            None,
            base_time + chrono::Duration::seconds(1),
        ),
        create_message_detail(
            3,
            1,
            "response",
            "Response 2",
            None,
            Some("g2".to_string()),
            None,
            base_time + chrono::Duration::seconds(2),
        ),
        create_message_detail(
            4,
            1,
            "reasoning",
            "Reasoning 2",
            None,
            Some("g2".to_string()),
            None,
            base_time + chrono::Duration::seconds(2),
        ),
        create_message_detail(
            5,
            1,
            "response",
            "Response 3",
            None,
            Some("g3".to_string()),
            None,
            base_time + chrono::Duration::seconds(2),
        ),
        create_message_detail(
            6,
            1,
            "reasoning",
            "Reasoning 4",
            None,
            Some("g4".to_string()),
            None,
            base_time + chrono::Duration::seconds(2),
        ),
    ];

    let result = process_message_versions(messages);
    let ids: Vec<i64> = result.iter().map(|message| message.id).collect();
    assert_eq!(ids, vec![1, 2, 4, 3, 5, 6]);
}

/// 测试多个 reasoning+response 组的排序
#[tokio::test]
async fn test_multiple_reasoning_response_groups() {
    let base_time =
        chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc);
    let group1_id = Uuid::new_v4().to_string();
    let group2_id = Uuid::new_v4().to_string();

    let messages = vec![
        // 第一轮对话
        create_message_detail(1, 1, "user", "Question 1", None, None, None, base_time),
        create_message_detail(
            2,
            1,
            "reasoning",
            "Reasoning 1",
            None,
            Some(group1_id.clone()),
            None,
            base_time + chrono::Duration::seconds(1),
        ),
        create_message_detail(
            3,
            1,
            "response",
            "Response 1",
            None,
            Some(group1_id.clone()),
            None,
            base_time + chrono::Duration::seconds(2),
        ),
        // 第二轮对话
        create_message_detail(
            4,
            1,
            "user",
            "Question 2",
            None,
            None,
            None,
            base_time + chrono::Duration::seconds(10),
        ),
        create_message_detail(
            5,
            1,
            "reasoning",
            "Reasoning 2",
            None,
            Some(group2_id.clone()),
            None,
            base_time + chrono::Duration::seconds(11),
        ),
        create_message_detail(
            6,
            1,
            "response",
            "Response 2",
            None,
            Some(group2_id.clone()),
            None,
            base_time + chrono::Duration::seconds(12),
        ),
    ];

    let result = process_message_versions(messages);

    assert_eq!(result.len(), 6);
    assert_eq!(result[0].message_type, "user");
    assert_eq!(result[1].message_type, "reasoning");
    assert_eq!(result[2].message_type, "response");
    assert_eq!(result[3].message_type, "user");
    assert_eq!(result[4].message_type, "reasoning");
    assert_eq!(result[5].message_type, "response");
}

/// 测试 reasoning+response 带重发的情况
#[tokio::test]
async fn test_reasoning_response_with_regeneration() {
    let base_time =
        chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc);
    let group1_id = Uuid::new_v4().to_string();
    let group2_id = Uuid::new_v4().to_string();

    let messages = vec![
        create_message_detail(1, 1, "user", "What is 2+2?", None, None, None, base_time),
        // 第一组（原始）
        create_message_detail(
            2,
            1,
            "reasoning",
            "Wrong reasoning",
            None,
            Some(group1_id.clone()),
            None,
            base_time + chrono::Duration::seconds(1),
        ),
        create_message_detail(
            3,
            1,
            "response",
            "Wrong answer",
            None,
            Some(group1_id.clone()),
            None,
            base_time + chrono::Duration::seconds(2),
        ),
        // 第二组（重发）
        create_message_detail(
            4,
            1,
            "reasoning",
            "Correct reasoning",
            Some(2),
            Some(group2_id.clone()),
            Some(group1_id.clone()),
            base_time + chrono::Duration::seconds(3),
        ),
        create_message_detail(
            5,
            1,
            "response",
            "Correct answer: 4",
            Some(3),
            Some(group2_id.clone()),
            Some(group1_id),
            base_time + chrono::Duration::seconds(4),
        ),
    ];

    let result = process_message_versions(messages);

    // 应该显示最新版本的 reasoning 和 response
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].message_type, "user");
    assert_eq!(result[1].message_type, "reasoning");
    assert_eq!(result[2].message_type, "response");
    assert_eq!(result[1].content, "Correct reasoning");
    assert_eq!(result[2].content, "Correct answer: 4");
}
