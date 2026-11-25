//! Desktop-only controls. Pairing decisions belong to the desktop settings window.
#[cfg(desktop)]
use super::{host::{self, Host}, store, types::*};
#[cfg(desktop)]
use {serde_json::{json, Value}, tauri::{AppHandle, Manager, WebviewWindow}, uuid::Uuid};

#[cfg(desktop)]
fn settings(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "config" { return Err("请在桌面设置里管理设备互联".into()); }
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
pub async fn bridge_host_status(app: AppHandle, window: WebviewWindow) -> Result<Value, String> {
    settings(&window)?;
    let addresses = host::addresses().map_err(|e| format!("枚举局域网地址：{e}"))?;
    let devices = store::open(&app).and_then(|conn| store::devices(&conn)).map_err(|e| format!("读取已配对设备：{e}"))?;
    let state = app.state::<Host>();
    let guard = state.0.lock().await;
    Ok(json!({"addresses":addresses, "devices":devices,
        "listening":guard.as_ref().map(|h| json!({"name":h.endpoint.name,"address":h.endpoint.address})),
        "pending":guard.as_ref().and_then(|h| h.pending.as_ref()).filter(|p| !p.reply.is_closed()).map(|p| json!({"id":p.id,"name":p.name}))}))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn bridge_start(app: AppHandle, window: WebviewWindow, name: String, address: std::net::Ipv4Addr) -> Result<(),String> {
    settings(&window)?;
    host::start(&app, name.trim().to_string(), address).await.map_err(|e| format!("启动设备互联：{e:#}"))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn bridge_stop(app: AppHandle, window: WebviewWindow) -> Result<(),String> {
    settings(&window)?;
    let state = app.state::<Host>();
    if let Some(running) = state.0.lock().await.take() { running.stop.cancel(); }
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
pub async fn bridge_invite(app: AppHandle, window: WebviewWindow) -> Result<String,String> {
    settings(&window)?;
    let state = app.state::<Host>();
    let mut host = state.0.lock().await;
    host.as_mut().ok_or("设备互联已关闭")?.invite().map_err(|e| format!("创建配对邀请：{e}"))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn bridge_decide(app: AppHandle, window: WebviewWindow, id: Uuid, grant: Option<Access>) -> Result<(),String> {
    settings(&window)?;
    let state = app.state::<Host>();
    let mut host = state.0.lock().await;
    host.as_mut().ok_or("设备互联已关闭")?.decide(&app,id,grant).map_err(|e| format!("确认配对：{e}"))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn bridge_revoke(app: AppHandle, window: WebviewWindow, id: Uuid) -> Result<(),String> {
    settings(&window)?;
    store::open(&app).and_then(|conn| store::revoke(&conn,id)).map_err(|e| format!("撤销设备：{e}"))
}

#[cfg(desktop)]
#[tauri::command]
pub fn bridge_set_chat_permission(app: AppHandle, window: WebviewWindow, id: Uuid, allowed: bool) -> Result<(),String> {
    settings(&window)?;
    let db = store::open(&app).map_err(|e| format!("打开设备授权库：{e}"))?;
    let count = db.execute("UPDATE host_bridge_peer SET access=json_set(access,'$.chat',json(?1)) WHERE id=?2",
        rusqlite::params![if allowed { "true" } else { "false" }, id.to_string()]).map_err(|e| format!("更新手机对话权限：{e}"))?;
    if count == 0 { return Err("设备配对已撤销".into()); }
    Ok(())
}
