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

pub struct OAuthHttp {
    client: reqwest::Client,
    #[cfg(test)]
    loopback: bool,
}

impl OAuthHttp {
    pub fn new() -> Result<Self, String> {
        Ok(Self { client: http()?, #[cfg(test)] loopback: false })
    }
    pub(super) fn endpoint(&self, value: &str) -> Result<reqwest::Url, String> {
        #[cfg(test)]
        if self.loopback {
            let url = reqwest::Url::parse(value).map_err(|e| e.to_string())?;
            if url.scheme() == "http" && url.host_str() == Some("127.0.0.1") { return Ok(url); }
        }
        public_https(value)
    }
    #[cfg(test)]
    pub(super) fn loopback() -> Self {
        Self { client: reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(3)).build().unwrap(), loopback: true }
    }
}

pub fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(30))
        .build().map_err(|_| "无法建立授权 HTTP 客户端".into())
}

pub async fn json(response: reqwest::Response, stage: &str) -> Result<serde_json::Value, String> {
    let status = response.status();
    if response.content_length().unwrap_or(0) > 1024 * 1024 { return Err(format!("{stage}响应过大")); }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| format!("{stage}响应读取失败"))? {
        if bytes.len() + chunk.len() > 1024 * 1024 { return Err(format!("{stage}响应超过上限")); }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let value = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_default();
        let code = match value["error"].as_str() {
            Some("invalid_grant") => "invalid_grant：授权已失效，需要重新登录",
            Some("invalid_client") => "invalid_client：客户端注册无效，需要重新连接",
            Some("access_denied") => "access_denied：服务商拒绝授权",
            Some("invalid_scope") => "invalid_scope：服务不接受请求的权限范围",
            Some("unsupported_token_type") => "unsupported_token_type：服务不支持撤销该令牌类型",
            _ => "服务未提供可安全展示的错误代码",
        };
        return Err(format!("{stage}失败：HTTP {status}；{code}"));
    }
    serde_json::from_slice(&bytes).map_err(|_| format!("{stage}返回无效 JSON"))
}

pub(super) async fn exchange_with(http: &OAuthHttp, endpoint: &str, form: &[(&str, &str)]) -> Result<Token, String> {
    exchange_authenticated(http,endpoint,form,"none","",None).await
}

pub(super) async fn exchange_authenticated(http: &OAuthHttp, endpoint: &str, form: &[(&str,&str)], method: &str, client_id: &str, secret: Option<&str>) -> Result<Token,String> {
    http.endpoint(endpoint)?;
    let mut fields=form.to_vec();
    if method == "client_secret_post" { fields.push(("client_secret",secret.ok_or("OAuth 缺少 client secret")?)); }
    let body = fields.iter().map(|(key, value)| format!("{}={}", urlencoding::encode(key), urlencoding::encode(value))).collect::<Vec<_>>().join("&");
    let mut request = http.client.post(endpoint).header("Content-Type", "application/x-www-form-urlencoded").body(body);
    if method == "client_secret_basic" { request=request.basic_auth(urlencoding::encode(client_id),Some(urlencoding::encode(secret.ok_or("OAuth 缺少 client secret")?))); }
    let response = request.send().await.map_err(|e| format!("OAuth 令牌请求失败：{}", e.without_url()))?;
    let token: Token = serde_json::from_value(json(response, "OAuth 令牌交换/刷新").await?).map_err(|_| "OAuth 令牌响应字段缺失")?;
    if token.access_token.is_empty() || !token.token_type.eq_ignore_ascii_case("bearer") || token.expires_in.is_some_and(|v| v > 315360000) {
        return Err("OAuth 返回了不支持的令牌类型或有效期".into());
    }
    Ok(token)
}

pub struct PendingLogin {
    pub client_secret: Option<String>,
    pub client_auth_method: String,
    pub url: String,
    pub listener: tokio::net::TcpListener,
    pub state: String,
    pub verifier: String,
    pub redirect: String,
    pub client_id: String,
    pub token_endpoint: String,
    pub endpoint: String,
    pub resource: String,
    pub revocation_endpoint: Option<String>,
}

fn random() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn callback(raw: &str, expected_state: &str) -> Result<String, String> { callback_path(raw,expected_state,"/oauth/callback") }
fn callback_path(raw: &str, expected_state: &str, expected_path: &str) -> Result<String,String> {
    let path = raw.lines().next().and_then(|line| line.strip_prefix("GET ")).and_then(|line| line.split_once(' ')).map(|v| v.0).ok_or("OAuth 回调不是 GET 请求")?;
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{path}")).map_err(|_| "OAuth 回调地址无效")?;
    if url.path() != expected_path { return Err("OAuth 回调路径不匹配".into()); }
    let pairs: Vec<_> = url.query_pairs().collect();
    if pairs.iter().filter(|(k, _)| k == "state").count() != 1 || !pairs.iter().any(|(k, v)| k == "state" && v == expected_state) {
        return Err("OAuth 回调 state 不匹配，已拒绝".into());
    }
    if pairs.iter().any(|(k, _)| k == "error") { return Err("服务商拒绝了授权或用户取消登录".into()); }
    let codes: Vec<_> = pairs.iter().filter(|(k, _)| k == "code").collect();
    if codes.len() != 1 || codes[0].1.is_empty() { return Err("OAuth 回调缺少唯一授权码".into()); }
    Ok(codes[0].1.to_string())
}

#[cfg(test)]
pub(super) async fn begin_with(http: &OAuthHttp, endpoint: &str, client_id: Option<&str>, scopes: &[String]) -> Result<PendingLogin, String> {
    begin_with_resource(http, endpoint, client_id, scopes, None).await
}

pub async fn begin_resource(endpoint: &str, client_id: Option<&str>, scopes: &[String], resource: Option<&str>) -> Result<PendingLogin, String> {
    begin_with_resource(&OAuthHttp::new()?, endpoint, client_id, scopes, resource).await
}

pub(super) async fn begin_with_resource(http: &OAuthHttp, endpoint: &str, client_id: Option<&str>, scopes: &[String], expected_resource: Option<&str>) -> Result<PendingLogin, String> {
    begin_options(http,endpoint,client_id,scopes,expected_resource,"none",None,None).await
}

pub fn redirect_target(uri: &str) -> Result<reqwest::Url,String> {
    let url=reqwest::Url::parse(uri).map_err(|_|"OAuth 回调地址无效")?;
    if url.scheme()!="http" || ![Some("127.0.0.1"),Some("localhost")].contains(&url.host_str()) || url.port().is_none() || url.port()==Some(0) || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {return Err("OAuth 回调必须是带固定端口的 http://127.0.0.1 或 localhost 地址".into());}
    Ok(url)
}

pub(super) async fn begin_options(http: &OAuthHttp, endpoint: &str, client_id: Option<&str>, scopes: &[String], expected_resource: Option<&str>, method: &str, secret: Option<&str>, redirect_uri: Option<&str>) -> Result<PendingLogin,String> {
    if !["none","client_secret_post","client_secret_basic"].contains(&method) {return Err("不支持的 OAuth 客户端认证方式".into());}
    if method!="none" && (secret.is_none_or(|s|s.is_empty()) || client_id.is_none_or(|s|s.is_empty())) {return Err("机密 OAuth 客户端必须填写自己的 client ID 与 client secret".into());}
    let explicit_resource = expected_resource.is_some();
    let expected_resource = expected_resource.unwrap_or(endpoint);
    if http.endpoint(expected_resource)?.origin() != http.endpoint(endpoint)?.origin() { return Err("OAuth resource 必须与 MCP 服务同源".into()); }
    let endpoint_url = http.endpoint(endpoint)?;
    let client = &http.client;
    let probe = client.get(endpoint).send().await.map_err(|e| format!("OAuth 发现请求失败：{}", e.without_url()))?;
    let challenge = probe.headers().get(reqwest::header::WWW_AUTHENTICATE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let metadata_url = if explicit_resource {
        let resource_url = http.endpoint(expected_resource)?;
        format!("{}/.well-known/oauth-protected-resource{}", resource_url.origin().ascii_serialization(), resource_url.path().trim_end_matches('/'))
    } else {
        challenge.split("resource_metadata=\"").nth(1).and_then(|v| v.split('"').next()).map(str::to_owned)
            .unwrap_or_else(|| format!("{}/.well-known/oauth-protected-resource{}", endpoint_url.origin().ascii_serialization(), endpoint_url.path().trim_end_matches('/')))
    };
    let requested_scopes = if scopes.is_empty() {
        challenge.split("scope=\"").nth(1).and_then(|v| v.split('"').next()).unwrap_or("").to_string()
    } else { scopes.join(" ") };
    http.endpoint(&metadata_url)?;
    let resource = json(client.get(&metadata_url).send().await.map_err(|_| "OAuth 资源元数据请求失败")?, "OAuth 资源元数据").await?;
    if resource["resource"].as_str() != Some(expected_resource) { return Err("OAuth 资源元数据与声明的 resource 不匹配".into()); }
    let issuer = resource["authorization_servers"].as_array().and_then(|a| a.first()).and_then(|s| s.as_str()).ok_or("OAuth 资源未声明授权服务器")?;
    let issuer_url = http.endpoint(issuer)?;
    let discovery = format!("{}/.well-known/oauth-authorization-server{}", issuer_url.origin().ascii_serialization(), issuer_url.path().trim_end_matches('/'));
    let metadata = json(client.get(discovery).send().await.map_err(|_| "OAuth 授权服务器元数据请求失败")?, "OAuth 授权服务器元数据").await?;
    if metadata["issuer"].as_str() != Some(issuer) { return Err("OAuth 授权服务器 issuer 不匹配".into()); }
    if metadata["code_challenge_methods_supported"].as_array().is_some_and(|methods| !methods.iter().any(|m| m == "S256")) {
        return Err("服务不支持 S256 PKCE，无法安全连接".into());
    }
    if metadata["token_endpoint_auth_methods_supported"].as_array().is_some_and(|methods| !methods.iter().any(|m| m == method)) {
        return Err(format!("服务不支持所选 OAuth 客户端认证方式 {method}"));
    }
    let auth = metadata["authorization_endpoint"].as_str().ok_or("OAuth 缺少授权地址")?;
    let token_endpoint = metadata["token_endpoint"].as_str().ok_or("OAuth 缺少令牌地址")?.to_owned();
    let mut auth_url = http.endpoint(auth)?;
    http.endpoint(&token_endpoint)?;
    let revocation_endpoint = metadata["revocation_endpoint"].as_str().map(str::to_owned);
    if let Some(endpoint) = &revocation_endpoint { http.endpoint(endpoint)?; }
    let redirect_url=redirect_uri.map(redirect_target).transpose()?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1",redirect_url.as_ref().and_then(|u|u.port()).unwrap_or(0))).await.map_err(|e| format!("OAuth 回调监听失败：{e}"))?;
    let redirect = redirect_uri.map(str::to_string).unwrap_or(format!("http://127.0.0.1:{}/oauth/callback", listener.local_addr().map_err(|e| e.to_string())?.port()));
    let client_id = if let Some(id) = client_id.filter(|v| !v.trim().is_empty()) { id.to_owned() } else {
        let registration = metadata["registration_endpoint"].as_str().ok_or("服务不支持动态注册，请在应用目录配置已注册的 OAuth client_id")?;
        http.endpoint(registration)?;
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
        ("resource", expected_resource), ("scope", &requested_scopes),
    ]);
    Ok(PendingLogin { client_secret:secret.map(str::to_string),client_auth_method:method.into(), url: auth_url.to_string(), listener, state, verifier, redirect, client_id, token_endpoint, endpoint: endpoint.to_owned(), resource: expected_resource.into(), revocation_endpoint })
}

pub async fn finish(pending: PendingLogin) -> Result<Credential, String> {
    finish_with(&OAuthHttp::new()?, pending).await
}

pub(super) async fn finish_with(http: &OAuthHttp, pending: PendingLogin) -> Result<Credential, String> {
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
        let redirect_url=reqwest::Url::parse(&pending.redirect).map_err(|_|"OAuth 回调地址损坏")?;
        let result = callback_path(std::str::from_utf8(&bytes).map_err(|_| "OAuth 回调编码无效")?, &pending.state,redirect_url.path());
        let body = if result.is_ok() { "Authorization received. Return to AIPP to finish connecting." } else { "Authorization rejected. Return to AIPP for details." };
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let _ = socket.write_all(response.as_bytes()).await;
        result
    }).await.map_err(|_| "OAuth 登录等待超时，请重新连接")??;
    let token = exchange_authenticated(http, &pending.token_endpoint, &[
        ("grant_type", "authorization_code"), ("code", &code), ("client_id", &pending.client_id),
        ("redirect_uri", &pending.redirect), ("code_verifier", &pending.verifier), ("resource", &pending.resource),
    ],&pending.client_auth_method,&pending.client_id,pending.client_secret.as_deref()).await?;
    Ok(Credential { server_id: 0, endpoint: pending.endpoint, headers: HashMap::new(), oauth: Some(OAuthCredential {
        client_secret:pending.client_secret,client_auth_method:Some(pending.client_auth_method),
        client_id: pending.client_id, token_endpoint: pending.token_endpoint,
        access_token: token.access_token, refresh_token: token.refresh_token,
        expires_at: token.expires_in.map(|v| chrono::Utc::now().timestamp() + v as i64),
        revocation_endpoint: pending.revocation_endpoint,
        resource: Some(pending.resource),
    }) })
}

pub async fn revoke(credential: &Credential) -> Result<bool, String> {
    revoke_with(&OAuthHttp::new()?, credential).await
}

pub(super) async fn revoke_with(http: &OAuthHttp, credential: &Credential) -> Result<bool, String> {
    let Some(oauth) = credential.oauth.as_ref() else { return Ok(false); };
    let Some(endpoint) = oauth.revocation_endpoint.as_deref() else { return Ok(false); };
    http.endpoint(endpoint)?;
    let mut tokens = Vec::new();
    if let Some(refresh) = oauth.refresh_token.as_deref() { tokens.push((refresh, "refresh_token")); }
    tokens.push((oauth.access_token.as_str(), "access_token"));
    for (token, hint) in tokens {
        let mut body = format!("token={}&token_type_hint={hint}&client_id={}", urlencoding::encode(token), urlencoding::encode(&oauth.client_id));
        let method=oauth.client_auth_method.as_deref().unwrap_or("none");
        if method=="client_secret_post" {body.push_str(&format!("&client_secret={}",urlencoding::encode(oauth.client_secret.as_deref().ok_or("OAuth 缺少 client secret")?)));}
        let mut request=http.client.post(endpoint).header("Content-Type", "application/x-www-form-urlencoded");
        if method=="client_secret_basic" {request=request.basic_auth(urlencoding::encode(&oauth.client_id),Some(urlencoding::encode(oauth.client_secret.as_deref().ok_or("OAuth 缺少 client secret")?)));}
        let response = request.body(body).send().await
            .map_err(|e| format!("服务商授权撤销请求失败：{}", e.without_url()))?;
        if !response.status().is_success() { json(response, "服务商授权撤销").await?; }
    }
    Ok(true)
}

#[cfg(test)]
#[path = "oauth_tests.rs"]
mod oauth_tests;
