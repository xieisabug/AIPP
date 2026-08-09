use crate::api::plugin_api::{
    build_comfyui_attachment, extract_image_urls, normalize_comfyui_base_url, parse_comfyui_history,
    PluginHttpRequestSpec, PluginImagePollSpec,
    validate_comfyui_target_message, validate_comfyui_workflow,
};
use crate::db::conversation_db::{Message, MessageAttachmentRepository, Repository};
use chrono::Utc;
use rusqlite::Connection;

fn sample_message(message_type: &str) -> Message {
    Message {
        id: 7,
        parent_id: None,
        conversation_id: 3,
        message_type: message_type.to_string(),
        content: "reply".to_string(),
        llm_model_id: Some(1),
        llm_model_name: Some("model".to_string()),
        created_time: Utc::now(),
        start_time: None,
        finish_time: None,
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: None,
        parent_group_id: None,
        tool_calls_json: None,
        metadata_json: None,
        first_token_time: None,
        ttft_ms: None,
    }
}

/// 验证固定 workflow 只能接受存在且匹配的 57:27 文本节点。
#[test]
fn test_comfyui_workflow_requires_matching_prompt_node() {
    let valid = serde_json::json!({ "57:27": { "inputs": { "text": "landscape" } } });
    assert!(validate_comfyui_workflow(&valid, "landscape", "57:27", "text").is_ok());
    assert!(validate_comfyui_workflow(&valid, "portrait", "57:27", "text").is_err());
    assert!(validate_comfyui_workflow(&serde_json::json!({}), "landscape", "57:27", "text").is_err());
}

/// 验证提示词节点和参数名可以脱离默认值配置。
#[test]
fn test_comfyui_workflow_supports_configured_prompt_path() {
    let workflow = serde_json::json!({ "prompt-node": { "inputs": { "positive": "landscape" } } });
    assert!(validate_comfyui_workflow(&workflow, "landscape", "prompt-node", "positive").is_ok());
    assert!(validate_comfyui_workflow(&workflow, "landscape", "prompt-node", "text").is_err());
}

/// 验证通用图片任务执行器可以解析供应商常见的 JSON 字符串结果。
#[test]
fn test_image_task_extracts_stringified_result_urls() {
    let payload = serde_json::json!({
        "data": { "state": "success", "resultJson": "{\"resultUrls\":[\"https://cdn.example/result.jpg\"]}" }
    });
    let spec = PluginImagePollSpec {
        request: PluginHttpRequestSpec { method: "GET".to_string(), url: "https://example.test/status".to_string(), headers: Default::default(), query: Default::default(), body: None },
        task_id_path: "/data/taskId".to_string(), status_path: "/data/state".to_string(), success_values: vec!["success".to_string()], failure_values: vec!["failed".to_string()], result_path: "/data/resultJson".to_string(), result_urls_path: Some("/resultUrls".to_string()), parse_json_string: true, interval_ms: 1000, timeout_ms: 180000,
    };
    assert_eq!(extract_image_urls(&payload, &spec), vec!["https://cdn.example/result.jpg"]);
}

/// 验证 history 只在输出图片可用时完成，并报告无图片执行结果。
#[test]
fn test_comfyui_history_extracts_output_and_rejects_empty_completion() {
    let history = serde_json::json!({
        "prompt-1": {
            "status": { "completed": true, "status_str": "success" },
            "outputs": { "9": { "images": [{ "filename": "out.png", "subfolder": "", "type": "output" }] } }
        }
    });
    let images = parse_comfyui_history(&history, "prompt-1").unwrap().unwrap();
    assert_eq!(images[0].filename, "out.png");
    let empty = serde_json::json!({ "prompt-1": { "status": { "completed": true, "status_str": "success" }, "outputs": {} } });
    assert!(parse_comfyui_history(&empty, "prompt-1").is_err());
}

/// 验证网络地址拒绝凭据和非 HTTP 协议。
#[test]
fn test_comfyui_url_rejects_credentials_and_non_http_schemes() {
    assert!(normalize_comfyui_base_url("http://127.0.0.1:8188").is_ok());
    assert!(normalize_comfyui_base_url("http://user:secret@127.0.0.1:8188").is_err());
    assert!(normalize_comfyui_base_url("file:///tmp/comfyui").is_err());
}

/// 验证生成图片只能绑定到 assistant/response 消息。
#[test]
fn test_comfyui_target_message_must_be_assistant_reply() {
    assert!(validate_comfyui_target_message(&sample_message("response"), 3).is_ok());
    assert!(validate_comfyui_target_message(&sample_message("assistant"), 3).is_ok());
    assert!(validate_comfyui_target_message(&sample_message("user"), 3).is_err());
    assert!(validate_comfyui_target_message(&sample_message("response"), 4).is_err());
}

/// 验证 ComfyUI 图片以 data URL 持久化并绑定目标消息。
#[test]
fn test_comfyui_image_attachment_persists_as_image_data_url() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE message_attachment (id INTEGER PRIMARY KEY AUTOINCREMENT, message_id INTEGER NOT NULL, attachment_type INTEGER NOT NULL, attachment_url TEXT, attachment_content TEXT, attachment_hash TEXT, use_vector BOOLEAN NOT NULL DEFAULT 0, token_count INTEGER);").unwrap();
    let repo = MessageAttachmentRepository::new(conn);
    let created = repo.create(&build_comfyui_attachment(7, "../out.png", "image/png", vec![1, 2, 3])).unwrap();
    assert_eq!(created.message_id, 7);
    assert_eq!(created.attachment_url.as_deref(), Some("out.png"));
    assert_eq!(created.attachment_content.as_deref(), Some("data:image/png;base64,AQID"));
}
