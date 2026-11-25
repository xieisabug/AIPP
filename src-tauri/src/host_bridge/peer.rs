//! Phone-side TLS and credential handling, outside the WebView.
#[cfg(target_os = "android")]
use super::types::*;
#[cfg(target_os = "android")]
use {anyhow::{Context, Result}, serde_json::{json, Value}, std::time::Duration, tauri::{AppHandle, Manager}};

#[cfg(target_os = "android")]
struct AndroidBridge(tauri::plugin::PluginHandle<tauri::Wry>);

pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("host-bridge")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            app.manage(AndroidBridge(api.register_android_plugin("com.xieisabug.aipp","HostBridgePlugin")?));
            #[cfg(desktop)]
            {
                use tauri::Manager;
                app.manage(super::host::Host::default());
            }
            let _ = (app, api);
            Ok(())
        }).build()
}

#[cfg(target_os = "android")]
async fn native(app: &AppHandle, command: &'static str, args: Value) -> Result<Value> {
    let plugin = app.state::<AndroidBridge>().0.clone();
    tokio::task::spawn_blocking(move || plugin.run_mobile_plugin(command, args)).await?
        .with_context(|| format!("Android 设备互联 {command}"))
}

#[cfg(target_os = "android")]
async fn request(endpoint: &Endpoint, path: &str, payload: Option<Value>, token: Option<&str>) -> Result<Value> {
    endpoint.id()?;
    let client = pinned_client(&endpoint.certificate, (endpoint.address, PORT).into(),
        Duration::from_secs(if path == "/v1/pair" { 135 } else { 12 }))?;
    let url = format!("https://{DNS_NAME}:{PORT}{path}");
    let mut req = if let Some(body) = payload { client.post(url).json(&body) } else { client.get(url) };
    if let Some(token) = token { req = req.bearer_auth(token); }
    let response = req.send().await.with_context(|| format!("连接电脑 {} ({})", endpoint.name, endpoint.address))?;
    let status = response.status();
    // Bound memory even for an invalid QR that points to a hostile service.
    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(body.len() + chunk.len() <= 512 * 1024, "电脑返回的数据超过设备互联限制");
        body.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&body).with_context(|| format!("读取电脑响应 HTTP {status}"))?;
    anyhow::ensure!(status.is_success(), "HTTP {status}：{}", value["error"].as_str().unwrap_or("电脑未提供错误说明"));
    Ok(value)
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn bridge_pair_phone(app: AppHandle, name: String) -> Result<Value,String> {
    async {
        valid_name(name.trim())?;
        let scan = native(&app,"scan",json!({})).await?;
        let Some(raw) = scan["code"].as_str() else { return Ok(Value::Null) };
        anyhow::ensure!(raw.len() <= 4096, "配对码过长");
        let invite: Invitation = serde_json::from_str(raw).context("请扫描 AIPP 设置页的设备互联二维码")?;
        anyhow::ensure!(invite.protocol_version == VERSION, "不兼容的设备互联协议版本 {}",invite.protocol_version);
        let id = invite.endpoint.id()?;
        let pair = PairRequest { protocol_version: VERSION, code: invite.code, name: name.trim().to_owned() };
        let reply = request(&invite.endpoint,"/v1/pair",Some(json!(pair)),None).await?;
        let session: Session = serde_json::from_value(reply).context("读取电脑签发的配对凭据")?;
        anyhow::ensure!(session.token.len() == 64 && session.token.bytes().all(|b| b.is_ascii_hexdigit()), "电脑签发的配对凭据无效");
        native(&app,"savePair",json!({"id":id,"endpoint":invite.endpoint,"token":session.token})).await?;
        Ok::<_,anyhow::Error>(json!({"id":id,"name":invite.endpoint.name}))
    }.await.map_err(|e| format!("手机配对：{e:#}"))
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn bridge_environments(app: AppHandle) -> Result<Value,String> {
    native(&app,"environments",json!({})).await.map_err(|e| format!("读取配对电脑：{e:#}"))
}

#[cfg(target_os = "android")]
async fn credential(app: &AppHandle, id: String) -> Result<Value> {
    use jni::objects::{JString, JValue};
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.get_webview_window("chat_ui").context("手机主窗口不存在")?.with_webview(move |platform| {
        platform.jni_handle().exec(move |env, activity, _| {
            let result = (|| -> anyhow::Result<String> {
                let key = env.new_string(id)?;
                let raw = env.call_method(activity, "hostBridgeCredential", "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&key)])?.l()?;
                let raw = JString::from(raw);
                let text: String = env.get_string(&raw)?.into();
                Ok(text)
            })();
            if result.is_err() { let _ = env.exception_clear(); }
            let _ = tx.send(result);
        });
    })?;
    let raw = tokio::time::timeout(Duration::from_secs(10), rx).await.context("访问 Android Keystore 超时")???;
    let pair: Value = serde_json::from_str(&raw)?;
    if let Some(error) = pair["error"].as_str() { anyhow::bail!("{error}") }
    Ok(pair)
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn bridge_connect(app: AppHandle, id: String, conversation: Option<i64>, before: Option<i64>, action: Option<ChatAction>, watch: Option<bool>) -> Result<Value,String> {
    async {
        let peer = credential(&app, id.clone()).await?;
        let endpoint: Endpoint = serde_json::from_value(peer["endpoint"].clone())?;
        anyhow::ensure!(endpoint.id()? == id, "配对身份不一致");
        let token = peer["token"].as_str().context("设备令牌丢失")?;
        if let Some(action) = action {
            return request(&endpoint, "/v1/chat", Some(json!(action)), Some(token)).await;
        }
        let path = match conversation {
            Some(id) if watch.unwrap_or(false) => format!("/v1/watch/{id}"),
            Some(id) => format!("/v1/history/{id}/{}", before.unwrap_or(i64::MAX)),
            None => "/v1/environment".to_string(),
        };
        request(&endpoint, &path, None, Some(token)).await
    }.await.map_err(|e| format!("读取电脑运行环境：{e:#}"))
}
