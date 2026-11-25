use super::*;

#[test]
fn test_codex_windows_preserve_buckets_and_skip_missing_secondary() {
    let bucket = json!({"primary":{"usedPercent":0,"windowDurationMins":300,"resetsAt":1700000000}});
    let other = json!({"secondary":{"usedPercent":100,"windowDurationMins":10080}});
    let windows = codex_windows(&json!({"rateLimits":bucket,"rateLimitsByLimitId":{"codex":bucket,"review":other}}));
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].label, "codex · 5 小时");
    assert_eq!(windows[0].resets_at.unwrap().timestamp(), 1700000000);
    assert_eq!(windows[1].remaining_percent, 0.0);
    assert!(windows[1].resets_at.is_none());
    assert_eq!(codex_windows(&json!({"rateLimits":bucket}))[0].remaining_percent, 100.0);
    let individual = codex_windows(&json!({"rateLimits":{"individualLimit":{"remainingPercent":27,"resetsAt":0}}}));
    assert_eq!(individual[0].remaining_percent, 27.0);
    assert!(individual[0].resets_at.is_none());
}

#[test]
fn test_go_windows_distinguish_missing_and_zero() {
    let meter = json!({"percent":25,"resetsAt":"2026-12-01T00:00:00Z"});
    let response = json!({"usage":{"rolling":meter,"weekly":{"percent":0},"monthly":meter}});
    let windows = go_windows(&response).unwrap();
    assert_eq!(windows[0].remaining_percent, 75.0);
    assert_eq!(windows[1].remaining_percent, 100.0);
    assert!(go_windows(&json!({"usage":{"rolling":meter}})).is_err());
}

#[test]
fn test_cursor_windows_use_account_percentages_and_epoch_milliseconds() {
    let windows = cursor_windows(&json!({"billingCycleEnd":"1789876386000","planUsage":{"autoPercentUsed":12.5,"totalPercentUsed":100}}));
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[1].remaining_percent, 87.5);
    assert_eq!(windows[1].resets_at.unwrap().timestamp_millis(), 1789876386000);
    assert!(cursor_windows(&json!({"planUsage":{}})).is_empty());
}
