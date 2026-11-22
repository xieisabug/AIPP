use std::collections::HashMap;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

#[test]
fn test_build_endpoint_url_normalizes_official_and_newapi_bases() {
    // TypeSafe Jev
    assert_eq!(
        build_endpoint_url(DecisionProtocol::TypesafeSystemOne, "https://api.typesafe.ai/v1/").unwrap(),
        "https://api.typesafe.ai/v1/systemone"
    );
    assert_eq!(
        build_endpoint_url(DecisionProtocol::TypesafeSystemOne, "https://my-newapi.example.com").unwrap(),
        "https://my-newapi.example.com/v1/systemone"
    );
    assert_eq!(
        build_endpoint_url(DecisionProtocol::TypesafeSystemOne, "https://my-newapi.example.com/v1").unwrap(),
        "https://my-newapi.example.com/v1/systemone"
    );
    assert_eq!(
        build_endpoint_url(
            DecisionProtocol::TypesafeSystemOne,
            "https://my-newapi.example.com/jev/alpha/decisions"
        )
        .unwrap(),
        "https://my-newapi.example.com/jev/alpha/decisions"
    );

    // OpenAI Decisions
    assert_eq!(
        build_endpoint_url(DecisionProtocol::OpenAiDecisions, "https://api.openai.com/v1/").unwrap(),
        "https://api.openai.com/v1/decisions"
    );
    assert_eq!(
        build_endpoint_url(DecisionProtocol::OpenAiDecisions, "https://newapi.local:3000").unwrap(),
        "https://newapi.local:3000/v1/decisions"
    );
    assert_eq!(
        build_endpoint_url(DecisionProtocol::OpenAiDecisions, "https://newapi.local:3000/v1/decisions/").unwrap(),
        "https://newapi.local:3000/v1/decisions"
    );

    // Gemini Decisions
    assert_eq!(
        build_endpoint_url(
            DecisionProtocol::GeminiDecisions,
            "https://generativelanguage.googleapis.com/v1beta/"
        )
        .unwrap(),
        "https://generativelanguage.googleapis.com/v1beta/decisions"
    );
    assert_eq!(
        build_endpoint_url(DecisionProtocol::GeminiDecisions, "https://newapi.local:3000").unwrap(),
        "https://newapi.local:3000/v1beta/decisions"
    );
    assert_eq!(
        build_endpoint_url(DecisionProtocol::GeminiDecisions, "https://newapi.local:3000/v1").unwrap(),
        "https://newapi.local:3000/v1/decisions"
    );
}

#[test]
fn test_encode_and_decode_typesafe_systemone() {
    let req = DecisionRequest::new(
        "jev-latest",
        "rm -rf /tmp/demo",
        vec![
            DecisionQuestion::bool("safe", "Is this safe?"),
            DecisionQuestion::choice(
                "risk",
                "Pick risk category",
                vec![
                    ChoiceOption::new("safe_readonly", "Read-only"),
                    ChoiceOption::new("destructive", "Destructive"),
                ],
            ),
            DecisionQuestion::score(
                "severity",
                "Rate severity",
                vec![
                    ScoreLevel::new("low", "Low risk"),
                    ScoreLevel::new("high", "High risk"),
                ],
            ),
        ],
    );

    let encoded = encode_request(DecisionProtocol::TypesafeSystemOne, &req).unwrap();
    assert_eq!(encoded["model"], "jev-latest");
    assert_eq!(encoded["state"], "rm -rf /tmp/demo");
    assert_eq!(encoded["questions"]["safe"]["type"], "noul");
    assert_eq!(encoded["questions"]["risk"]["type"], "choice");
    assert_eq!(encoded["questions"]["risk"]["criteria"]["destructive"], "Destructive");
    assert_eq!(encoded["questions"]["severity"]["type"], "score");

    let raw_resp = json!({
        "model": "jev-latest.0",
        "answers": {
            "safe": { "type": "noul", "noul": 0.04 },
            "risk": {
                "type": "choice",
                "choice": "destructive",
                "probabilities": { "safe_readonly": 0.03, "destructive": 0.97 },
                "confidence": 0.97
            },
            "severity": {
                "type": "score",
                "score": 0.92,
                "probabilities": [0.08, 0.92]
            }
        },
        "usage": { "input_tokens": 55, "output_tokens": 0 }
    });

    let decoded = decode_response(DecisionProtocol::TypesafeSystemOne, &req, raw_resp, 42).unwrap();
    assert_eq!(decoded.bool_probability("safe"), Some(0.04));
    assert_eq!(decoded.choice("risk"), Some(("destructive", Some(0.97))));
    assert_eq!(decoded.answers["severity"].as_score(), Some(0.92));
    assert_eq!(decoded.usage.input_tokens, Some(55));
    assert_eq!(decoded.usage.output_tokens, 0);
}

#[test]
fn test_encode_and_decode_openai_decisions() {
    let req = DecisionRequest::new(
        "gpt-6-luna",
        "Check git status",
        vec![
            DecisionQuestion::bool("safe", "Is this safe?"),
            DecisionQuestion::choice(
                "risk",
                "Category",
                vec![
                    ChoiceOption::new("safe_readonly", "Read-only"),
                    ChoiceOption::new("destructive", "Destructive"),
                ],
            ),
        ],
    );

    let encoded = encode_request(DecisionProtocol::OpenAiDecisions, &req).unwrap();
    assert_eq!(encoded["model"], "gpt-6-luna");
    assert_eq!(encoded["input"], "Check git status");
    let q_arr = encoded["questions"].as_array().unwrap();
    assert_eq!(q_arr.len(), 2);
    assert_eq!(q_arr[0]["type"], "predicate");
    assert_eq!(q_arr[0]["name"], "safe");
    assert_eq!(q_arr[1]["type"], "choice");
    assert_eq!(q_arr[1]["choices"][0]["value"], "safe_readonly");

    let raw_resp = json!({
        "model": "gpt-6-luna",
        "answers": [
            { "type": "predicate", "name": "safe", "probability": 0.98 },
            { "type": "choice", "name": "risk", "value": "safe_readonly", "probability": 0.96 }
        ],
        "usage": {
            "input_tokens": 80,
            "input_tokens_details": { "cached_tokens": 32 },
            "output_tokens": 0,
            "total_tokens": 80
        }
    });

    let decoded = decode_response(DecisionProtocol::OpenAiDecisions, &req, raw_resp, 30).unwrap();
    assert_eq!(decoded.bool_probability("safe"), Some(0.98));
    assert_eq!(decoded.choice("risk"), Some(("safe_readonly", Some(0.96))));
    assert_eq!(decoded.usage.input_tokens, Some(80));
    assert_eq!(decoded.usage.cached_input_tokens, Some(32));
    assert_eq!(decoded.usage.output_tokens, 0);
}

#[test]
fn test_decision_openai_probability_arrays_by_choice_and_level() {
    let req = DecisionRequest::new("gpt-6-luna", "support ticket", vec![
        DecisionQuestion::choice("department", "Route", vec![
            ChoiceOption::new("billing", "Billing"), ChoiceOption::new("support", "Support"),
        ]),
        DecisionQuestion::score("severity", "Severity", vec![
            ScoreLevel::new("Cosmetic", ""), ScoreLevel::new("Workaround available", ""),
            ScoreLevel::new("Fully blocked", ""),
        ]),
    ]);
    let body = json!({
        "answers": [
            {"type": "choice", "name": "department", "choice": "billing", "confidence": 0.81,
             "probabilities": [{"value": "support", "probability": 0.08}, {"value": "billing", "probability": 0.92}]},
            {"type": "score", "name": "severity", "score": 1.1, "confidence": 0.55, "probabilities": [
                {"value": 2, "label": "Fully blocked", "probability": 0.2},
                {"value": 0, "label": "Cosmetic", "probability": 0.1},
                {"value": 1, "label": "Workaround available", "probability": 0.7}
            ]}
        ]
    });
    let decode = |body| decode_response(DecisionProtocol::OpenAiDecisions, &req, body, 0);
    let response = decode(body.clone()).unwrap();
    assert_eq!(response.choice("department"), Some(("billing", Some(0.92))));
    assert_eq!(response.answers["department"], DecisionAnswer::Choice {
        selected: "billing".into(),
        probabilities: HashMap::from([("support".into(), 0.08), ("billing".into(), 0.92)]),
        confidence: Some(0.81),
    });
    assert_eq!(response.answers["severity"], DecisionAnswer::Score {
        score: 1.1, probabilities: vec![0.1, 0.7, 0.2], confidence: Some(0.55),
    });

    let mut without_distribution = body.clone();
    without_distribution["answers"][0].as_object_mut().unwrap().remove("probabilities");
    assert_eq!(decode(without_distribution).unwrap().choice("department"), Some(("billing", None)));

    let mut invalid = body;
    invalid["answers"][1]["probabilities"][0]["probability"] = json!(1.1);
    assert!(decode(invalid).unwrap_err().contains("severity"));
}

#[test]
fn test_encode_and_decode_gemini_decisions() {
    let req = DecisionRequest::new(
        "gemini-decision-1",
        "Read README.md",
        vec![
            DecisionQuestion::bool_with_criteria("safe", "Safe?", "Read only", "Has side effect"),
            DecisionQuestion::score(
                "urgency",
                "Urgency",
                vec![
                    ScoreLevel::new("low", "Low"),
                    ScoreLevel::new("high", "High"),
                ],
            ),
        ],
    );

    let encoded = encode_request(DecisionProtocol::GeminiDecisions, &req).unwrap();
    assert_eq!(encoded["input"], "Read README.md");
    assert_eq!(encoded["questions"]["safe"]["type"], "noul");
    assert_eq!(encoded["questions"]["safe"]["criteria"]["true"], "Read only");
    assert_eq!(encoded["questions"]["urgency"]["criteria"]["0"], "low: Low");
    assert_eq!(encoded["questions"]["urgency"]["criteria"]["1"], "high: High");

    let raw_resp = json!({
        "answers": {
            "safe": { "type": "noul", "noul": 0.99, "confidence": 0.99 },
            "urgency": {
                "type": "score",
                "score": 0.12,
                "probabilities": { "0": 0.88, "1": 0.12 },
                "confidence": 0.91
            }
        },
        "usage": {
            "total_tokens": 44,
            "total_input_tokens": 44,
            "total_output_tokens": 0
        }
    });

    let decoded = decode_response(DecisionProtocol::GeminiDecisions, &req, raw_resp, 25).unwrap();
    assert_eq!(decoded.bool_probability("safe"), Some(0.99));
    assert_eq!(decoded.answers["urgency"].as_score(), Some(0.12));
    assert_eq!(decoded.usage.input_tokens, Some(44));
    assert_eq!(decoded.usage.output_tokens, 0);
}

#[test]
fn test_decode_rejects_missing_answer_out_of_range_and_unknown_choice() {
    let req = DecisionRequest::new(
        "jev-latest",
        "ctx",
        vec![
            DecisionQuestion::bool("safe", "Safe?"),
            DecisionQuestion::choice(
                "cat",
                "Cat",
                vec![ChoiceOption::new("a", "A"), ChoiceOption::new("b", "B")],
            ),
        ],
    );

    // Missing key
    let missing = json!({ "answers": { "safe": { "type": "noul", "noul": 0.9 } } });
    assert!(decode_response(DecisionProtocol::TypesafeSystemOne, &req, missing, 1).is_err());

    // Out-of-range probability
    let bad_prob = json!({
        "answers": {
            "safe": { "type": "noul", "noul": 1.5 },
            "cat": { "type": "choice", "choice": "a" }
        }
    });
    assert!(decode_response(DecisionProtocol::TypesafeSystemOne, &req, bad_prob, 1).is_err());

    // Hallucinated choice
    let bad_choice = json!({
        "answers": {
            "safe": { "type": "noul", "noul": 0.9 },
            "cat": { "type": "choice", "choice": "hallucinated" }
        }
    });
    assert!(decode_response(DecisionProtocol::TypesafeSystemOne, &req, bad_choice, 1).is_err());
}

async fn spawn_mock_server(
    status_line: &'static str,
    response_body: impl ToString,
) -> (String, tokio::task::JoinHandle<(String, String)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body_str = response_body.to_string();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 16384];
        let n = socket.read(&mut buf).await.unwrap();
        let raw = String::from_utf8_lossy(&buf[..n]).to_string();
        let (head, req_body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
        let reply = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
            body_str.len()
        );
        socket.write_all(reply.as_bytes()).await.unwrap();
        let _ = socket.shutdown().await;
        (head.to_string(), req_body.to_string())
    });
    (format!("http://{}", addr), handle)
}

#[test]
fn test_decision_endpoint_uses_provider_default_and_explicit_gateway() {
    use super::client::decision_endpoint_url;
    use DecisionProtocol::*;
    for (api_type, endpoint, protocol, expected) in [
        ("openai_api", None, OpenAiDecisions, "https://api.openai.com/v1/decisions"),
        ("openai", Some("   "), OpenAiDecisions, "https://api.openai.com/v1/decisions"),
        ("gemini", None, GeminiDecisions, "https://generativelanguage.googleapis.com/v1beta/decisions"),
        ("typesafe_jev", None, TypesafeSystemOne, "https://api.typesafe.ai/v1/systemone"),
        ("typesafe_jev", Some("https://gateway.example/proxy/v1"), TypesafeSystemOne,
            "https://gateway.example/proxy/v1/systemone"),
    ] {
        assert_eq!(decision_endpoint_url(api_type, endpoint, protocol).unwrap(), expected);
    }
    assert!(decision_endpoint_url("openai_api", Some("bad url"), OpenAiDecisions).is_err());
    for (api_type, own_protocol) in [
        ("openai_api", OpenAiDecisions), ("gemini", GeminiDecisions), ("typesafe_jev", TypesafeSystemOne),
    ] {
        for protocol in [OpenAiDecisions, GeminiDecisions, TypesafeSystemOne] {
            if protocol != own_protocol {
                let err = decision_endpoint_url(api_type, Some("https://gateway.example/v1"), protocol).unwrap_err();
                assert!(err.contains(api_type) && err.contains(protocol.as_request_mode()));
            }
        }
    }
}

#[tokio::test]
async fn test_decision_provider_redacts_echo_before_both_error_limits() {
    use crate::db::llm_db::LLMProviderConfig;
    use crate::vault::config_ref::PROVIDER_GUARD;

    let secret = "synthetic-test-credential";
    let row = |name: &str, value: String| LLMProviderConfig {
        id: 0, llm_provider_id: 1, name: name.into(), value,
        append_location: String::new(), is_addition: false,
    };
    for (status, limit, diagnostic) in [
        ("401 Unauthorized", 400, "HTTP 401"),
        ("200 OK", 200, "不是有效 JSON"),
    ] {
        let prefix = "测".repeat(limit - 12);
        let body = format!("{prefix}{secret}{}END", ".".repeat(20));
        let (base, handle) = spawn_mock_server(status, body).await;
        let configs = [
            row("endpoint", base.clone()),
            row("api_key", format!("test-prefix-{secret}-suffix")),
            row(PROVIDER_GUARD, json!({
                "sites": [base], "context": "test", "credentials": ["mock"], "secrets": [secret],
            }).to_string()),
        ];
        let request = DecisionRequest::new("model", "ping", vec![DecisionQuestion::bool("ok", "ok?")]);
        let error = execute_for_provider(
            "typesafe_jev", &configs, MODE_JEV_SYSTEMONE, &request, None, false, Some(5), &HashMap::new(),
        ).await.unwrap_err();
        assert!(error.contains(diagnostic), "{error}");
        assert!(error.ends_with(&format!("{prefix}[REDACTED]..")), "{error}");
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_execute_decision_http_contract_all_three_protocols() {
    // 1. OpenAI Decisions (Bearer auth + /v1/decisions)
    let (base, handle) = spawn_mock_server(
        "200 OK",
        json!({
            "model": "gpt-6-luna",
            "answers": [{ "type": "predicate", "name": "ok", "probability": 0.95 }],
            "usage": { "input_tokens": 12, "output_tokens": 0, "total_tokens": 12 }
        }),
    )
    .await;
    let cfg = DecisionClientConfig {
        protocol: DecisionProtocol::OpenAiDecisions,
        endpoint_url: build_endpoint_url(DecisionProtocol::OpenAiDecisions, &base).unwrap(),
        api_key: "sk-test-oai".into(),
        network_proxy: None,
        timeout_secs: 5,
        custom_headers: HashMap::new(),
    };
    let req = DecisionRequest::new("gpt-6-luna", "ping", vec![DecisionQuestion::bool("ok", "ok?")]);
    let resp = execute_decision(&cfg, &req).await.unwrap();
    assert_eq!(resp.bool_probability("ok"), Some(0.95));
    let (head, _) = handle.await.unwrap();
    assert!(head.starts_with("POST /v1/decisions HTTP/1.1"), "head={head}");
    assert!(head.to_ascii_lowercase().contains("authorization: bearer sk-test-oai"));

    // 2. Gemini Decisions (x-goog-api-key + /v1beta/decisions)
    let (base, handle) = spawn_mock_server(
        "200 OK",
        json!({
            "answers": { "ok": { "type": "noul", "noul": 0.91, "confidence": 0.9 } },
            "usage": { "total_input_tokens": 10, "total_output_tokens": 0, "total_tokens": 10 }
        }),
    )
    .await;
    let cfg = DecisionClientConfig {
        protocol: DecisionProtocol::GeminiDecisions,
        endpoint_url: build_endpoint_url(DecisionProtocol::GeminiDecisions, &base).unwrap(),
        api_key: "gm-test-key".into(),
        network_proxy: None,
        timeout_secs: 5,
        custom_headers: HashMap::new(),
    };
    let req = DecisionRequest::new("gemini-decision-1", "ping", vec![DecisionQuestion::bool("ok", "ok?")]);
    let resp = execute_decision(&cfg, &req).await.unwrap();
    assert_eq!(resp.bool_probability("ok"), Some(0.91));
    let (head, _) = handle.await.unwrap();
    assert!(head.starts_with("POST /v1beta/decisions HTTP/1.1"), "head={head}");
    assert!(head.to_ascii_lowercase().contains("x-goog-api-key: gm-test-key"));

    // 3. HTTP error propagation
    let (base, _handle) = spawn_mock_server("401 Unauthorized", json!({ "error": "bad key" })).await;
    let cfg = DecisionClientConfig {
        protocol: DecisionProtocol::TypesafeSystemOne,
        endpoint_url: build_endpoint_url(DecisionProtocol::TypesafeSystemOne, &base).unwrap(),
        api_key: "ts-bad".into(),
        network_proxy: None,
        timeout_secs: 5,
        custom_headers: HashMap::new(),
    };
    let err = execute_decision(&cfg, &req).await.unwrap_err();
    assert!(err.contains("401"), "err={err}");
}
