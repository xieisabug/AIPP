use super::{runtime,format,store};
use serde_json::json;
use std::collections::HashMap;

#[test]
fn test_plugin_runtime_preserves_argv_and_expands_only_declared_variables() {
    let root=std::path::Path::new("plugin directory");
    let values=HashMap::from([("TOKEN".into(),"quoted $value & untouched".into())]);
    let config=json!({"command":"node","args":["./mcp/server.mjs","${CODEX_PLUGIN_ROOT}/asset name","${TOKEN}"],"env":{"AUTH":"Bearer ${TOKEN}"}});
    let launch=runtime::launch(&config,root,&values).unwrap();
    assert_eq!(launch.args[0],"./mcp/server.mjs");
    assert_eq!(launch.args[1],"plugin directory/asset name");
    assert_eq!(launch.args[2],values["TOKEN"]);
    assert_eq!(launch.env["AUTH"],"Bearer quoted $value & untouched");
    assert!(runtime::expand("${MISSING}",root,&values).unwrap_err().contains("MISSING"));
}

#[test]
fn test_plugin_runtime_connection_identity_has_no_environment_values() {
    let b=runtime::Binding {key:"a".repeat(24),name:"test".into(),digest:"digest".into(),reference:"vault-reference".into()};
    let encoded=format!("{}{}",runtime::PREFIX,serde_json::to_string(&b).unwrap());
    assert_eq!(runtime::binding(Some(&encoded)).unwrap().unwrap().reference,"vault-reference");
    assert!(runtime::binding(Some("node normal.js")).unwrap().is_none());
    assert!(runtime::binding(Some("agent-plugin-stdio:bad-json")).is_err());
}

#[tokio::test]
async fn test_plugin_runtime_node_receives_exact_environment_and_working_directory() {
    let dir=tempfile::tempdir().unwrap();
    let config=json!({"command":"node","args":["-e","process.stdout.write(JSON.stringify({cwd:process.cwd(),value:process.env.AIPP_TEST_VALUE,arg:process.argv[1]}))","spaces & literal $()"],"env":{"AIPP_TEST_VALUE":"${VALUE}"}});
    let values=HashMap::from([("VALUE".into(),"test-value".into())]);
    let output=runtime::command(runtime::launch(&config,dir.path(),&values).unwrap()).unwrap().output().await.unwrap();
    assert!(output.status.success());
    let result:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["value"],"test-value");
    assert_eq!(result["arg"],"spaces & literal $()");
    assert_eq!(std::path::Path::new(result["cwd"].as_str().unwrap()).canonicalize().unwrap(),dir.path().canonicalize().unwrap());
}

#[tokio::test]
#[ignore="Offline real package smoke test requires pinned AIPP_PLUGIN_TEST_ARCHIVE and Node.js"]
async fn test_plugin_runtime_real_upstream_stdio() {
    use rmcp::ServiceExt;
    let archive=std::fs::read(std::env::var("AIPP_PLUGIN_TEST_ARCHIVE").unwrap()).unwrap();
    for name in ["creative-production","data-analytics","openai-developers"] {
        let source=format::Source {repo:"openai/plugins".into(),git_ref:"main".into(),path:format!("plugins/{name}")};
        let files=format::unpack(&archive,&source).unwrap();
        let mut preview=format::inspect(source.clone(),"d420d13051bffe4543ed39e30fa46b87148e47dd".into(),&files).unwrap();
        preview.set_permissions(format::permissions(&archive,&source).unwrap());
        let dir=tempfile::tempdir().unwrap();store::install(dir.path(),&preview,&files).unwrap();
        assert!(!store::modified(dir.path(),&store::receipt(dir.path(),&preview.key).unwrap().unwrap()).unwrap());
        let root=store::active(dir.path(),&preview.key).unwrap();
        let config=&preview.servers.iter().find(|s|s.config["command"].is_string()).unwrap().config;
        let command=runtime::command(runtime::launch(config,&root,&HashMap::new()).unwrap()).unwrap();
        let transport=rmcp::transport::TokioChildProcess::new(command).unwrap();
        let client=tokio::time::timeout(std::time::Duration::from_secs(20),().serve(transport)).await.unwrap().unwrap();
        let tools=tokio::time::timeout(std::time::Duration::from_secs(20),client.list_all_tools()).await.unwrap().unwrap();
        assert!(!tools.is_empty(),"{name}");
        println!("{name}: {}",tools.iter().map(|t|t.name.as_ref()).collect::<Vec<_>>().join(", "));
        if name=="data-analytics" {
            let args=json!({"title":"AIPP synthetic smoke test","source":{"query":{"engine":"test","sql":"SELECT 42 AS value","description":"Synthetic fixture; no database query is executed."}},"rows":[{"value":42}],"columns":[{"key":"value","type":"number"}]});
            let request:rmcp::model::CallToolRequestParams=serde_json::from_value(json!({"name":"render_table","arguments":args})).unwrap();
            let result=client.call_tool(request).await.unwrap();
            assert!(!result.is_error.unwrap_or(false),"{:?}",result.content);
            assert!(result.structured_content.is_some());
            println!("data-analytics: render_table returned real structured data");
        }
        if name=="openai-developers" {
            let request:rmcp::model::CallToolRequestParams=serde_json::from_value(json!({"name":"confirm_openai_api_key_local_destination","arguments":{"workspacePath":root,"targetPath":".env"}})).unwrap();
            let result=client.call_tool(request).await.unwrap();
            assert!(!result.is_error.unwrap_or(false));
            assert_eq!(result.structured_content.unwrap()["status"],"not_approved");
            println!("openai-developers: elicitation decline respected; no key created");
        }
        client.cancel().await.unwrap();
    }
}

#[test]
fn test_plugin_stderr_decoder_preserves_lossy_utf8_across_every_split() {
    let mut input = "before中文😀after".as_bytes().to_vec();
    input.extend_from_slice(&[0xff, 0xe4, 0xb8, b'x', 0xe4, 0xb8]);
    for split in 0..=input.len() {
        let mut pending = input[..split].to_vec();
        let mut output = runtime::decode_stderr(&mut pending, false);
        pending.extend_from_slice(&input[split..]);
        output.push_str(&runtime::decode_stderr(&mut pending, false));
        output.push_str(&runtime::decode_stderr(&mut pending, true));
        assert_eq!(output, String::from_utf8_lossy(&input));
        assert!(pending.is_empty());
    }
}

#[tokio::test]
async fn test_plugin_stderr_redacts_before_ring_truncation() {
    use std::sync::{Arc, Mutex};
    // Longer than the entire diagnostic buffer: neither its beginning nor its tail
    // may survive the retention boundary, including before the process exits.
    let secret = format!("credential-start-{}-credential-tail", "private".repeat(3000));
    let redactor = Arc::new(crate::vault::executor::redactor_for(vec![secret.clone()]));
    let buffer = Arc::new(Mutex::new(String::new()));
    let input = format!("before\n{secret}\nafter");
    runtime::capture_stderr(input.as_bytes(), buffer.clone(), redactor).await;
    assert_eq!(&*buffer.lock().unwrap(), "before\n[REDACTED]\nafter");
}

#[tokio::test]
async fn test_plugin_stderr_encoded_context_values_and_split_unicode_are_redacted() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::sync::{Arc, Mutex};
    let credential = "plugin-credential-value";
    let placeholder = "秘密-first\nsecond-value";
    let configured = crate::vault::executor::redactor_for(vec![credential.to_string()]);
    let context = crate::vault::executor::redactor_for(vec![placeholder.to_string()]);
    let redactor = Arc::new(configured.merged(Some(&context)));
    let buffer = Arc::new(Mutex::new(String::new()));
    let escaped = serde_json::to_string(placeholder).unwrap();
    // The first Chinese code point straddles the reader's 2048-byte chunk.
    let input = format!("{}{placeholder}\n{}\n{}\nafter", "x".repeat(2047), STANDARD.encode(credential), &escaped[1..escaped.len()-1]);
    runtime::capture_stderr(input.as_bytes(), buffer.clone(), redactor.clone()).await;
    let output = buffer.lock().unwrap().clone();
    assert_eq!(output, format!("{}[REDACTED]\n[REDACTED]\n[REDACTED]\nafter", "x".repeat(2047)));
    let diagnostic = runtime::diagnostic_error(42, Some(1), "request", format!("echo: {escaped}"), &output, &redactor);
    assert!(!diagnostic.contains("second-value"));
    assert!(!diagnostic.contains("秘密"));
    assert!(diagnostic.contains("[REDACTED]"));
}

#[tokio::test]
async fn test_plugin_stderr_no_secret_keeps_utf8_tail_within_limit() {
    use std::sync::{Arc, Mutex};
    let input = "你".repeat(6000);
    let buffer = Arc::new(Mutex::new(String::new()));
    let redactor = Arc::new(crate::vault::executor::redactor_for(Vec::<String>::new()));
    runtime::capture_stderr(input.as_bytes(), buffer.clone(), redactor).await;
    let output = buffer.lock().unwrap();
    assert!(output.len() <= 16384);
    assert_eq!(output.as_str(), "你".repeat(16384 / 3));
}

#[tokio::test]
async fn test_plugin_stderr_read_error_hides_pending_prefix_and_error_secret() {
    use std::sync::{Arc, Mutex};
    struct FailingOutput(bool);
    impl tokio::io::AsyncRead for FailingOutput {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.0 {
                self.0 = false;
                buf.put_slice(b"safe\nsynthetic-prefix");
                std::task::Poll::Ready(Ok(()))
            } else {
                std::task::Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, "transport-error-credential")))
            }
        }
    }
    let redactor = Arc::new(crate::vault::executor::redactor_for(vec![
        "synthetic-prefix-rest".to_string(), "transport-error-credential".to_string(),
    ]));
    let buffer = Arc::new(Mutex::new(String::new()));
    runtime::capture_stderr(FailingOutput(true), buffer.clone(), redactor).await;
    let output = buffer.lock().unwrap();
    assert_eq!(output.as_str(), "safe\n[REDACTED]\n[stderr read error: [REDACTED]]");
}
