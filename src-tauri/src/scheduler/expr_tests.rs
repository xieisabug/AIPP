use super::*;
use chrono::FixedOffset;

fn tz() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}

/// 本机 +08:00 的时间转 UTC。
fn at(text: &str) -> DateTime<Utc> {
    let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap();
    tz().from_local_datetime(&naive).unwrap().with_timezone(&Utc)
}

fn parse8(text: &str, now: &str) -> Schedule {
    parse_in(text, at(now), &tz()).unwrap()
}

fn next8(schedule: &Schedule, after: &str, anchor: &str) -> Option<String> {
    schedule.next_after_in(at(after), at(anchor), &tz()).map(|t| format_local_in(t, &tz()))
}

#[test]
fn test_parse_once_accepts_iso_variants_and_relative() {
    let now = "2026-11-12 10:00";
    let expected = Schedule::Once(at("2026-11-13 16:00"));
    for text in ["2026-11-13T16:00:00+08:00", "2026-11-13T16:00+08:00", "2026-11-13 16:00", "2026-11-13T16:00"] {
        assert_eq!(parse8(text, now), expected, "{text}");
    }
    assert_eq!(parse8("in 30m", now), Schedule::Once(at("2026-11-12 10:30")));
    assert_eq!(parse8("+2h", now), Schedule::Once(at("2026-11-12 12:00")));
    assert_eq!(parse8("in 1 day", now), Schedule::Once(at("2026-11-13 10:00")));
}

#[test]
fn test_parse_every_and_round_trip_expr() {
    let now = "2026-11-12 10:00";
    for (text, expr) in [
        ("every 30m", "every 30m"),
        ("every 2 hours", "every 2h"),
        ("every day at 9:05", "every 1d at 09:05"),
        ("Every 3d at 22:00", "every 3d at 22:00"),
        ("every week at 16:00", "every 1w at 16:00"),
    ] {
        let schedule = parse8(text, now);
        assert_eq!(schedule.to_expr_in(&tz()), expr, "{text}");
        assert_eq!(parse8(expr, now), schedule, "re-parse {expr}");
    }
}

#[test]
fn test_parse_rejects_invalid_with_usage_hint() {
    let now = at("2026-11-12 10:00");
    for text in ["", "每周四16点", "every 0m", "every 2h at 09:00", "0 16 * *", "61 * * * *", "0 9 32 * *", "in 5 parsecs"] {
        let error = parse_in(text, now, &tz()).unwrap_err();
        assert!(!error.is_empty(), "{text}");
    }
    let error = parse_in("每周四16点", now, &tz()).unwrap_err();
    assert!(error.contains("0 16 * * 4"), "错误信息应给出可照抄的 cron 示例: {error}");
}

#[test]
fn test_cron_weekly_thursday() {
    let schedule = parse8("0 16 * * 4", "2026-11-12 10:00");
    // 2026-11-12 是周四：当天 16:00 尚未到
    assert_eq!(next8(&schedule, "2026-11-12 10:00", "2026-11-12 10:00").unwrap(), "2026-11-12 16:00 周四");
    assert_eq!(next8(&schedule, "2026-11-12 16:00", "2026-11-12 10:00").unwrap(), "2026-11-19 16:00 周四");
    assert_eq!(schedule.describe_in(at("2026-11-12 10:00"), &tz()), "每周四 16:00");
}

#[test]
fn test_cron_weekdays_names_and_sunday_seven() {
    let schedule = parse8("30 9 * * mon-fri", "2026-11-12 10:00");
    assert_eq!(next8(&schedule, "2026-11-13 10:00", "2026-11-12 10:00").unwrap(), "2026-11-16 09:30 周一");
    assert_eq!(schedule.describe_in(at("2026-11-12 10:00"), &tz()), "工作日（周一至周五）09:30");
    let sunday = parse8("0 9 * * 7", "2026-11-12 10:00");
    assert_eq!(next8(&sunday, "2026-11-12 10:00", "2026-11-12 10:00").unwrap(), "2026-11-15 09:00 周日");
    let multi = parse8("0 21 * * 2,6", "2026-11-12 10:00");
    assert_eq!(multi.describe_in(at("2026-11-12 10:00"), &tz()), "每周二、周六 21:00");
}

#[test]
fn test_cron_day_and_weekday_use_or_semantics() {
    // 每月 1 号或每周一
    let schedule = parse8("0 9 1 * 1", "2026-11-12 10:00");
    assert_eq!(next8(&schedule, "2026-11-12 10:00", "2026-11-12 10:00").unwrap(), "2026-11-16 09:00 周一");
    assert_eq!(next8(&schedule, "2026-11-30 10:00", "2026-11-12 10:00").unwrap(), "2026-12-01 09:00 周二");
}

#[test]
fn test_cron_leap_day_and_steps() {
    let leap = parse8("0 0 29 2 *", "2026-11-12 10:00");
    assert_eq!(next8(&leap, "2026-11-12 10:00", "2026-11-12 10:00").unwrap(), "2028-02-29 00:00 周二");
    let every15 = parse8("*/15 * * * *", "2026-11-12 10:00");
    assert_eq!(next8(&every15, "2026-11-12 10:07", "2026-11-12 10:00").unwrap(), "2026-11-12 10:15 周四");
    assert_eq!(every15.describe_in(at("2026-11-12 10:00"), &tz()), "每 15 分钟");
    let monthly = parse8("0 9 1,15 * *", "2026-11-12 10:00");
    assert_eq!(monthly.describe_in(at("2026-11-12 10:00"), &tz()), "每月 1、15 日 09:00");
}

#[test]
fn test_every_interval_is_anchored_and_strictly_after() {
    let schedule = parse8("every 30m", "2026-11-12 10:00");
    assert_eq!(next8(&schedule, "2026-11-12 10:00", "2026-11-12 10:00").unwrap(), "2026-11-12 10:30 周四");
    assert_eq!(next8(&schedule, "2026-11-12 10:30", "2026-11-12 10:00").unwrap(), "2026-11-12 11:00 周四");
    // 长时间未运行后，下一次仍落在锚点的整倍数上
    assert_eq!(next8(&schedule, "2026-11-12 13:47", "2026-11-12 10:00").unwrap(), "2026-11-12 14:00 周四");
}

#[test]
fn test_every_days_at_time_follows_anchor_date() {
    let daily = parse8("every 1d at 09:00", "2026-11-12 10:00");
    assert_eq!(next8(&daily, "2026-11-12 10:00", "2026-11-12 10:00").unwrap(), "2026-11-13 09:00 周五");
    assert_eq!(next8(&daily, "2026-11-12 08:00", "2026-11-12 08:00").unwrap(), "2026-11-12 09:00 周四");
    let every3 = parse8("every 3d at 22:00", "2026-11-12 10:00");
    assert_eq!(next8(&every3, "2026-11-12 23:00", "2026-11-12 10:00").unwrap(), "2026-11-15 22:00 周日");
    let biweekly = parse8("every 2w at 16:00", "2026-11-12 10:00");
    assert_eq!(next8(&biweekly, "2026-11-12 17:00", "2026-11-12 10:00").unwrap(), "2026-11-26 16:00 周四");
    assert_eq!(biweekly.describe_in(at("2026-11-12 10:00"), &tz()), "每 2 周的周四 16:00");
}

#[test]
fn test_once_has_no_next_after_it_passed_and_upcoming_counts() {
    let once = parse8("2026-11-13 16:00", "2026-11-12 10:00");
    assert!(next8(&once, "2026-11-13 16:00", "2026-11-12 10:00").is_none());
    assert_eq!(once.upcoming_in(at("2026-11-12 10:00"), at("2026-11-12 10:00"), 3, &tz()).len(), 1);
    let weekly = parse8("0 16 * * 4", "2026-11-12 10:00");
    let upcoming: Vec<String> = weekly
        .upcoming_in(at("2026-11-12 10:00"), at("2026-11-12 10:00"), 3, &tz())
        .into_iter()
        .map(|t| format_local_in(t, &tz()))
        .collect();
    assert_eq!(upcoming, ["2026-11-12 16:00 周四", "2026-11-19 16:00 周四", "2026-11-26 16:00 周四"]);
}
