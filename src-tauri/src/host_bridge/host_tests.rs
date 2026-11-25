use super::{conversations, store, types::*};
use rusqlite::{params, Connection};
use uuid::Uuid;

#[test]
fn test_send_receipt_is_idempotent_per_device_and_rejects_changed_payload() {
    let conn = Connection::open_in_memory().unwrap();
    store::init(&conn).unwrap();
    let request = Uuid::new_v4();
    let turn = super::chat::reserve(&conn, "phone-A",request,b"original").unwrap().unwrap();
    assert!(super::chat::reserve(&conn,"phone-A",request,b"original").unwrap().is_none());
    assert!(super::chat::reserve(&conn,"phone-A",request,b"changed").is_err());
    let other = super::chat::reserve(&conn,"phone-B",request,b"original").unwrap().unwrap();
    assert_ne!(other,turn);
    assert_eq!(conn.query_row("SELECT count(*) FROM host_bridge_send",[],|r|r.get::<_,i64>(0)).unwrap(),2);
}

#[test]
fn test_chat_permission_defaults_to_read_only_for_older_pairings() {
    let grant: Access = serde_json::from_value(serde_json::json!({"assistants":[7],"butler":true})).unwrap();
    assert!(!grant.chat);
    for payload in [serde_json::json!({"action":"send","destination":{"kind":"new","assistant":7,"cwd":"extra"},"request":Uuid::new_v4(),"text":"x"}),
        serde_json::json!({"action":"send","destination":{"kind":"existing","conversation":10},"request":Uuid::new_v4(),"text":"x","model":"override"}),
        serde_json::json!({"action":"stop","conversation":10})] {
        assert!(serde_json::from_value::<ChatAction>(payload).is_err());
    }
}

#[test]
fn test_pairing_invitation_is_consumed_once_and_checks_expiry_and_version() {
    use std::time::{Duration,Instant};
    let mut running = super::host::Running {
        endpoint: Endpoint { name:"测试电脑".into(), address:"192.168.1.8".parse().unwrap(), certificate:String::new() },
        stop: Default::default(), invitation: None, pending: None,
    };
    let code = Uuid::new_v4();
    let request = |version| PairRequest { code, name:"测试手机".into(), protocol_version:version };
    running.invitation = Some((code,Instant::now()));
    assert!(running.request(request(VERSION)).is_err());
    running.invitation = Some((code,Instant::now()+Duration::from_secs(60)));
    assert!(running.request(request(VERSION+1)).is_err());
    let (_id, reply) = running.request(request(VERSION)).unwrap();
    assert!(running.request(request(VERSION)).is_err());
    assert!(running.pending.is_some());
    drop(reply);
}

#[test]
fn test_device_authorization_requires_the_issued_token_and_ends_on_revocation() {
    let conn = Connection::open_in_memory().unwrap();
    store::init(&conn).unwrap();
    let device = Device { id: Uuid::new_v4(), name: "手机".into(), access: Access { assistants: vec![7], butler: false, chat: false } };
    store::add(&conn,&device,"test-device-token").unwrap();
    assert!(store::access(&conn,"other-phone").unwrap().is_none());
    assert_eq!(store::access(&conn,"test-device-token").unwrap().unwrap().assistants, vec![7]);
    let persisted: Vec<u8> = conn.query_row("SELECT token_hash FROM host_bridge_peer", [], |r|r.get(0)).unwrap();
    assert_eq!(persisted.len(), 32);
    store::revoke(&conn,device.id).unwrap();
    assert!(store::access(&conn,"test-device-token").unwrap().is_none());
}

#[test]
fn test_history_filters_conversation_ownership_and_model_only_context() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,name TEXT,conversation_kind TEXT,
        assistant_id INTEGER,is_hidden_from_normal_chat_list INTEGER);
        CREATE TABLE butler_main_state(butler_conversation_id INTEGER,slot TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,message_type TEXT,content TEXT,
            metadata_json TEXT,tool_calls_json TEXT,finish_time INTEGER);
        INSERT INTO conversation VALUES(10,'聊天','normal',7,0),(11,'未授权','normal',8,0),
            (12,'总管家','butler_main',99,1),(13,'旧主会话','butler_main',99,1),
            (14,'私聊','butler_contact',7,1),(15,'旧私聊','butler_contact_archive',7,1),(16,'任务','butler_task',7,1);
        INSERT INTO butler_main_state VALUES(12,'default');").unwrap();
    let context = crate::utils::turn_context::wrap(["secret-hidden-context"]).unwrap();
    let body = crate::utils::turn_context::append("问题",Some(&context));
    conn.execute("INSERT INTO message VALUES(1,10,'user',?1,NULL,NULL,NULL)", params![body]).unwrap();
    conn.execute_batch("INSERT INTO message VALUES(2,10,'reasoning','private reasoning',NULL,NULL,1),
        (3,10,'response','答复',NULL,NULL,1),(4,10,'user','internal work','{\"origin\":\"internal\"}',NULL,1);").unwrap();
    let grant = Access { assistants: vec![7], butler: true, chat: true };
    for id in [11,13,15,16] { assert!(conversations::history(&conn,&grant,id,99).is_err()); }
    for id in [12,14] { assert!(conversations::history(&conn,&grant,id,99).is_ok()); }
    let rows = conversations::history(&conn,&grant,10,99).unwrap();
    assert_eq!(rows["messages"].as_array().unwrap().len(),2);
    assert_eq!(rows["messages"][0]["content"],"问题");
    assert_eq!(rows["messages"][1]["content"],"答复");
    assert_eq!(conversations::history(&conn,&grant,10,3).unwrap()["messages"].as_array().unwrap().len(),1);
    conn.execute_batch("INSERT INTO message VALUES
        (5,10,'error','private diagnostic',NULL,NULL,1),
        (6,10,'user','公开问题',NULL,NULL,NULL),
        (7,10,'response','正在回复',NULL,NULL,NULL),
        (8,10,'error','provider response and credentials',NULL,NULL,1);").unwrap();
    let page = conversations::history(&conn,&grant,10,99).unwrap();
    let visible = page["messages"].as_array().unwrap();
    assert!(visible.iter().any(|r|r["id"]==7 && r["content"]=="正在回复"));
    assert!(!visible.iter().any(|r|r["id"]==5));
    assert!(visible.iter().any(|r|r["id"]==8));
    assert!(!page.to_string().contains("credentials"));
    assert!(conversations::history(&conn,&Access { butler:false,..grant },12,99).is_err());
}

#[test]
fn test_host_identity_is_independent_of_name_and_address() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let certificate = || STANDARD.encode(rcgen::generate_simple_self_signed(vec![DNS_NAME.into()]).unwrap().cert.der());
    let endpoint = Endpoint { name:"电脑".into(), address:"192.168.1.9".parse().unwrap(), certificate:certificate() };
    let original = endpoint.id().unwrap();
    let mut changed = endpoint;
    changed.name = "新名字".into();
    changed.address = "10.0.0.2".parse().unwrap();
    assert_eq!(changed.id().unwrap(), original);
    changed.certificate = certificate();
    assert_ne!(changed.id().unwrap(), original);
    changed.address = "0.0.0.0".parse().unwrap();
    assert!(changed.id().is_err());
}

#[tokio::test]
async fn test_phone_tls_accepts_only_the_paired_desktop_key() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use http_body_util::Full;
    use hyper::{body::Bytes, Response};
    use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
    use std::{convert::Infallible, sync::Arc, time::Duration};
    crate::ensure_rustls_crypto_provider();
    let identity = rcgen::generate_simple_self_signed(vec![DNS_NAME.into()]).unwrap();
    let config = rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(
        vec![CertificateDer::from(identity.cert.der().to_vec())],
        PrivatePkcs8KeyDer::from(identity.key_pair.serialize_der()).into(),
    ).unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.unwrap();
            if let Ok(stream) = acceptor.accept(stream).await {
                let service = hyper::service::service_fn(|_| async {
                    Ok::<_,Infallible>(Response::new(Full::new(Bytes::from_static(b"paired"))))
                });
                hyper::server::conn::http1::Builder::new().keep_alive(false)
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service).await.unwrap();
            }
        }
    });
    let url = format!("https://{DNS_NAME}:{}/",address.port());
    let other = rcgen::generate_simple_self_signed(vec![DNS_NAME.into()]).unwrap();
    let timeout = Duration::from_secs(4);
    let wrong_client = pinned_client(&STANDARD.encode(other.cert.der()),address,timeout).unwrap();
    assert!(wrong_client.get(&url).send().await.is_err());
    let paired_client = pinned_client(&STANDARD.encode(identity.cert.der()),address,timeout).unwrap();
    assert_eq!(paired_client.get(&url).send().await.unwrap().text().await.unwrap(),"paired");
    tokio::time::timeout(timeout,server).await.unwrap().unwrap();
}
