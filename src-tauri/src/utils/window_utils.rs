use crate::api::ai::events::{
    ConversationEvent, ErrorNotificationPayload, ERROR_NOTIFICATION_EVENT,
};
use tauri::{Emitter, Manager, Window};

/// 检查chat和ask窗口是否有任何一个聚焦
/// 如果有任何一个窗口聚焦，返回true；否则返回false
pub fn is_chat_or_ask_window_focused(app_handle: &tauri::AppHandle) -> bool {
    // 检查 ask 窗口是否聚焦
    if let Some(ask_window) = app_handle.get_webview_window("ask") {
        if let Ok(is_visible) = ask_window.is_visible() {
            if is_visible {
                if let Ok(is_focused) = ask_window.is_focused() {
                    if is_focused {
                        return true;
                    }
                }
            }
        }
    }

    // 检查 chat_ui 窗口是否聚焦
    if let Some(chat_ui_window) = app_handle.get_webview_window("chat_ui") {
        if let Ok(is_visible) = chat_ui_window.is_visible() {
            if is_visible {
                if let Ok(is_focused) = chat_ui_window.is_focused() {
                    if is_focused {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// 智能发送错误到合适的窗口
/// 如果 ChatUI 窗口存在（不管是否可见），只发送给 ChatUI（不发送给 Ask 窗口）
/// 否则发送给 Ask 窗口，并附带 conversation_id 以便前端过滤
pub fn send_error_to_appropriate_window(
    window: &Window,
    error_message: &str,
    conversation_id: Option<i64>,
) {
    let payload =
        ErrorNotificationPayload { conversation_id, error_message: error_message.to_string() };

    // 获取 ChatUI 窗口
    if let Some(chat_ui_window) = window.app_handle().get_webview_window("chat_ui") {
        // ChatUI 窗口存在，只发送错误给 ChatUI，不发送给 Ask 窗口
        // 不再检查 is_visible，因为用户说只要 chat 窗口打开了就不在 ask 显示
        let _ = chat_ui_window.emit(ERROR_NOTIFICATION_EVENT, &payload);
        return;
    }

    // ChatUI 窗口不存在，发送给 Ask 窗口
    if let Some(ask_window) = window.app_handle().get_webview_window("ask") {
        let _ = ask_window.emit(ERROR_NOTIFICATION_EVENT, &payload);
    } else {
        // 回退到全局广播（保险起见）
        let _ = window.emit(ERROR_NOTIFICATION_EVENT, &payload);
    }
}

/// 向对话相关窗口发送对话事件
/// 同时向 ask 和 chat_ui 窗口发送对话事件，确保所有相关界面都能收到通知
pub fn send_conversation_event_to_chat_windows(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    event: ConversationEvent,
) {
    let event_name = format!("conversation_event_{}", conversation_id);

    // 发送给 Ask 窗口
    if let Some(ask_window) = app_handle.get_webview_window("ask") {
        let _ = ask_window.emit(&event_name, &event);
    }

    // 发送给 Chat UI 窗口
    if let Some(chat_ui_window) = app_handle.get_webview_window("chat_ui") {
        let _ = chat_ui_window.emit(&event_name, &event);
    }
}
