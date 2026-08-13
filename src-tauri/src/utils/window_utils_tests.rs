use std::sync::mpsc;
use std::time::Duration;

use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::{Listener, WebviewUrl, WebviewWindowBuilder};

use crate::api::ai::events::ConversationEvent;

use super::window_utils::send_conversation_event_to_chat_windows;

/// 测试总管家窗口能够收到对话事件广播
///
/// 验证内容：
/// - 创建与生产环境同名的 `butler_experiment` WebviewWindow
/// - 订阅指定会话的事件频道
/// - 广播错误消息更新后，窗口收到完整事件负载
#[test]
fn test_conversation_event_reaches_butler_window() {
    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    let butler_window = WebviewWindowBuilder::new(
        &app,
        "butler_experiment",
        WebviewUrl::default(),
    )
    .build()
    .unwrap();
    let conversation_id = 732;
    let event_name = format!("conversation_event_{}", conversation_id);
    let (sender, receiver) = mpsc::channel();

    butler_window.listen(&event_name, move |event| {
        sender.send(event.payload().to_string()).unwrap();
    });

    send_conversation_event_to_chat_windows(
        app.handle(),
        conversation_id,
        ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::json!({
                "message_id": 9986,
                "message_type": "error",
                "content": "No available providers",
                "is_done": true,
            }),
        },
    );

    let payload = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("butler window should receive the conversation event");
    let event: ConversationEvent = serde_json::from_str(&payload).unwrap();

    assert_eq!(event.r#type, "message_update");
    assert_eq!(event.data["message_id"], 9986);
    assert_eq!(event.data["message_type"], "error");
    assert_eq!(event.data["content"], "No available providers");
}
