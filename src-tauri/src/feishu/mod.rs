mod api;
mod config;
mod debug;
mod events;
mod interaction;
mod relay;
mod runtime;
mod types;

// ── Public re-exports ──────────────────────────────────────────────

pub use debug::{
    debug_build_feishu_markdown_card, debug_build_feishu_interactive_payload,
    debug_describe_feishu_markdown_blocks,
};
pub use types::{FeishuButlerState, FeishuDebugSendResult, FeishuRuntimeStatus};

pub(crate) use config::{clear_feishu_secret, migrate_secure_storage_if_needed, save_feishu_secret};
pub(crate) use debug::resend_message_to_feishu_for_debug;
pub(crate) use interaction::try_deliver_ask_user_question_to_feishu;
pub(crate) use relay::{
    conversation_has_feishu_target, inherit_latest_feishu_target,
    maybe_schedule_butler_feishu_relay_for_aipp_turn,
};
pub(crate) use runtime::{get_runtime_status, refresh_runtime, refresh_runtime_async};
pub(crate) use api::{
    try_deliver_acp_permission_to_feishu, try_deliver_operation_permission_to_feishu,
};

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::api::*;
    use super::debug::*;
    use super::events::*;
    use super::interaction::*;
    use super::relay::*;
    use super::types::*;

    use chrono::Utc;
    use serde_json::{json, Map, Value};

    use crate::mcp::builtin_mcp::interaction::{AskUserQuestionItem, AskUserQuestionRequestEvent};

    #[test]
    fn split_markdown_blocks_extracts_table() {
        let blocks = split_markdown_into_feishu_blocks(
            "# Title\n\n| Name | Value |\n| --- | --- |\n| A | **1** |\n| B | [2](https://example.com) |\n\nTail",
        );

        assert_eq!(blocks.len(), 3);
        assert!(
            matches!(&blocks[0], FeishuCardBlock::Markdown(content) if content.contains("# Title"))
        );
        assert!(matches!(
            &blocks[1],
            FeishuCardBlock::Table(table)
                if table.headers == vec!["Name".to_string(), "Value".to_string()]
                && table.rows.len() == 2
        ));
        assert!(
            matches!(&blocks[2], FeishuCardBlock::Markdown(content) if content.contains("Tail"))
        );
    }

    #[test]
    fn split_markdown_blocks_ignores_table_inside_code_fence() {
        let blocks = split_markdown_into_feishu_blocks(
            "```markdown\n| Name | Value |\n| --- | --- |\n| A | B |\n```\n",
        );

        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], FeishuCardBlock::Markdown(content) if content.contains("```markdown"))
        );
    }

    #[test]
    fn build_feishu_markdown_card_uses_markdown_and_table_elements() {
        let card = build_feishu_markdown_card(
            "# Summary\n\n- item 1\n- item 2\n\n| Name | Status |\n| --- | --- |\n| A | ~~done~~ |\n",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0]["tag"], "markdown");
        assert_eq!(elements[1]["tag"], "table");
        assert_eq!(elements[1]["columns"][0]["display_name"], "Name");
        assert_eq!(elements[1]["rows"][0]["col_2"], "~~done~~");
    }

    #[test]
    fn parse_markdown_table_handles_alignment_escaped_pipes_and_irregular_rows() {
        let table = parse_markdown_table(&[
            "| Name | Value \\| Detail | Score |".to_string(),
            "| :--- | :------------- | ----: |".to_string(),
            "| Alice | `A\\|B` | 42 |".to_string(),
            "| Bob | plain |".to_string(),
            "| Carol | too | many | columns |".to_string(),
        ])
        .expect("table should parse");

        assert_eq!(
            table.headers,
            vec!["Name".to_string(), "Value | Detail".to_string(), "Score".to_string()]
        );
        assert_eq!(
            table.rows,
            vec![
                vec!["Alice".to_string(), "`A|B`".to_string(), "42".to_string()],
                vec!["Bob".to_string(), "plain".to_string(), String::new()],
                vec!["Carol".to_string(), "too".to_string(), "many".to_string()],
            ]
        );
    }

    #[test]
    fn split_markdown_blocks_keeps_invalid_table_like_text_as_markdown() {
        let blocks = split_markdown_into_feishu_blocks(
            "Value A | Value B\nThis line is not a markdown separator\nnext line",
        );

        assert_eq!(blocks.len(), 1);
        assert!(matches!(
            &blocks[0],
            FeishuCardBlock::Markdown(content)
                if content.contains("Value A | Value B")
                && content.contains("This line is not a markdown separator")
        ));
    }

    #[test]
    fn build_feishu_markdown_card_supports_multiple_tables_and_markdown_blocks() {
        let card = build_feishu_markdown_card(
            "前言\n\n| Key | Value |\n| --- | --- |\n| A | 1 |\n\n中间段落\n\n| Env | Status |\n| --- | --- |\n| Prod | **OK** |\n",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 4);
        assert_eq!(elements[0]["tag"], "markdown");
        assert_eq!(elements[1]["tag"], "table");
        assert_eq!(elements[2]["tag"], "markdown");
        assert_eq!(elements[3]["tag"], "table");
        assert_eq!(elements[3]["rows"][0]["col_2"], "**OK**");
    }

    #[test]
    fn build_feishu_markdown_card_preserves_complex_chinese_supplement_table() {
        let card = build_feishu_markdown_card(
            "| 补剂 | 证据强度 | 推荐剂量 | 关键注意事项 |\n\
             |------|----------|----------|--------------|\n\
             | **圣约翰草** | ⭐⭐⭐ 最强 | 900mg/日 (分3次) | ⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用 |\n\
             | **SAM-e** | ⭐⭐⭐ 强 | 800-1600mg/日 | ⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险 |\n\
             | **EPA鱼油** | ⭐⭐ 中等 | EPA 1-2g/日 | ⚠️与阿司匹林/华法林同服增加出血风险 |\n\
             | **藏红花** | ⭐⭐ 中等 | 30mg/日 | ⚠️孕妇禁用 |\n\
             | **维生素D** | ⭐⭐ 缺乏者有效 | 1000-4000 IU/日 | 建议先检测水平再补充 |\n\
             | **L-甲基叶酸** | ⭐⭐ 增效剂 | 7.5-15mg/日 | 配合抗抑郁药使用效果更佳 |\n\
             | **NAC** | ⭐⭐ 辅助 | 2000mg/日 | 哮喘患者慎用 |\n\
             | **锌** | ⭐ 初步 | 25-50mg/日 | 长期高剂量导致铜缺乏 |\n\
             | **5-HTP** | ⭐ 有限 | 100-300mg/日 | ⚠️与抗抑郁药同服有血清素综合征风险 |\n",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0]["tag"], "table");
        assert_eq!(elements[0]["columns"][0]["display_name"], "补剂");
        assert_eq!(elements[0]["columns"][3]["display_name"], "关键注意事项");
        assert_eq!(elements[0]["rows"].as_array().expect("rows should be array").len(), 9);
        assert_eq!(elements[0]["rows"][0]["col_1"], "**圣约翰草**");
        assert_eq!(elements[0]["rows"][0]["col_2"], "⭐⭐⭐ 最强");
        assert_eq!(
            elements[0]["rows"][0]["col_4"],
            "⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用"
        );
        assert_eq!(elements[0]["rows"][1]["col_1"], "**SAM-e**");
        assert_eq!(elements[0]["rows"][4]["col_2"], "⭐⭐ 缺乏者有效");
        assert_eq!(elements[0]["rows"][5]["col_4"], "配合抗抑郁药使用效果更佳");
        assert_eq!(elements[0]["rows"][8]["col_1"], "**5-HTP**");
        assert_eq!(elements[0]["rows"][8]["col_4"], "⚠️与抗抑郁药同服有血清素综合征风险");
    }

    #[test]
    fn build_feishu_markdown_card_matches_expected_supplement_table_schema() {
        let markdown = "| 补剂 | 证据强度 | 推荐剂量 | 关键注意事项 |\n\
                        |------|----------|----------|--------------|\n\
                        | **圣约翰草** | ⭐⭐⭐ 最强 | 900mg/日 (分3次) | ⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用 |\n\
                        | **SAM-e** | ⭐⭐⭐ 强 | 800-1600mg/日 | ⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险 |\n\
                        | **EPA鱼油** | ⭐⭐ 中等 | EPA 1-2g/日 | ⚠️与阿司匹林/华法林同服增加出血风险 |\n\
                        | **藏红花** | ⭐⭐ 中等 | 30mg/日 | ⚠️孕妇禁用 |\n\
                        | **维生素D** | ⭐⭐ 缺乏者有效 | 1000-4000 IU/日 | 建议先检测水平再补充 |\n\
                        | **L-甲基叶酸** | ⭐⭐ 增效剂 | 7.5-15mg/日 | 配合抗抑郁药使用效果更佳 |\n\
                        | **NAC** | ⭐⭐ 辅助 | 2000mg/日 | 哮喘患者慎用 |\n\
                        | **锌** | ⭐ 初步 | 25-50mg/日 | 长期高剂量导致铜缺乏 |\n\
                        | **5-HTP** | ⭐ 有限 | 100-300mg/日 | ⚠️与抗抑郁药同服有血清素综合征风险 |\n";
        let card = build_feishu_markdown_card(markdown).expect("card should be built");

        let expected = json!({
            "schema": "2.0",
            "body": {
                "elements": [
                    {
                        "tag": "table",
                        "element_id": "md_table_1",
                        "row_height": "low",
                        "page_size": 9,
                        "columns": [
                            { "name": "col_1", "display_name": "补剂", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_2", "display_name": "证据强度", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_3", "display_name": "推荐剂量", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_4", "display_name": "关键注意事项", "data_type": "lark_md", "width": "auto" }
                        ],
                        "rows": [
                            { "col_1": "**圣约翰草**", "col_2": "⭐⭐⭐ 最强", "col_3": "900mg/日 (分3次)", "col_4": "⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用" },
                            { "col_1": "**SAM-e**", "col_2": "⭐⭐⭐ 强", "col_3": "800-1600mg/日", "col_4": "⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险" },
                            { "col_1": "**EPA鱼油**", "col_2": "⭐⭐ 中等", "col_3": "EPA 1-2g/日", "col_4": "⚠️与阿司匹林/华法林同服增加出血风险" },
                            { "col_1": "**藏红花**", "col_2": "⭐⭐ 中等", "col_3": "30mg/日", "col_4": "⚠️孕妇禁用" },
                            { "col_1": "**维生素D**", "col_2": "⭐⭐ 缺乏者有效", "col_3": "1000-4000 IU/日", "col_4": "建议先检测水平再补充" },
                            { "col_1": "**L-甲基叶酸**", "col_2": "⭐⭐ 增效剂", "col_3": "7.5-15mg/日", "col_4": "配合抗抑郁药使用效果更佳" },
                            { "col_1": "**NAC**", "col_2": "⭐⭐ 辅助", "col_3": "2000mg/日", "col_4": "哮喘患者慎用" },
                            { "col_1": "**锌**", "col_2": "⭐ 初步", "col_3": "25-50mg/日", "col_4": "长期高剂量导致铜缺乏" },
                            { "col_1": "**5-HTP**", "col_2": "⭐ 有限", "col_3": "100-300mg/日", "col_4": "⚠️与抗抑郁药同服有血清素综合征风险" }
                        ]
                    }
                ]
            }
        });

        assert_eq!(card, expected);
    }

    #[test]
    fn build_feishu_interactive_payload_serializes_card_into_content_string() {
        let card = json!({
            "schema": "2.0",
            "body": {
                "elements": [
                    {
                        "tag": "markdown",
                        "content": "**bold**",
                        "text_align": "left"
                    }
                ]
            }
        });

        let payload = build_feishu_interactive_payload(&card);
        assert_eq!(payload["msg_type"], "interactive");
        assert!(payload.get("card").is_none());

        let content = payload["content"]
            .as_str()
            .expect("interactive content should be a serialized JSON string");
        let reparsed: Value =
            serde_json::from_str(content).expect("interactive content should parse back to JSON");
        assert_eq!(reparsed, card);
    }

    #[test]
    fn build_feishu_interactive_payload_matches_expected_reply_body_for_supplement_table() {
        let card = json!({
            "schema": "2.0",
            "body": {
                "elements": [
                    {
                        "tag": "table",
                        "element_id": "md_table_1",
                        "row_height": "low",
                        "page_size": 9,
                        "columns": [
                            { "name": "col_1", "display_name": "补剂", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_2", "display_name": "证据强度", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_3", "display_name": "推荐剂量", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_4", "display_name": "关键注意事项", "data_type": "lark_md", "width": "auto" }
                        ],
                        "rows": [
                            { "col_1": "**圣约翰草**", "col_2": "⭐⭐⭐ 最强", "col_3": "900mg/日 (分3次)", "col_4": "⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用" },
                            { "col_1": "**SAM-e**", "col_2": "⭐⭐⭐ 强", "col_3": "800-1600mg/日", "col_4": "⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险" },
                            { "col_1": "**EPA鱼油**", "col_2": "⭐⭐ 中等", "col_3": "EPA 1-2g/日", "col_4": "⚠️与阿司匹林/华法林同服增加出血风险" },
                            { "col_1": "**藏红花**", "col_2": "⭐⭐ 中等", "col_3": "30mg/日", "col_4": "⚠️孕妇禁用" },
                            { "col_1": "**维生素D**", "col_2": "⭐⭐ 缺乏者有效", "col_3": "1000-4000 IU/日", "col_4": "建议先检测水平再补充" },
                            { "col_1": "**L-甲基叶酸**", "col_2": "⭐⭐ 增效剂", "col_3": "7.5-15mg/日", "col_4": "配合抗抑郁药使用效果更佳" },
                            { "col_1": "**NAC**", "col_2": "⭐⭐ 辅助", "col_3": "2000mg/日", "col_4": "哮喘患者慎用" },
                            { "col_1": "**锌**", "col_2": "⭐ 初步", "col_3": "25-50mg/日", "col_4": "长期高剂量导致铜缺乏" },
                            { "col_1": "**5-HTP**", "col_2": "⭐ 有限", "col_3": "100-300mg/日", "col_4": "⚠️与抗抑郁药同服有血清素综合征风险" }
                        ]
                    }
                ]
            }
        });

        let expected_payload = json!({
            "msg_type": "interactive",
            "content": card.to_string()
        });

        let payload = build_feishu_interactive_payload(&card);
        assert_eq!(payload, expected_payload);
    }

    #[test]
    fn build_ask_user_question_card_renders_single_and_multi_select_fields() {
        let card = build_ask_user_question_card(&AskUserQuestionRequestEvent {
            request_id: "req-1".to_string(),
            conversation_id: Some(42),
            questions: vec![
                AskUserQuestionItem {
                    question: "选择一个模型".to_string(),
                    header: "模型".to_string(),
                    options: vec![
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "GPT-5.4".to_string(),
                            description: "推荐".to_string(),
                        },
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "Claude".to_string(),
                            description: "保守".to_string(),
                        },
                    ],
                    multi_select: false,
                },
                AskUserQuestionItem {
                    question: "选择输出格式".to_string(),
                    header: "格式".to_string(),
                    options: vec![
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "表格".to_string(),
                            description: "结构化".to_string(),
                        },
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "列表".to_string(),
                            description: "简洁".to_string(),
                        },
                    ],
                    multi_select: true,
                },
            ],
            metadata: None,
        });

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        let form =
            elements.iter().find(|element| element["tag"] == "form").expect("form should exist");
        let form_elements = form["elements"].as_array().expect("form elements should be array");
        assert!(form_elements.iter().any(|element| element["tag"] == "select_static"));
        assert!(form_elements.iter().any(|element| element["tag"] == "multi_select_static"));
        assert_eq!(form["name"], "ask_user_req-1");
        let submit_button = form_elements.last().expect("submit button should exist");
        assert_eq!(submit_button["tag"], "button");
        assert_eq!(submit_button["name"], "ask_user_submit");
        assert_eq!(submit_button["form_action_type"], "submit");
        assert_eq!(submit_button["behaviors"][0]["type"], "callback");
        assert_eq!(submit_button["behaviors"][0]["value"]["action"], "submit");
        assert_eq!(submit_button["behaviors"][0]["value"]["request_id"], "req-1");

        let cancel_button = elements
            .iter()
            .find(|element| element["name"] == "ask_user_cancel")
            .expect("cancel button should exist");
        assert_eq!(cancel_button["tag"], "button");
        assert_eq!(cancel_button["name"], "ask_user_cancel");
        assert_eq!(cancel_button["behaviors"][0]["type"], "callback");
        assert_eq!(cancel_button["behaviors"][0]["value"]["action"], "cancel");
        assert_eq!(cancel_button["behaviors"][0]["value"]["request_id"], "req-1");
    }

    #[test]
    fn parse_permission_reply_command_supports_operation_variants() {
        assert_eq!(
            parse_permission_reply_command("批准一次 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "allow",
            })
        );
        assert_eq!(
            parse_permission_reply_command("本任务批准 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "allow_for_conversation",
            })
        );
        assert_eq!(
            parse_permission_reply_command("助手允许 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "allow_for_assistant",
            })
        );
        assert_eq!(
            parse_permission_reply_command("拒绝 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "deny",
            })
        );
    }

    #[test]
    fn parse_permission_reply_command_supports_acp_variants() {
        assert_eq!(
            parse_permission_reply_command("批准 2 ACP-QWERTY"),
            Some(PermissionReplyCommand::AcpSelect {
                review_code: "ACP-QWERTY".to_string(),
                option_index: 2,
            })
        );
        assert_eq!(
            parse_permission_reply_command("取消 ACP-QWERTY"),
            Some(PermissionReplyCommand::AcpCancel { review_code: "ACP-QWERTY".to_string() })
        );
        assert_eq!(parse_permission_reply_command("批准 0 ACP-QWERTY"), None);
    }

    #[test]
    fn map_ask_user_form_values_to_answers_supports_single_and_multi_select() {
        let questions = vec![
            AskUserQuestionItem {
                question: "选择一个模型".to_string(),
                header: "模型".to_string(),
                options: vec![
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "GPT-5.4".to_string(),
                        description: "推荐".to_string(),
                    },
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "Claude".to_string(),
                        description: "保守".to_string(),
                    },
                ],
                multi_select: false,
            },
            AskUserQuestionItem {
                question: "选择输出格式".to_string(),
                header: "格式".to_string(),
                options: vec![
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "表格".to_string(),
                        description: "结构化".to_string(),
                    },
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "列表".to_string(),
                        description: "简洁".to_string(),
                    },
                ],
                multi_select: true,
            },
        ];
        let form_value = Map::from_iter([
            ("question_0".to_string(), Value::String("GPT-5.4".to_string())),
            (
                "question_1".to_string(),
                Value::Array(vec![
                    Value::String("表格".to_string()),
                    Value::String("列表".to_string()),
                ]),
            ),
        ]);

        let answers = map_ask_user_form_values_to_answers(&questions, &form_value)
            .expect("answers should map");
        assert_eq!(answers.get("选择一个模型"), Some(&"GPT-5.4".to_string()));
        assert_eq!(answers.get("选择输出格式"), Some(&"表格, 列表".to_string()));
    }

    #[test]
    fn feishu_card_action_callback_parses_inner_event_payload() {
        let raw_event = json!({
            "operator": {
                "open_id": "ou_test_user"
            },
            "context": {
                "open_message_id": "om_test_message"
            },
            "action": {
                "value": {
                    "request_id": "req-1",
                    "action": "submit"
                },
                "form_value": {
                    "question_0": "GPT-5.4"
                }
            }
        });

        let callback: FeishuCardActionCallback =
            serde_json::from_value(raw_event).expect("inner event payload should parse");

        assert_eq!(callback.event().operator.open_id, "ou_test_user");
        assert_eq!(
            callback
                .event()
                .context
                .as_ref()
                .and_then(|context| context.open_message_id.as_deref()),
            Some("om_test_message")
        );
        assert_eq!(
            callback
                .event()
                .action
                .value
                .as_ref()
                .and_then(|value| value.get("request_id"))
                .and_then(Value::as_str),
            Some("req-1")
        );
    }

    #[test]
    fn feishu_card_action_callback_parses_enveloped_payload() {
        let raw_event = json!({
            "event": {
                "operator": {
                    "open_id": "ou_test_user"
                },
                "context": {
                    "open_message_id": "om_test_message"
                },
                "action": {
                    "value": {
                        "request_id": "req-1",
                        "action": "submit"
                    },
                    "form_value": {
                        "question_0": "GPT-5.4"
                    }
                }
            }
        });

        let callback: FeishuCardActionCallback =
            serde_json::from_value(raw_event).expect("enveloped payload should parse");

        assert_eq!(callback.event().operator.open_id, "ou_test_user");
        assert_eq!(
            callback
                .event()
                .context
                .as_ref()
                .and_then(|context| context.open_message_id.as_deref()),
            Some("om_test_message")
        );
        assert_eq!(
            callback
                .event()
                .action
                .value
                .as_ref()
                .and_then(|value| value.get("request_id"))
                .and_then(Value::as_str),
            Some("req-1")
        );
    }

    #[test]
    fn find_latest_recoverable_ask_user_tool_call_prefers_pending_or_executing() {
        let base = crate::db::mcp_db::MCPToolCall {
            id: 1,
            conversation_id: 42,
            message_id: None,
            subtask_id: None,
            server_id: 1,
            server_name: "ui_interaction".to_string(),
            tool_name: "ask_user_question".to_string(),
            parameters: "{}".to_string(),
            status: "success".to_string(),
            result: None,
            error: None,
            created_time: "2026-03-18 00:00:00".to_string(),
            started_time: None,
            finished_time: None,
            llm_call_id: None,
            assistant_message_id: None,
        };
        let calls = vec![
            crate::db::mcp_db::MCPToolCall {
                id: 2,
                status: "executing".to_string(),
                ..base.clone()
            },
            crate::db::mcp_db::MCPToolCall {
                id: 3,
                tool_name: "preview_file".to_string(),
                status: "pending".to_string(),
                ..base.clone()
            },
        ];

        let tool_call =
            find_latest_recoverable_ask_user_tool_call(&calls).expect("tool call should exist");
        assert_eq!(tool_call.id, 2);
    }

    #[test]
    fn parse_bot_menu_click_event_extracts_open_id_and_event_key() {
        let raw_event = json!({
            "operator": {
                "operator_id": {
                    "open_id": "ou_test_user"
                }
            },
            "event_key": "feishu::conversation::new",
            "timestamp": 1669364458
        });

        let event = parse_bot_menu_click_event(&raw_event)
            .expect("menu event should parse")
            .expect("menu event should not be empty");

        assert_eq!(
            event,
            FeishuBotMenuClickEvent {
                operator_open_id: "ou_test_user".to_string(),
                event_key: "feishu::conversation::new".to_string(),
            }
        );
    }

    #[test]
    fn feishu_relay_waits_for_finished_assistant_messages() {
        let now = Utc::now();
        let streaming = crate::db::conversation_db::Message {
            id: 1,
            parent_id: None,
            conversation_id: 1,
            message_type: "response".to_string(),
            content: "半句输出".to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: now,
            start_time: Some(now),
            finish_time: None,
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: None,
            parent_group_id: None,
            tool_calls_json: None,
            first_token_time: None,
            ttft_ms: None,
        };
        let finished =
            crate::db::conversation_db::Message { finish_time: Some(now), ..streaming.clone() };
        let tool_result = crate::db::conversation_db::Message {
            message_type: "tool_result".to_string(),
            finish_time: None,
            ..streaming.clone()
        };

        assert!(!is_message_ready_for_feishu_relay(&streaming));
        assert!(is_message_ready_for_feishu_relay(&finished));
        assert!(is_message_ready_for_feishu_relay(&tool_result));
    }

    #[test]
    fn debug_resend_prefers_preview_tool_result_after_response() {
        let now = Utc::now();
        let response = crate::db::conversation_db::Message {
            id: 10,
            parent_id: None,
            conversation_id: 1,
            message_type: "response".to_string(),
            content: "好的。<!-- MCP_TOOL_CALL:{\"server_name\":\"UI交互工具\",\"tool_name\":\"preview_file\",\"parameters\":\"{\\\"files\\\":[{\\\"title\\\":\\\"华容道情节构思\\\"}]}\"} -->".to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: now,
            start_time: Some(now),
            finish_time: Some(now),
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: Some("group-1".to_string()),
            parent_group_id: None,
            tool_calls_json: None,
            first_token_time: None,
            ttft_ms: None,
        };
        let preview_tool_result = crate::db::conversation_db::Message {
            id: 11,
            message_type: "tool_result".to_string(),
            content: "Tool execution completed:\n\nTool Call ID: call_1\nTool: preview_file\nServer: UI交互工具\nParameters: {\"files\":[{\"title\":\"华容道情节构思\",\"type\":\"text\",\"content\":\"完整正文\"}]}\nResult:\n[{\"type\":\"json\",\"json\":{\"status\":\"preview_shown\"}}]".to_string(),
            ..response.clone()
        };
        let later_response = crate::db::conversation_db::Message {
            id: 12,
            content: "后续回复".to_string(),
            ..response.clone()
        };

        let selected =
            collect_feishu_debug_resend_messages(&response, &[response.clone(), preview_tool_result.clone(), later_response]);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, preview_tool_result.id);
    }
}
