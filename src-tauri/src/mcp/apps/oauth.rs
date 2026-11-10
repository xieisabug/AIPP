use super::catalog::public_https;
use super::credentials::{Credential, OAuthCredential};
use base64::Engine;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Deserialize)]
pub struct Token {
    pub access_token: String,
    pub token_type: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

pub fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(30))
        .build().map_err(|_| "无法建立授权 HTTP 客户端".into())
}

pub async fn json(response: reqwest::Response, stage: &str) -> Result<serde_json::Value, String> {
    let status = response.status();
    if !status.is_success() { return Err(format!("{stage}失败：HTTP {status}（服务响应正文可能包含凭据，未记录）")); }
    if response.content_length().unwrap_or(0) > 1024 * 1024 { return Err(format!("{stage}响应过大")); }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| format!("{stage}响应读取失败"))? {
        if bytes.len() + chunk.len() > 1024 * 1024 { return Err(format!("{stage}响应超过上限")); }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| format!("{stage}返回无效 JSON"))
}

pub async fn exchange(endpoint: &str, form: &[(&str, &str)]) -> Result<Token, String> {
    public_https(endpoint)?;
    let body = form.iter().map(|(key, value)| format!("{}={}", urlencoding::encode(key), urlencoding::encode(value))).collect::<Vec<_>>().join("&");
    let response = http()?.post(endpoint).header("Content-Type", "application/x-www-form-urlencoded").body(body).send().await.map_err(|e| format!("OAuth 令牌请求失败：{}", e.without_url()))?;
    let token: Token = serde_json::from_value(json(response, "OAuth 令牌交换/刷新").await?).map_err(|_| "OAuth 令牌响应字段缺失")?;
    if token.access_token.is_empty() || !token.token_type.eq_ignore_ascii_case("bearer") || token.expires_in.is_some_and(|v| v > 315360000) {
        return Err("OAuth 返回了不支持的令牌类型或有效期".into());
    }
    Ok(token)
}

pub struct PendingLogin {
    pub url: String,
    pub listener: tokio::net::TcpListener,
    pub state: String,
    pub verifier: String,
    pub redirect: String,
    pub client_id: String,
    pub token_endpoint: String,
    pub endpoint: String,
}

fn random() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn callback(raw: &str, expected_state: &str) -> Result<String, String> {
    let path = raw.lines().next().and_then(|line| line.strip_prefix("GET ")).and_then(|line| line.split_once(' ')).map(|v| v.0).ok_or("OAuth 回调不是 GET 请求")?;
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{path}")).map_err(|_| "OAuth 回调地址无效")?;
    if url.path() != "/oauth/callback" { return Err("OAuth 回调路径不匹配".into()); }
    let pairs: Vec<_> = url.query_pairs().collect();
    if pairs.iter().filter(|(k, _)| k == "state").count() != 1 || !pairs.iter().any(|(k, v)| k == "state" && v == expected_state) {
        return Err("OAuth 回调 state 不匹配，已拒绝".into());
    }
    if pairs.iter().any(|(k, _)| k == "error") { return Err("服务商拒绝了授权或用户取消登录".into()); }
    let codes: Vec<_> = pairs.iter().filter(|(k, _)| k == "code").collect();
    if codes.len() != 1 || codes[0].1.is_empty() { return Err("OAuth 回调缺少唯一授权码".into()); }
    Ok(codes[0].1.to_string())
}

pub async fn begin(endpoint: &str, client_id: Option<&str>, scopes: &[String]) -> Result<PendingLogin, String> {
    let endpoint_url = public_https(endpoint)?;
    let client = http()?;
    let probe = client.get(endpoint).send().await.map_err(|e| format!("OAuth 发现请求失败：{}", e.without_url()))?;
    let challenge = probe.headers().get(reqwest::header::WWW_AUTHENTICATE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let metadata_url = challenge.split("resource_metadata=\"").nth(1).and_then(|v| v.split('"').next()).map(str::to_owned)
        .unwrap_or_else(|| format!("{}/.well-known/oauth-protected-resource{}", endpoint_url.origin().ascii_serialization(), endpoint_url.path().trim_end_matches('/')));
    public_https(&metadata_url)?;
    let resource = json(client.get(&metadata_url).send().await.map_err(|_| "OAuth 资源元数据请求失败")?, "OAuth 资源元数据").await?;
    if resource["resource"].as_str() != Some(endpoint) { return Err("OAuth 资源元数据与应用服务地址不匹配".into()); }
    let issuer = resource["authorization_servers"].as_array().and_then(|a| a.first()).and_then(|s| s.as_str()).ok_or("OAuth 资源未声明授权服务器")?;
    let issuer_url = public_https(issuer)?;
    let discovery = format!("{}/.well-known/oauth-authorization-server{}", issuer_url.origin().ascii_serialization(), issuer_url.path().trim_end_matches('/'));
    let metadata = json(client.get(discovery).send().await.map_err(|_| "OAuth 授权服务器元数据请求失败")?, "OAuth 授权服务器元数据").await?;
    if metadata["issuer"].as_str() != Some(issuer) { return Err("OAuth 授权服务器 issuer 不匹配".into()); }
    let auth = metadata["authorization_endpoint"].as_str().ok_or("OAuth 缺少授权地址")?;
    let token_endpoint = metadata["token_endpoint"].as_str().ok_or("OAuth 缺少令牌地址")?.to_owned();
    let mut auth_url = public_https(auth)?;
    public_https(&token_endpoint)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|e| format!("OAuth 回调监听失败：{e}"))?;
    let redirect = format!("http://127.0.0.1:{}/oauth/callback", listener.local_addr().map_err(|e| e.to_string())?.port());
    let client_id = if let Some(id) = client_id.filter(|v| !v.trim().is_empty()) { id.to_owned() } else {
        let registration = metadata["registration_endpoint"].as_str().ok_or("服务不支持动态注册，请在应用目录配置已注册的 OAuth client_id")?;
        public_https(registration)?;
        let registered = json(client.post(registration).json(&serde_json::json!({
            "client_name":"AIPP", "redirect_uris":[redirect], "token_endpoint_auth_method":"none",
            "grant_types":["authorization_code","refresh_token"], "response_types":["code"]
        })).send().await.map_err(|_| "OAuth 客户端注册请求失败")?, "OAuth 客户端注册").await?;
        if registered["token_endpoint_auth_method"].as_str().is_some_and(|m| m != "none") { return Err("当前连接器仅支持 PKCE 公共客户端，服务要求私密客户端认证".into()); }
        registered["client_id"].as_str().ok_or("OAuth 注册响应缺少 client_id")?.to_owned()
    };
    let state = random();
    let verifier = random();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    auth_url.query_pairs_mut().extend_pairs([
        ("response_type", "code"), ("client_id", &client_id), ("redirect_uri", &redirect),
        ("state", &state), ("code_challenge", &challenge), ("code_challenge_method", "S256"),
        ("resource", endpoint), ("scope", &scopes.join(" ")),
    ]);
    Ok(PendingLogin { url: auth_url.to_string(), listener, state, verifier, redirect, client_id, token_endpoint, endpoint: endpoint.to_owned() })
}

pub async fn finish(pending: PendingLogin) -> Result<Credential, String> {
    let code = tokio::time::timeout(Duration::from_secs(300), async {
        let (mut socket, _) = pending.listener.accept().await.map_err(|e| format!("OAuth 回调接收失败：{e}"))?;
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0u8; 1024];
            let count = socket.read(&mut chunk).await.map_err(|_| "OAuth 回调读取失败")?;
            if count == 0 || bytes.len() + count > 16384 { return Err("OAuth 回调不完整或过大".to_string()); }
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.windows(4).any(|v| v == b"\r\n\r\n") { break; }
        }
        let result = callback(std::str::from_utf8(&bytes).map_err(|_| "OAuth 回调编码无效")?, &pending.state);
        let body = if result.is_ok() { "Authorization received. Return to AIPP to finish connecting." } else { "Authorization rejected. Return to AIPP for details." };
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let _ = socket.write_all(response.as_bytes()).await;
        result
    }).await.map_err(|_| "OAuth 登录等待超时，请重新连接")??;
    let token = exchange(&pending.token_endpoint, &[
        ("grant_type", "authorization_code"), ("code", &code), ("client_id", &pending.client_id),
        ("redirect_uri", &pending.redirect), ("code_verifier", &pending.verifier), ("resource", &pending.endpoint),
    ]).await?;
    Ok(Credential { server_id: 0, endpoint: pending.endpoint, headers: HashMap::new(), oauth: Some(OAuthCredential {
        client_id: pending.client_id, token_endpoint: pending.token_endpoint,
        access_token: token.access_token, refresh_token: token.refresh_token,
        expires_at: token.expires_in.map(|v| chrono::Utc::now().timestamp() + v as i64),
    }) })
}
