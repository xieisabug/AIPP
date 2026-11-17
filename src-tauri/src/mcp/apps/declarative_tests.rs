use super::*;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

fn sample() -> Operation {
    serde_json::from_value(json!({
        "name":"inspect_item", "description":"Read an item", "path":"/items/{id}", "method":"GET",
        "input_schema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}
    })).unwrap()
}

#[test]
fn test_request_rejects_ambiguous_path_and_unknown_parameters() {
    let client = reqwest::Client::new();
    let base = reqwest::Url::parse("https://service.example").unwrap();
    for args in [json!({"id":".."}), json!({"id":"a/b"}), json!({"id":"%2e"}), json!({}),
        json!({"id":"123","url":"https://elsewhere.example"})] {
        assert!(sample().request(&client, base.clone(), &args).is_err());
    }
    let req = sample().request(&client, base, &json!({"id":"a b"})).unwrap().build().unwrap();
    assert_eq!(req.url().as_str(), "https://service.example/items/a%20b");
}

// Allows a separately released catalog to exercise the actual request encoder and result reader
// against a loopback provider. The fixture contains a manifest and independent wire expectations.
#[tokio::test]
#[ignore = "requires an external application's contract fixture"]
async fn test_external_http_app_contract() {
    let path = std::env::var("AIPP_HTTP_APP_CONTRACT").expect("path to contract fixture");
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let def: AppDefinition = serde_json::from_value(fixture["app"].clone()).unwrap();
    super::super::catalog::validate_app(&def).unwrap();
    let values = def.fields.iter().filter(|f| f.secret).map(|f| (f.key.clone(), "mock-secret".to_owned())).collect();
    let (config, bound) = def.configuration(&values).unwrap();
    assert!(config.values().all(|v| v != "mock-secret"));
    let redactor = http_transport::ResponseRedactor::new(http_transport::secret_header_values(&bound, Some(&def.secret_headers())).into_iter());
    let header_map = reqwest::header::HeaderMap::try_from(&bound).unwrap();
    let client = reqwest::Client::builder().default_headers(header_map).no_proxy().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let api = def.connection.http_api.unwrap();
    let mut covered = HashSet::new();
    for case in fixture["cases"].as_array().unwrap() {
        let op = api.tools.iter().find(|op| case["tool"] == op.name).unwrap();
        covered.insert(&op.name);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri = reqwest::Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let request = op.request(&client, uri, &case["args"]).unwrap();
        let serve = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut first = String::new();
            reader.read_line(&mut first).await.unwrap();
            assert_eq!(first.trim_end(), format!("{} HTTP/1.1", case["wire"].as_str().unwrap()), "{}", op.name);
            let mut headers = std::collections::HashMap::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                if line.trim().is_empty() { break; }
                let (name,value) = line.split_once(':').unwrap();
                headers.insert(name.to_ascii_lowercase(),value.trim().to_owned());
            }
            for (key, value) in &bound { assert_eq!(&headers[key], value); }
            let len = headers.get("content-length").map(|v| v.parse().unwrap()).unwrap_or(0);
            let mut body = vec![0;len];
            reader.read_exact(&mut body).await.unwrap();
            if let Some(binary) = case["bytes"].as_str() {
                for (key, value) in case["headers"].as_object().unwrap() {
                    assert_eq!(headers[key], value.as_str().unwrap());
                }
                assert_eq!(headers["content-type"], "application/octet-stream");
                assert_eq!(body, binary.as_bytes());
            } else if let Some(expected) = case["args"].get("body") {
                assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), *expected);
            } else { assert!(body.is_empty()); }
            let reply = case["reply"].to_string();
            let code = case["status"].as_u64().unwrap_or(200);
            let message = format!("HTTP/1.1 {code} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",reply.len());
            socket.write_all(message.as_bytes()).await.unwrap();
        };
        let invoke = async {
            let response = request.send().await.unwrap();
            read_result(op, response, &redactor).await
        };
        let (_, result) = tokio::join!(serve, invoke);
        if case["status"].as_u64().is_some_and(|status| status >= 400) {
            let error = result.unwrap_err();
            assert!(error.contains(&case["status"].to_string()));
            assert!(!error.contains("mock-secret"));
        } else {
            let result: Value = serde_json::from_str(&result.unwrap()).unwrap();
            let text = result[0]["text"].as_str().unwrap();
            assert_eq!(serde_json::from_str::<Value>(text).unwrap(),case["reply"]);
        }
    }
    assert_eq!(covered.len(), api.tools.len());
}
