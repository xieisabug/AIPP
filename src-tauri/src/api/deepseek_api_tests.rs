use super::*;
use serde_json::json;

#[test]
fn test_balance_url_uses_provider_origin() {
    assert_eq!(balance_url("https://api.deepseek.com/").unwrap().as_str(), "https://api.deepseek.com/user/balance");
    assert_eq!(balance_url("https://api.deepseek.com/v1/").unwrap().as_str(), "https://api.deepseek.com/user/balance");
    assert_eq!(balance_url("https://relay.example:8443/deepseek/v1/").unwrap().as_str(), "https://relay.example:8443/user/balance");
    assert!(balance_url("not a url").is_err());
}

#[test]
fn test_parse_balance_prefers_cny_and_keeps_string_precision() {
    let value = json!({"is_available": true, "balance_infos": [
        {"currency": "USD", "total_balance": "1.50", "granted_balance": "0.00", "topped_up_balance": "1.50"},
        {"currency": "CNY", "total_balance": "19.00", "granted_balance": "0.00", "topped_up_balance": "19.00"},
    ]});
    assert_eq!(parse_balance(&value).unwrap(), DeepSeekBalance { available: true, currency: "CNY".into(), total: "19.00".into() });
    let usd = json!({"is_available": false, "balance_infos": [{"currency": "USD", "total_balance": "0.00"}]});
    assert_eq!(parse_balance(&usd).unwrap(), DeepSeekBalance { available: false, currency: "USD".into(), total: "0.00".into() });
}

#[test]
fn test_parse_balance_rejects_malformed_responses() {
    assert!(parse_balance(&json!({"balance_infos": []})).is_err());
    assert!(parse_balance(&json!({"is_available": true})).is_err());
    assert!(parse_balance(&json!({"is_available": true, "balance_infos": [{"currency": "CNY"}]})).is_err());
    // An empty list is a real (zero) balance, not an error.
    assert_eq!(parse_balance(&json!({"is_available": false, "balance_infos": []})).unwrap().total, "0.00");
}

#[test]
fn test_status_errors_never_include_the_response_body() {
    for code in [401u16, 402, 403, 404, 429, 500] {
        let text = status_error(reqwest::StatusCode::from_u16(code).unwrap());
        assert!(!text.contains('*'), "{text}");
    }
    assert_eq!(status_error(reqwest::StatusCode::UNAUTHORIZED), "API Key 无效或已吊销");
}

fn provider_config(name: &str, value: &str) -> LLMProviderConfig {
    LLMProviderConfig {
        id: 1,
        name: name.into(),
        llm_provider_id: 42,
        value: value.into(),
        append_location: String::new(),
        is_addition: false,
    }
}

#[test]
fn test_show_balance_in_chat_requires_deepseek_switch_and_key() {
    let enabled_configs = vec![
        provider_config("api_key", "vault://3f2a9c1e-0b7d-4e5f-9a12-6c8d0e1f2a3b"),
        provider_config(SHOW_BALANCE_IN_CHAT_CONFIG, "true"),
    ];
    assert!(show_balance_in_chat_enabled("deepseek", &enabled_configs));
    assert!(!show_balance_in_chat_enabled("openai_api", &enabled_configs));

    let disabled_configs = vec![
        provider_config("api_key", "sk-test"),
        provider_config(SHOW_BALANCE_IN_CHAT_CONFIG, "false"),
    ];
    assert!(!show_balance_in_chat_enabled("deepseek", &disabled_configs));

    let missing_switch = vec![provider_config("api_key", "sk-test")];
    assert!(!show_balance_in_chat_enabled("deepseek", &missing_switch));

    let blank_key = vec![
        provider_config("api_key", "   "),
        provider_config(SHOW_BALANCE_IN_CHAT_CONFIG, "true"),
    ];
    assert!(!show_balance_in_chat_enabled("deepseek", &blank_key));
}
