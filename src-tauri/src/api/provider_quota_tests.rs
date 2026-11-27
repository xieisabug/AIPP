use super::*;

#[tokio::test]
async fn test_quota_cache_ttl_manual_refresh_failure_and_account_scope() {
    let db = LLMDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let mut cache = QuotaCache { conn: db.conn, owner: 41, force: false };
    let query = |used| async move { Ok(vec![QuotaWindow::new("每周", used, None)]) };
    let first = cache.read("cursor", "account-a", query(20.0)).await.unwrap();
    let reopen = cache.read("cursor", "account-a", async { panic!("valid cache must avoid query") }).await.unwrap();
    assert_eq!(first.checked_at, reopen.checked_at);
    assert_eq!(reopen.windows[0].remaining_percent, Some(80.0));
    cache.force = true;
    let manual = cache.read("cursor", "account-a", query(30.0)).await.unwrap();
    assert_eq!(manual.windows[0].remaining_percent, Some(70.0));
    assert!(cache.read("cursor", "account-a", async { Err("offline".into()) }).await.is_err());
    cache.force = false;
    let retained = cache.read("cursor", "account-a", async { panic!("failed refresh must preserve snapshot") }).await.unwrap();
    assert_eq!(retained.checked_at, manual.checked_at);
    assert_eq!(retained.windows[0].remaining_percent, Some(70.0));
    cache.conn.execute("UPDATE provider_quota_cache SET checked_at=?1", [Utc::now().timestamp_millis() - 900_000]).unwrap();
    let expired = cache.read("cursor", "account-a", query(40.0)).await.unwrap();
    assert_eq!(expired.windows[0].remaining_percent, Some(60.0));
    let switched = cache.read("cursor", "account-b", query(90.0)).await.unwrap();
    assert_eq!(switched.windows[0].remaining_percent, Some(10.0));
    cache.owner = 42;
    assert!(cache.read("cursor", "account-b", async { Err("no cache for this provider".into()) }).await.is_err());
}

#[test]
fn test_codex_windows_preserve_buckets_and_skip_missing_secondary() {
    let bucket = json!({"primary":{"usedPercent":0,"windowDurationMins":300,"resetsAt":1700000000}});
    let other = json!({"secondary":{"usedPercent":100,"windowDurationMins":10080}});
    let windows = codex_windows(&json!({"rateLimits":bucket,"rateLimitsByLimitId":{"codex":bucket,"review":other}}));
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].label, "codex · 5 小时");
    assert_eq!(windows[0].resets_at.unwrap().timestamp(), 1700000000);
    assert_eq!(windows[1].remaining_percent, Some(0.0));
    assert!(windows[1].resets_at.is_none());
    assert_eq!(codex_windows(&json!({"rateLimits":bucket}))[0].remaining_percent, Some(100.0));
    let individual = codex_windows(&json!({"rateLimits":{"individualLimit":{"remainingPercent":27,"resetsAt":0}}}));
    assert_eq!(individual[0].remaining_percent, Some(27.0));
    assert!(individual[0].resets_at.is_none());
}

#[test]
fn test_go_windows_distinguish_missing_and_zero() {
    let meter = json!({"percent":25,"resetsAt":"2026-12-01T00:00:00Z"});
    let response = json!({"usage":{"rolling":meter,"weekly":{"percent":0},"monthly":meter}});
    let windows = go_windows(&response).unwrap();
    assert_eq!(windows[0].remaining_percent, Some(75.0));
    assert_eq!(windows[1].remaining_percent, Some(100.0));
    assert!(go_windows(&json!({"usage":{"rolling":meter}})).is_err());
}

#[test]
fn test_cursor_windows_use_account_percentages_and_epoch_milliseconds() {
    let windows = cursor_windows(&json!({"billingCycleEnd":"1789876386000","planUsage":{"autoPercentUsed":12.5,"totalPercentUsed":100}}));
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[1].remaining_percent, Some(87.5));
    assert_eq!(windows[1].resets_at.unwrap().timestamp_millis(), 1789876386000);
    assert!(cursor_windows(&json!({"planUsage":{}})).is_empty());
}

#[test]
fn test_antigravity_windows_preserve_groups_remaining_values_and_reset_times() {
    let report = json!({"status":"SUCCESS","num_turns":0,"usage":{"total_tokens":0},
        "command":{"name":"usage","data":{"groups":[
            {"name":"Gemini Models","buckets":[
                {"window":"weekly","remaining_fraction":0.375,"reset_time":"2026-12-01T10:00:00+08:00"},
                {"window":"5h","remaining_fraction":0}]},
            {"name":"Claude and GPT models","buckets":[{"window":"weekly","remaining_amount":12}]}
        ]}}});
    let windows = antigravity::windows(report).unwrap();
    assert_eq!(windows.len(), 3);
    assert_eq!(windows[0].group.as_deref(), Some("Gemini Models"));
    assert_eq!(windows[0].label, "每周");
    assert_eq!(windows[0].remaining_percent, Some(37.5));
    assert_eq!(windows[0].resets_at.unwrap().to_rfc3339(), "2026-12-01T02:00:00+00:00");
    assert_eq!(windows[1].label, "5 小时");
    assert_eq!(windows[1].remaining_percent, Some(0.0));
    assert!(windows[1].resets_at.is_none());
    assert_eq!(windows[2].group.as_deref(), Some("Claude and GPT models"));
    assert_eq!(windows[2].remaining_percent, None);
    assert_eq!(windows[2].remaining_amount, Some(12.0));
}

#[test]
fn test_antigravity_windows_reject_non_command_output_and_surface_cli_errors() {
    let error = antigravity::windows(json!({"status":"ERROR","error":"authentication required"}));
    assert!(error.err().unwrap().contains("authentication required"));
    assert!(antigravity::windows(json!({"status":"SUCCESS","num_turns":1,"response":"quota is full"})).is_err());
    assert!(antigravity::windows(json!({"status":"SUCCESS","num_turns":0,
        "command":{"name":"usage","data":{"groups":[]}}})).is_err());
}

#[test]
fn test_show_quota_in_chat_enabled_respects_provider_type_toggle_and_key() {
    let config = |name: &str, value: &str| LLMProviderConfig {
        id: 1,
        name: name.into(),
        llm_provider_id: 42,
        value: value.into(),
        append_location: String::new(),
        is_addition: false,
    };

    let toggle_on = vec![config(SHOW_QUOTA_IN_CHAT_CONFIG, "true")];
    let toggle_off = vec![config(SHOW_QUOTA_IN_CHAT_CONFIG, "false")];
    let opencode_go_ready = vec![
        config(SHOW_QUOTA_IN_CHAT_CONFIG, "true"),
        config("api_key", "sk-go"),
    ];
    let opencode_go_blank_key = vec![
        config(SHOW_QUOTA_IN_CHAT_CONFIG, "true"),
        config("api_key", "   "),
    ];

    assert!(show_quota_in_chat_enabled("codex_app_server", &toggle_on));
    assert!(show_quota_in_chat_enabled("cursor", &toggle_on));
    assert!(show_quota_in_chat_enabled("antigravity", &toggle_on));
    assert!(!show_quota_in_chat_enabled("codex_app_server", &toggle_off));
    assert!(!show_quota_in_chat_enabled("cursor", &[]));
    assert!(!show_quota_in_chat_enabled("openai_api", &opencode_go_ready));
    assert!(!show_quota_in_chat_enabled("deepseek", &opencode_go_ready));
    assert!(show_quota_in_chat_enabled("opencode_go", &opencode_go_ready));
    assert!(!show_quota_in_chat_enabled("opencode_go", &toggle_on));
    assert!(!show_quota_in_chat_enabled("opencode_go", &opencode_go_blank_key));
}

#[tokio::test]
#[ignore = "requires a locally installed and signed-in official agy CLI"]
async fn test_antigravity_quota_from_local_cli() {
    let windows = antigravity::local_quota(None).await.unwrap();
    assert!(windows.iter().all(|w| w.group.as_ref().is_some_and(|g| !g.is_empty())));
    assert!(windows.iter().any(|w| w.remaining_percent.is_some() || w.remaining_amount.is_some()));
}
