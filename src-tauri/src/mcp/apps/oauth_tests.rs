use super::*;
use crate::mcp::apps::credentials::{self, CredentialStore};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State { registrations: usize, refreshes: HashMap<String, usize>, revoked: Vec<String>, client_auth: Vec<(HashMap<String,String>,String)>,
    /// Requests seen before any delay, so a test can cancel while the provider is still working.
    refresh_requests: usize, refresh_delay: Duration, strict_rotation: bool }

struct Provider { base: String, state: Arc<Mutex<State>>, task: tokio::task::JoinHandle<()> }
impl Drop for Provider { fn drop(&mut self) { self.task.abort(); } }

async fn provider() -> Provider {
    provider_with_resource(false).await
}

async fn provider_with_resource(origin_resource: bool) -> Provider {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(State::default()));
    let address = base.clone();
    let shared = state.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let base = address.clone();
            let shared = shared.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let (header_end, size) = loop {
                    let mut buffer = [0u8; 4096];
                    let n = socket.read(&mut buffer).await.unwrap();
                    if n == 0 { return; }
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let size = headers.lines().find_map(|line| line.split_once(':').filter(|(key,_)| key.eq_ignore_ascii_case("content-length")).map(|(_,value)| value.trim().parse::<usize>().unwrap())).unwrap_or(0);
                        break (end + 4, size);
                    }
                };
                while bytes.len() < header_end + size {
                    let mut buffer = [0u8; 4096];
                    let n = socket.read(&mut buffer).await.unwrap();
                    if n == 0 { return; }
                    bytes.extend_from_slice(&buffer[..n]);
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.lines().next().unwrap().split_whitespace().nth(1).unwrap();
                let form: HashMap<String,String> = reqwest::Url::parse(&format!("http://localhost/?{}", String::from_utf8_lossy(&bytes[header_end..header_end+size]))).unwrap().query_pairs().into_owned().collect();
                if path == "/token" && form.get("grant_type").map(String::as_str) == Some("refresh_token") {
                    let delay = { let mut state = shared.lock().unwrap(); state.refresh_requests += 1; state.refresh_delay };
                    if !delay.is_zero() { tokio::time::sleep(delay).await; }
                }
                if path == "/token" || path == "/revoke" {
                    let authorization=request.lines().find_map(|l|l.split_once(':').filter(|(k,_)|k.eq_ignore_ascii_case("authorization")).map(|(_,v)|v.trim().to_string())).unwrap_or_default();
                    shared.lock().unwrap().client_auth.push((form.clone(),authorization));
                }
                let mut status = "200 OK";
                let mut extra = String::new();
                let body = match path {
                    "/mcp" => { status = "401 Unauthorized"; extra = format!("WWW-Authenticate: Bearer resource_metadata=\"{base}/metadata\"\r\n"); serde_json::json!({}) },
                    "/metadata" | "/.well-known/oauth-protected-resource" => serde_json::json!({"resource":if origin_resource { base.clone() } else { format!("{base}/mcp") },"authorization_servers":[base]}),
                    "/.well-known/oauth-authorization-server" => serde_json::json!({"issuer":base,"authorization_endpoint":format!("{base}/authorize"),"token_endpoint":format!("{base}/token"),"registration_endpoint":format!("{base}/register"),"revocation_endpoint":format!("{base}/revoke"),"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none","client_secret_post","client_secret_basic"]}),
                    "/register" => {
                        let body: serde_json::Value = serde_json::from_slice(&bytes[header_end..header_end+size]).unwrap();
                        assert_eq!(body["token_endpoint_auth_method"], "none");
                        assert!(body["redirect_uris"][0].as_str().unwrap().starts_with("http://127.0.0.1:"));
                        let mut state = shared.lock().unwrap(); state.registrations += 1;
                        serde_json::json!({"client_id":format!("client-{}",state.registrations),"token_endpoint_auth_method":"none"})
                    },
                    "/token" => {
                        if form["grant_type"] == "authorization_code" {
                            assert!(!form["code_verifier"].is_empty());
                            assert_eq!(form["resource"], if origin_resource { base.clone() } else { format!("{base}/mcp") });
                            let account = &form["code"];
                            serde_json::json!({"access_token":format!("access-{account}"),"refresh_token":format!("refresh-{account}"),"token_type":"Bearer","expires_in":0})
                        } else {
                            assert_eq!(form["resource"], if origin_resource { base.clone() } else { format!("{base}/mcp") });
                            let token = &form["refresh_token"];
                            let mut state = shared.lock().unwrap();
                            *state.refreshes.entry(token.clone()).or_default() += 1;
                            if state.revoked.contains(token) {
                                status = "400 Bad Request";
                                serde_json::json!({"error":"invalid_grant","error_description":format!("private token {token}")})
                            } else {
                                // Strict providers invalidate the used refresh token once it is rotated.
                                if state.strict_rotation { state.revoked.push(token.clone()); }
                                let account = token.strip_prefix("refresh-").unwrap();
                                serde_json::json!({"access_token":format!("fresh-{account}"),"refresh_token":format!("rotated-{account}"),"token_type":"Bearer","expires_in":3600})
                            }
                        }
                    },
                    "/revoke" => { shared.lock().unwrap().revoked.push(form["token"].clone()); serde_json::json!({}) },
                    _ => { status = "404 Not Found"; serde_json::json!({}) },
                }.to_string();
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                // The client may have gone away (cancelled caller); the provider-side effects already happened.
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    Provider { base, state, task }
}

#[tokio::test]
async fn test_apps_oauth_declared_resource_survives_login_and_refresh() {
    let provider = provider_with_resource(true).await;
    let http = OAuthHttp::loopback();
    let endpoint = format!("{}/mcp", provider.base);
    assert!(begin_with(&http, &endpoint, None, &[]).await.is_err());
    let pending = begin_with_resource(&http, &endpoint, None, &[], Some(&provider.base)).await.unwrap();
    let query: HashMap<_,_> = reqwest::Url::parse(&pending.url).unwrap().query_pairs().into_owned().collect();
    assert_eq!(query["resource"], provider.base);
    let mut callback = reqwest::Url::parse(&pending.redirect).unwrap();
    callback.query_pairs_mut().append_pair("state", &pending.state).append_pair("code", "resource-test");
    let (credential, response) = tokio::join!(finish_with(&http, pending), http.client.get(callback).send());
    assert!(response.unwrap().status().is_success());
    let mut credential = credential.unwrap(); credential.server_id = 77;
    assert_eq!(credential.endpoint, endpoint);
    assert_eq!(credential.oauth.as_ref().unwrap().resource.as_deref(), Some(provider.base.as_str()));
    let store = Arc::new(MemoryStore::default()); store.save("resource", &credential).unwrap();
    assert_eq!(credentials::resolve_with(store, &http, "resource", &endpoint, 77).await.unwrap()["authorization"], "Bearer fresh-resource-test");
}

#[tokio::test]
async fn test_apps_oauth_declared_resource_rejects_other_origin() {
    let http = OAuthHttp::loopback();
    let result = begin_with_resource(&http, "http://127.0.0.1:1/mcp", None, &[], Some("http://127.0.0.1:2")).await;
    assert!(result.err().unwrap().contains("同源"));
}

async fn login(http: &OAuthHttp, provider: &Provider, account: &str, id: i64) -> Credential {
    let pending = begin_with(http, &format!("{}/mcp",provider.base), None, &["read".into()]).await.unwrap();
    let query: HashMap<_,_> = reqwest::Url::parse(&pending.url).unwrap().query_pairs().into_owned().collect();
    assert_eq!(query["code_challenge_method"], "S256");
    assert_ne!(query["code_challenge"], pending.verifier);
    let mut callback = reqwest::Url::parse(&pending.redirect).unwrap();
    callback.query_pairs_mut().append_pair("state", &pending.state).append_pair("code", account);
    let (credential, response) = tokio::join!(finish_with(http, pending), http.client.get(callback).send());
    assert!(response.unwrap().status().is_success());
    let mut credential = credential.unwrap(); credential.server_id = id; credential
}

#[derive(Default)]
struct MemoryStore(Mutex<HashMap<String, Credential>>);
impl CredentialStore for MemoryStore {
    fn load(&self, key: &str) -> Result<Credential,String> { self.0.lock().unwrap().get(key).cloned().ok_or("missing credential".into()) }
    fn save(&self, key: &str, value: &Credential) -> Result<(),String> { self.0.lock().unwrap().insert(key.into(), value.clone()); Ok(()) }
}

#[tokio::test]
async fn test_apps_oauth_two_connections_refresh_once_and_keep_identity() {
    let provider = provider().await;
    let http = OAuthHttp::loopback();
    let a = login(&http, &provider, "account-a", 11).await;
    let b = login(&http, &provider, "account-b", 12).await;
    let endpoint = a.endpoint.clone();
    let store = Arc::new(MemoryStore::default());
    store.save("a", &a).unwrap(); store.save("b", &b).unwrap();
    let (first, second, other) = tokio::join!(
        credentials::resolve_with(store.clone(), &http, "a", &endpoint, 11),
        credentials::resolve_with(store.clone(), &http, "a", &endpoint, 11),
        credentials::resolve_with(store.clone(), &http, "b", &endpoint, 12));
    assert_eq!(first.unwrap()["authorization"], "Bearer fresh-account-a");
    assert_eq!(second.unwrap()["authorization"], "Bearer fresh-account-a");
    assert_eq!(other.unwrap()["authorization"], "Bearer fresh-account-b");
    assert_eq!(provider.state.lock().unwrap().refreshes["refresh-account-a"], 1);
    assert_eq!(provider.state.lock().unwrap().refreshes["refresh-account-b"], 1);
    assert!(credentials::resolve_with(store.clone(), &http, "a", &endpoint, 12).await.is_err());
    // A new store loaded from persisted data models a fresh process, without retaining a session cache.
    let restarted = Arc::new(MemoryStore::default());
    let saved = serde_json::to_string(&store.load("a").unwrap()).unwrap();
    restarted.save("a", &serde_json::from_str(&saved).unwrap()).unwrap();
    assert_eq!(credentials::resolve_with(restarted, &http, "a", &endpoint, 11).await.unwrap()["authorization"], "Bearer fresh-account-a");
    assert_eq!(provider.state.lock().unwrap().refreshes["refresh-account-a"], 1);
}

#[tokio::test]
async fn test_apps_oauth_revocation_blocks_refresh_and_keeps_errors_secret_free() {
    let provider = provider().await;
    let http = OAuthHttp::loopback();
    let credential = login(&http, &provider, "account-a", 11).await;
    assert!(revoke_with(&http, &credential).await.unwrap());
    assert!(provider.state.lock().unwrap().revoked.contains(&"refresh-account-a".into()));
    assert!(provider.state.lock().unwrap().revoked.contains(&"access-account-a".into()));
    let store = Arc::new(MemoryStore::default()); store.save("a", &credential).unwrap();
    let error = credentials::resolve_with(store.clone(), &http, "a", &credential.endpoint, 11).await.unwrap_err();
    assert!(error.contains("invalid_grant"));
    assert!(!error.contains("refresh-account-a"));
    assert_eq!(store.load("a").unwrap().oauth.unwrap().access_token, "access-account-a");
    let relogged = login(&http, &provider, "account-b", 11).await;
    store.save("a", &relogged).unwrap();
    assert_eq!(credentials::resolve_with(store, &http, "a", &credential.endpoint, 11).await.unwrap()["authorization"], "Bearer fresh-account-b");
}

#[tokio::test]
async fn test_apps_oauth_denial_never_exchanges_a_token() {
    let provider = provider().await;
    let http = OAuthHttp::loopback();
    let pending = begin_with(&http, &format!("{}/mcp",provider.base), Some("known-client"), &[]).await.unwrap();
    let callback = format!("{}?state={}&error=access_denied",pending.redirect,pending.state);
    let (result, _) = tokio::join!(finish_with(&http, pending), http.client.get(callback).send());
    assert!(result.err().unwrap().contains("拒绝"));
    assert_eq!(provider.state.lock().unwrap().registrations, 0);
    assert!(provider.state.lock().unwrap().refreshes.is_empty());
}

#[tokio::test]
async fn test_apps_cancelled_refresh_keeps_write_serialized_with_disconnect() {
    struct BlockingStore {
        credential: Credential,
        started: Arc<tokio::sync::Notify>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl CredentialStore for BlockingStore {
        fn load(&self, _: &str) -> Result<Credential,String> { Ok(self.credential.clone()) }
        fn save(&self, _: &str, _: &Credential) -> Result<(),String> {
            self.started.notify_one();
            self.release.lock().unwrap().recv_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())
        }
    }
    let provider = provider().await;
    let http = OAuthHttp::loopback();
    let credential = login(&http, &provider, "account-a", 11).await;
    let endpoint = credential.endpoint.clone();
    let started = Arc::new(tokio::sync::Notify::new());
    let (release, receiver) = std::sync::mpsc::channel();
    let store = Arc::new(BlockingStore { credential, started: started.clone(), release: Mutex::new(receiver) });
    let job = tokio::spawn(async move { credentials::resolve_with(store, &http, "a", &endpoint, 11).await });
    tokio::time::timeout(Duration::from_secs(5), started.notified()).await.unwrap();
    job.abort();
    assert!(job.await.unwrap_err().is_cancelled());
    assert!(credentials::CREDENTIAL_LOCK.try_lock().is_err(), "disconnect must wait until the in-flight credential write finishes");
    release.send(()).unwrap();
    let _guard = tokio::time::timeout(Duration::from_secs(5), credentials::CREDENTIAL_LOCK.lock()).await.unwrap();
}

#[tokio::test]
async fn test_apps_oauth_confidential_client_login_refresh_and_revoke() {
    for method in ["client_secret_post","client_secret_basic"] {
        let provider=provider().await;let http=OAuthHttp::loopback();let endpoint=format!("{}/mcp",provider.base);
        let port=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let redirect=format!("http://127.0.0.1:{}/custom-callback",port.local_addr().unwrap().port());drop(port);
        let pending=begin_options(&http,&endpoint,Some("own-client"),&[],None,method,Some("own-test-secret"),Some(&redirect)).await.unwrap();
        assert!(!pending.url.contains("own-test-secret"));assert_eq!(pending.redirect,redirect);
        let mut callback=reqwest::Url::parse(&pending.redirect).unwrap();callback.query_pairs_mut().append_pair("state",&pending.state).append_pair("code","own-account");
        let (credential,response)=tokio::join!(finish_with(&http,pending),http.client.get(callback).send());
        assert!(response.unwrap().status().is_success());let mut credential=credential.unwrap();credential.server_id=71;
        let store=Arc::new(MemoryStore::default());store.save("own",&credential).unwrap();
        assert_eq!(credentials::resolve_with(store.clone(),&http,"own",&endpoint,71).await.unwrap()["authorization"],"Bearer fresh-own-account");
        revoke_with(&http,&store.load("own").unwrap()).await.unwrap();
        let captured=provider.state.lock().unwrap();assert_eq!(captured.registrations,0);assert_eq!(captured.client_auth.len(),4);
        for (form,authorization) in &captured.client_auth {
            if method=="client_secret_post" {assert_eq!(form["client_secret"],"own-test-secret");assert!(authorization.is_empty());}
            else {assert!(!form.contains_key("client_secret"));assert_eq!(authorization, &format!("Basic {}",base64::engine::general_purpose::STANDARD.encode("own-client:own-test-secret")));}
        }
    }
}

#[test]
fn test_apps_oauth_fixed_redirect_rejects_non_loopback_targets() {
    for uri in ["https://example.com/callback","http://127.0.0.1/callback","http://127.0.0.1:0/callback","http://user@localhost:1234/callback"] { assert!(redirect_target(uri).is_err(),"{uri}"); }
    assert!(redirect_target("http://localhost:1234/callback").is_ok());
}

/// Regression: the token exchange ran inside the caller's future. A tool timeout or cancel after the
/// provider rotated the refresh token dropped the response, leaving only the now-invalid old token.
#[tokio::test]
async fn test_apps_oauth_refresh_survives_caller_cancellation() {
    let provider = provider().await;
    { let mut state = provider.state.lock().unwrap(); state.refresh_delay = Duration::from_millis(300); state.strict_rotation = true; }
    let http = OAuthHttp::loopback();
    let credential = login(&http, &provider, "account-c", 31).await;
    let endpoint = credential.endpoint.clone();
    let store = Arc::new(MemoryStore::default()); store.save("c", &credential).unwrap();
    let job = { let (store, http, endpoint) = (store.clone(), http.clone(), endpoint.clone());
        tokio::spawn(async move { credentials::resolve_with(store, &http, "c", &endpoint, 31).await }) };
    tokio::time::timeout(Duration::from_secs(5), async { while provider.state.lock().unwrap().refresh_requests == 0 { tokio::time::sleep(Duration::from_millis(10)).await; } }).await.unwrap();
    job.abort();
    let _ = job.await;
    tokio::time::timeout(Duration::from_secs(5), async { loop {
        if store.load("c").unwrap().oauth.unwrap().refresh_token.as_deref() == Some("rotated-account-c") { break; }
        tokio::time::sleep(Duration::from_millis(20)).await;
    } }).await.expect("the rotated refresh token must be stored even though the caller was cancelled");
    assert_eq!(credentials::resolve_with(store, &http, "c", &endpoint, 31).await.unwrap()["authorization"], "Bearer fresh-account-c");
    assert_eq!(provider.state.lock().unwrap().refreshes["refresh-account-c"], 1);
}

/// Regression: the callback listener accepted exactly one connection, so a browser preconnect or a
/// favicon probe consumed it and the login failed or hung until the five-minute timeout.
#[tokio::test]
async fn test_apps_oauth_callback_ignores_stray_connections_before_the_callback() {
    let provider = provider().await;
    let http = OAuthHttp::loopback();
    let pending = begin_with(&http, &format!("{}/mcp", provider.base), Some("known-client"), &[]).await.unwrap();
    let redirect = reqwest::Url::parse(&pending.redirect).unwrap();
    let address = format!("127.0.0.1:{}", redirect.port().unwrap());
    let mut callback = redirect.clone();
    callback.query_pairs_mut().append_pair("state", &pending.state).append_pair("code", "account-s");
    let browser = async {
        drop(tokio::net::TcpStream::connect(&address).await.unwrap());
        let idle = tokio::net::TcpStream::connect(&address).await.unwrap();
        let favicon = http.client.get(format!("http://{address}/favicon.ico")).send().await.unwrap();
        assert_eq!(favicon.status(), reqwest::StatusCode::NOT_FOUND);
        let response = http.client.get(callback).send().await.unwrap();
        drop(idle);
        response
    };
    let (credential, response) = tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(finish_with(&http, pending), browser) }).await
        .expect("an idle preconnected socket must not hold up the real callback");
    assert!(response.status().is_success());
    assert_eq!(credential.unwrap().oauth.unwrap().access_token, "access-account-s");
}

/// OAuth traffic built from a connection's client builder honours its proxy (here an unreachable one),
/// which is what lets refresh and revoke follow the connection's network settings.
#[tokio::test]
async fn test_apps_oauth_refresh_uses_the_connection_network_settings() {
    let provider = provider().await;
    let http = OAuthHttp::loopback();
    let credential = login(&http, &provider, "account-p", 41).await;
    let store = Arc::new(MemoryStore::default()); store.save("p", &credential).unwrap();
    let proxied = OAuthHttp::loopback_with(reqwest::Client::builder().proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap()));
    let error = credentials::resolve_with(store, &proxied, "p", &credential.endpoint, 41).await.unwrap_err();
    assert!(error.contains("OAuth 令牌请求失败"), "{error}");
    assert!(!provider.state.lock().unwrap().refreshes.contains_key("refresh-account-p"));
}
