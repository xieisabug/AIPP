use super::{store, types::*};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{body::{Bytes, Incoming}, Method, Request, Response, StatusCode};
use rand::RngCore;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{convert::Infallible, net::Ipv4Addr, sync::Arc, time::{Duration, Instant}};
use tauri::{AppHandle, Manager};
use tokio::{net::TcpListener, sync::{oneshot, Mutex, Semaphore}};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const PAIRING_TTL: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct Host(pub Mutex<Option<Running>>);

pub struct Running {
    pub endpoint: Endpoint,
    pub stop: CancellationToken,
    pub invitation: Option<(Uuid, Instant)>,
    pub pending: Option<Pending>,
}

pub struct Pending {
    pub id: Uuid,
    pub name: String,
    pub reply: oneshot::Sender<Session>,
}

#[derive(Serialize, Deserialize)]
struct Identity {
    cert: String,
    key: String,
}

fn tls(name: String, address: Ipv4Addr) -> Result<(Endpoint, TlsAcceptor)> {
    let entry = keyring::Entry::new("com.aipp.host-bridge", "tls-identity-v1")?;
    let identity: Identity = match entry.get_password() {
        Ok(raw) => serde_json::from_str(&raw).context("读取设备互联 TLS 身份")?,
        Err(keyring::Error::NoEntry) => {
            let generated = rcgen::generate_simple_self_signed(vec![DNS_NAME.to_string()])?;
            let identity = Identity {
                cert: STANDARD.encode(generated.cert.der()),
                key: STANDARD.encode(generated.key_pair.serialize_der()),
            };
            entry.set_password(&serde_json::to_string(&identity)?)?;
            identity
        }
        Err(error) => return Err(error).context("打开设备互联系统钥匙串"),
    };
    let config = rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(
        vec![CertificateDer::from(STANDARD.decode(&identity.cert)?)],
        PrivatePkcs8KeyDer::from(STANDARD.decode(&identity.key)?).into(),
    )?;
    Ok((Endpoint { name, address, certificate: identity.cert }, TlsAcceptor::from(Arc::new(config))))
}

pub fn addresses() -> Result<Vec<Ipv4Addr>> {
    let mut addresses: Vec<_> = if_addrs::get_if_addrs()?.into_iter().filter_map(|iface| match iface.ip() {
        std::net::IpAddr::V4(ip) if ip.is_private() || ip.is_link_local() => Some(ip),
        _ => None,
    }).collect();
    addresses.sort_unstable();
    addresses.dedup();
    Ok(addresses)
}

pub async fn start(app: &AppHandle, name: String, address: Ipv4Addr) -> Result<()> {
    valid_name(&name)?;
    anyhow::ensure!(addresses()?.contains(&address), "所选局域网网卡地址已不可用");
    let host = app.state::<Host>();
    let mut guard = host.0.lock().await;
    anyhow::ensure!(guard.is_none(), "设备互联已开启");
    let listener = TcpListener::bind((address, PORT)).await.context("监听设备互联地址")?;
    let (endpoint, acceptor) = tokio::task::spawn_blocking(move || tls(name, address)).await??;
    let stop = CancellationToken::new();
    *guard = Some(Running { endpoint, stop: stop.clone(), invitation: None, pending: None });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let slots = Arc::new(Semaphore::new(32));
        loop {
            let accepted = tokio::select! { _ = stop.cancelled() => break, result = listener.accept() => result };
            let (stream, peer) = match accepted {
                Ok(pair) => pair,
                Err(error) => {
                    tracing::warn!(%error, "Host Bridge accept failed");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            if !matches!(peer.ip(), std::net::IpAddr::V4(ip) if ip.is_private() || ip.is_link_local()) { continue; }
            let Ok(permit) = slots.clone().try_acquire_owned() else { continue };
            let (acceptor, app, closed) = (acceptor.clone(), app.clone(), stop.clone());
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                let work = async move {
                    let stream = tokio::time::timeout(Duration::from_secs(8), acceptor.accept(stream)).await??;
                    let service = hyper::service::service_fn(move |request| serve(app.clone(), request));
                    hyper::server::conn::http1::Builder::new().keep_alive(false)
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service).await?;
                    Ok::<_, anyhow::Error>(())
                };
                tokio::select! { _ = closed.cancelled() => (), _ = tokio::time::timeout(Duration::from_secs(150), work) => () }
            });
        }
    });
    Ok(())
}

impl Running {
    pub fn invite(&mut self) -> Result<String> {
        self.pending.take();
        let code = Uuid::new_v4();
        let payload = serde_json::to_string(&Invitation {
            protocol_version: VERSION, endpoint: self.endpoint.clone(), code,
        })?;
        let qr = qrcode::QrCode::new(payload.as_bytes()).context("生成配对二维码")?;
        self.invitation = Some((code, Instant::now() + PAIRING_TTL));
        Ok(qr.render::<qrcode::render::svg::Color>().min_dimensions(300, 300).build())
    }

    pub(super) fn request(&mut self, request: PairRequest) -> Result<(Uuid, oneshot::Receiver<Session>)> {
        valid_name(&request.name)?;
        anyhow::ensure!(request.protocol_version == VERSION, "手机和电脑的设备互联协议版本不兼容");
        anyhow::ensure!(matches!(self.invitation, Some((code, until)) if code == request.code && Instant::now() < until),
            "配对码无效或已过期，请重新扫码");
        self.invitation = None;
        let id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        self.pending = Some(Pending { id, name: request.name, reply: tx });
        Ok((id, rx))
    }

    pub fn decide(&mut self, app: &AppHandle, id: Uuid, grant: Option<Access>) -> Result<()> {
        anyhow::ensure!(self.pending.as_ref().is_some_and(|p| p.id == id), "手机的配对请求已经结束");
        let pending = self.pending.take().unwrap();
        if let Some(access) = grant {
            anyhow::ensure!(!pending.reply.is_closed(), "手机等待配对超时，请重新扫码");
            let mut random = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut random);
            let token = hex::encode(random);
            let db = store::open(app)?;
            store::add(&db, &Device { id, name: pending.name, access }, &token)?;
            if pending.reply.send(Session { token }).is_err() {
                store::revoke(&db, id)?;
                anyhow::bail!("手机连接已结束，请重新配对");
            }
        }
        Ok(())
    }
}

async fn pair(app: &AppHandle, body: Incoming) -> Result<Session> {
    let body = tokio::time::timeout(Duration::from_secs(8), Limited::new(body, 2048).collect())
        .await.context("读取配对请求超时")?.map_err(|_| anyhow::anyhow!("配对请求不完整或超过大小限制"))?;
    let request = serde_json::from_slice(&body.to_bytes()).context("配对请求格式无效")?;
    let host = app.state::<Host>();
    let (id, receiver) = host.0.lock().await.as_mut().context("设备互联已关闭")?.request(request)?;
    let decision = tokio::time::timeout(Duration::from_secs(120), receiver).await;
    let mut host = host.0.lock().await;
    if let Some(host) = host.as_mut() {
        if host.pending.as_ref().is_some_and(|pending| pending.id == id) { host.pending.take(); }
    }
    decision.context("等待电脑确认超时")?.context("电脑已拒绝或取消配对")
}

fn json_response(status: StatusCode, data: Value) -> Response<Full<Bytes>> {
    Response::builder().status(status).header("content-type", "application/json")
        .header("cache-control","no-store").body(Full::new(Bytes::from(data.to_string()))).unwrap()
}

async fn serve(app: AppHandle, req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    let reply = match (req.method(), req.uri().path()) {
        (&Method::POST, "/v1/pair") => match pair(&app, req.into_body()).await {
            Ok(session) => json_response(StatusCode::OK, json!(session)),
            Err(e) => json_response(StatusCode::FORBIDDEN, json!({"error":e.to_string()})),
        },
        (&Method::POST, "/v1/chat") => {
            let device = req.headers().get("authorization").and_then(|h| h.to_str().ok())
                .and_then(|h| h.strip_prefix("Bearer ")).filter(|t| t.len() == 64)
                .map(|token| store::open(&app).and_then(|db| store::paired_device(&db, token))).transpose();
            match device {
                Ok(Some(Some((id, access)))) if access.chat => {
                    let action = async {
                        let body = tokio::time::timeout(Duration::from_secs(8), Limited::new(req.into_body(), 192*1024).collect())
                            .await.context("读取对话请求超时")?.map_err(|_| anyhow::anyhow!("对话请求过大或不完整"))?;
                        super::chat::handle(&app, id, serde_json::from_slice(&body.to_bytes())?).await
                    }.await;
                    match action {
                        Ok(value) => json_response(StatusCode::OK, value),
                        Err(e) => json_response(StatusCode::BAD_REQUEST, json!({"error":format!("{e:#}")})),
                    }
                }
                Ok(Some(Some(_))) => json_response(StatusCode::FORBIDDEN,json!({"error":"此手机仅获准查看历史，请在电脑的设备互联设置中勾选「允许对话」"})),
                Ok(_) => json_response(StatusCode::UNAUTHORIZED,json!({"error":"配对已撤销或设备凭据无效"})),
                Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR,json!({"error":format!("读取设备授权：{e}")})),
            }
        }
        (&Method::GET, path) if path == "/v1/environment" || path.starts_with("/v1/history/") || path.starts_with("/v1/watch/") => {
            let token = req.headers().get("authorization").and_then(|h| h.to_str().ok())
                .and_then(|h| h.strip_prefix("Bearer ")).unwrap_or("");
            if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Ok(json_response(StatusCode::UNAUTHORIZED, json!({"error":"缺少有效的设备配对凭据"})));
            }
            match store::open(&app).and_then(|conn| store::access(&conn, token)) {
                Ok(Some(access)) => {
                    let result = if path == "/v1/environment" {
                        super::conversations::catalog(&app, &access)
                    } else if let Some(raw) = path.strip_prefix("/v1/watch/") {
                        async {
                            let id = raw.parse::<i64>()?;
                            super::conversations::target(&crate::db::conversation_db::ConversationDatabase::new(&app)?.get_connection()?, &access,id)?;
                            super::chat::wait_for_change(id).await;
                            let access = store::access(&store::open(&app)?,token)?.context("电脑已撤销手机配对")?;
                            let mut page = super::conversations::history(&crate::db::conversation_db::ConversationDatabase::new(&app)?.get_connection()?,&access,id,i64::MAX)?;
                            let runtime = app.state::<crate::state::activity_state::ConversationActivityManager>().get_runtime_state(id).await;
                            page["running"] = json!(runtime.is_running);
                            page["phase"] = json!(runtime.phase);
                            if runtime.is_running {
                                let summary = crate::api::butler_progress::get_butler_progress_summary(app.clone(),id).await;
                                page["progress"] = json!(summary).get("text").cloned().unwrap_or(Value::Null);
                            }
                            Ok(page)
                        }.await
                    } else {
                        (|| {
                            let (id, before) = path.trim_start_matches("/v1/history/").split_once('/').context("缺少会话编号或消息位置")?;
                            let db = crate::db::conversation_db::ConversationDatabase::new(&app)?;
                            super::conversations::history(&db.get_connection()?, &access, id.parse()?, before.parse()?)
                        })()
                    };
                    match result {
                        Ok(value) => json_response(StatusCode::OK, value),
                        Err(e) => json_response(StatusCode::BAD_REQUEST,json!({"error":e.to_string()})),
                    }
                }
                Ok(None) => json_response(StatusCode::UNAUTHORIZED, json!({"error":"配对已撤销或设备凭据无效"})),
                Err(e) => { tracing::error!(%e, "读取设备授权失败"); json_response(StatusCode::INTERNAL_SERVER_ERROR,json!({"error":"桌面读取设备授权失败"})) }
            }
        }
        _ => json_response(StatusCode::NOT_FOUND, json!({"error":"设备互联未开放此操作"})),
    };
    Ok(reply)
}
