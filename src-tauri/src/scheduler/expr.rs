//! 定时任务的时间表达：一次性时间、固定间隔、5 段 cron，统一用一个字符串表示。
//!
//! 任务只在创建它的设备上运行，时间按本机时区解释（带偏移的 ISO 时间按其偏移换算）。
//! 核心计算对时区泛型，测试用固定偏移，运行时用 [`chrono::Local`]。
use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike,
    Utc, Weekday,
};

/// 计算下一次时间时最多向后查找的天数（覆盖 2 月 29 日这类四年一遇的 cron）。
const CRON_SEARCH_DAYS: i64 = 366 * 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    Once(DateTime<Utc>),
    Every(Every),
    Cron(Cron),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EveryUnit {
    Minute,
    Hour,
    Day,
    Week,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Every {
    pub value: u32,
    pub unit: EveryUnit,
    /// 按天/周间隔时的固定时刻（时, 分）。
    pub at: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    source: String,
    minutes: u64,
    hours: u32,
    days: u32,
    months: u16,
    weekdays: u8,
    days_any: bool,
    weekdays_any: bool,
}

pub fn usage_hint() -> &'static str {
    "支持三种写法：一次性用 ISO 时间（如 2026-11-13T16:00+08:00 或 2026-11-13 16:00）或相对时间（如 in 30m）；\
固定间隔用 every（如 every 30m、every 2h、every 1d at 09:00、every 1w at 16:00）；\
周期用 5 段 cron（分 时 日 月 周，按本机时区；如每周四 16:00 写 0 16 * * 4，工作日 9 点写 0 9 * * 1-5，每月 1 号 9 点写 0 9 1 * *）。"
}

fn unrecognized(input: &str) -> String {
    format!("无法识别的时间「{input}」。{}", usage_hint())
}

/// 解析时间表达。相对时间（`in 30m`）按 `now` 换算成绝对的一次性时间。
pub fn parse(input: &str, now: DateTime<Utc>) -> Result<Schedule, String> {
    parse_in(input, now, &Local)
}

pub fn parse_in<Tz: TimeZone>(input: &str, now: DateTime<Utc>, tz: &Tz) -> Result<Schedule, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err(format!("时间不能为空。{}", usage_hint()));
    }
    let lower = text.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("in ").or_else(|| lower.strip_prefix('+')) {
        let duration = parse_duration(rest.trim()).ok_or_else(|| unrecognized(text))?;
        return Ok(Schedule::Once(now + duration));
    }
    if let Some(rest) = lower.strip_prefix("every") {
        return parse_every(rest.trim(), text).map(Schedule::Every);
    }
    if let Some(cron) = cron_alias(&lower) {
        return Cron::parse(cron).map(Schedule::Cron);
    }
    if let Some(at) = parse_datetime_in(text, tz) {
        return Ok(Schedule::Once(at));
    }
    if text.split_whitespace().count() == 5 {
        return Cron::parse(text).map(Schedule::Cron);
    }
    Err(unrecognized(text))
}

fn cron_alias(lower: &str) -> Option<&'static str> {
    match lower {
        "@hourly" => Some("0 * * * *"),
        "@daily" | "@midnight" => Some("0 0 * * *"),
        "@weekly" => Some("0 0 * * 0"),
        "@monthly" => Some("0 0 1 * *"),
        _ => None,
    }
}

fn parse_datetime_in<Tz: TimeZone>(text: &str, tz: &Tz) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
        return Some(dt.with_timezone(&Utc));
    }
    // RFC3339 要求秒；模型常省略秒，例如 2026-11-13T16:00+08:00。
    for fmt in ["%Y-%m-%dT%H:%M%:z", "%Y-%m-%d %H:%M%:z", "%Y-%m-%d %H:%M:%S%:z"] {
        if let Ok(dt) = DateTime::parse_from_str(text, fmt) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, fmt) {
            return local_to_utc(tz, naive);
        }
    }
    None
}

fn local_to_utc<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    tz.from_local_datetime(&naive).earliest().map(|dt| dt.with_timezone(&Utc))
}

fn split_number(text: &str) -> (Option<u32>, &str) {
    let digits = text.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return (None, text);
    }
    (text[..digits].parse().ok(), text[digits..].trim())
}

fn parse_unit(unit: &str) -> Option<EveryUnit> {
    match unit {
        "m" | "min" | "mins" | "minute" | "minutes" | "分" | "分钟" => Some(EveryUnit::Minute),
        "h" | "hr" | "hrs" | "hour" | "hours" | "小时" => Some(EveryUnit::Hour),
        "d" | "day" | "days" | "天" => Some(EveryUnit::Day),
        "w" | "wk" | "week" | "weeks" | "周" | "星期" => Some(EveryUnit::Week),
        _ => None,
    }
}

impl EveryUnit {
    fn duration(self, value: u32) -> Duration {
        let value = i64::from(value);
        match self {
            Self::Minute => Duration::minutes(value),
            Self::Hour => Duration::hours(value),
            Self::Day => Duration::days(value),
            Self::Week => Duration::weeks(value),
        }
    }

    fn short(self) -> &'static str {
        match self {
            Self::Minute => "m",
            Self::Hour => "h",
            Self::Day => "d",
            Self::Week => "w",
        }
    }
}

fn parse_duration(text: &str) -> Option<Duration> {
    let (value, unit) = split_number(text);
    let value = value.filter(|v| *v > 0)?;
    Some(parse_unit(unit)?.duration(value))
}

fn parse_hhmm(text: &str) -> Option<(u32, u32)> {
    let time = NaiveTime::parse_from_str(text.trim(), "%H:%M").ok()?;
    Some((time.hour(), time.minute()))
}

fn parse_every(rest: &str, original: &str) -> Result<Every, String> {
    let (body, at) = match rest.split_once(" at ") {
        Some((body, at)) => {
            let at = parse_hhmm(at)
                .ok_or_else(|| format!("「{original}」里的时刻应为 HH:MM，例如 every 1d at 09:00"))?;
            (body.trim(), Some(at))
        }
        None => (rest, None),
    };
    let (value, unit_text) = split_number(body);
    let unit = parse_unit(unit_text).ok_or_else(|| unrecognized(original))?;
    let value = value.unwrap_or(1);
    if value == 0 {
        return Err(format!("「{original}」的间隔必须大于 0"));
    }
    if at.is_some() && matches!(unit, EveryUnit::Minute | EveryUnit::Hour) {
        return Err(format!("「{original}」：按分钟/小时的间隔不能指定时刻；需要固定时刻请用 every 1d at HH:MM 或 cron"));
    }
    Ok(Every { value, unit, at })
}

fn weekday_label(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "周一",
        Weekday::Tue => "周二",
        Weekday::Wed => "周三",
        Weekday::Thu => "周四",
        Weekday::Fri => "周五",
        Weekday::Sat => "周六",
        Weekday::Sun => "周日",
    }
}

fn weekday_from_sunday(day: u32) -> Weekday {
    match day {
        0 => Weekday::Sun,
        1 => Weekday::Mon,
        2 => Weekday::Tue,
        3 => Weekday::Wed,
        4 => Weekday::Thu,
        5 => Weekday::Fri,
        _ => Weekday::Sat,
    }
}

const WEEKDAY_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const MONTH_NAMES: [&str; 12] =
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn parse_cron_value(text: &str, min: u32, names: Option<&[&str]>) -> Option<u32> {
    if let Ok(value) = text.parse::<u32>() {
        return Some(value);
    }
    let lower = text.to_ascii_lowercase();
    names?.iter().position(|name| *name == lower).map(|index| index as u32 + min)
}

/// 解析一个 cron 字段，返回位掩码以及是否为不限（`*` / `?`）。
fn parse_cron_field(
    field: &str,
    label: &str,
    min: u32,
    max: u32,
    names: Option<&[&str]>,
) -> Result<(u64, bool), String> {
    let invalid = || format!("cron 的{label}字段「{field}」无效，取值范围 {min}-{max}");
    let mut mask = 0u64;
    let any = field == "*" || field == "?";
    for part in field.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => (range, Some(step.parse::<u32>().ok().filter(|s| *s > 0).ok_or_else(invalid)?)),
            None => (part, None),
        };
        let (start, end) = if range == "*" || range == "?" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (parse_cron_value(a, min, names).ok_or_else(invalid)?, parse_cron_value(b, min, names).ok_or_else(invalid)?)
        } else {
            let value = parse_cron_value(range, min, names).ok_or_else(invalid)?;
            (value, if step.is_some() { max } else { value })
        };
        if start < min || end > max || start > end {
            return Err(invalid());
        }
        let step = step.unwrap_or(1);
        let mut value = start;
        while value <= end {
            mask |= 1u64 << value;
            value += step;
        }
    }
    if mask == 0 {
        return Err(invalid());
    }
    Ok((mask, any))
}

impl Cron {
    pub fn parse(text: &str) -> Result<Self, String> {
        let fields: Vec<&str> = text.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!("cron 需要 5 段（分 时 日 月 周），收到「{text}」。{}", usage_hint()));
        }
        let (minutes, _) = parse_cron_field(fields[0], "分钟", 0, 59, None)?;
        let (hours, _) = parse_cron_field(fields[1], "小时", 0, 23, None)?;
        let (days, days_any) = parse_cron_field(fields[2], "日", 1, 31, None)?;
        let (months, _) = parse_cron_field(fields[3], "月", 1, 12, Some(&MONTH_NAMES))?;
        let (mut weekdays, weekdays_any) = parse_cron_field(fields[4], "周", 0, 7, Some(&WEEKDAY_NAMES))?;
        if weekdays & (1 << 7) != 0 {
            weekdays = (weekdays | 1) & 0x7f;
        }
        Ok(Self {
            source: fields.join(" "),
            minutes,
            hours: hours as u32,
            days: days as u32,
            months: months as u16,
            weekdays: weekdays as u8,
            days_any,
            weekdays_any,
        })
    }

    fn day_matches(&self, date: NaiveDate) -> bool {
        let day = self.days & (1 << date.day()) != 0;
        let weekday = self.weekdays & (1 << date.weekday().num_days_from_sunday()) != 0;
        // Vixie cron：日与周都有限定时，满足其一即可。
        match (self.days_any, self.weekdays_any) {
            (true, true) => true,
            (true, false) => weekday,
            (false, true) => day,
            (false, false) => day || weekday,
        }
    }

    fn next_after_in<Tz: TimeZone>(&self, after: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Utc>> {
        let start = after.with_timezone(tz).date_naive();
        for offset in 0..CRON_SEARCH_DAYS {
            let date = start + Duration::days(offset);
            if self.months & (1 << date.month()) == 0 || !self.day_matches(date) {
                continue;
            }
            for hour in (0..24).filter(|h| self.hours & (1 << h) != 0) {
                for minute in (0..60).filter(|m| self.minutes & (1u64 << m) != 0) {
                    let naive = date.and_hms_opt(hour, minute, 0)?;
                    // 夏令时跳过的时刻不存在，跳过而不是挪到别的时刻。
                    let Some(at) = local_to_utc(tz, naive) else { continue };
                    if at > after {
                        return Some(at);
                    }
                }
            }
        }
        None
    }

    fn single(mask: u64) -> Option<u32> {
        (mask.count_ones() == 1).then(|| mask.trailing_zeros())
    }

    fn describe(&self) -> String {
        let fields: Vec<&str> = self.source.split(' ').collect();
        let all_months = self.months == 0x1ffe;
        if let (Some(minute), Some(hour)) = (Self::single(self.minutes), Self::single(u64::from(self.hours))) {
            let time = format!("{hour:02}:{minute:02}");
            if all_months && self.days_any && self.weekdays_any {
                return format!("每天 {time}");
            }
            if all_months && self.days_any && !self.weekdays_any {
                return match self.weekdays {
                    0b0111110 => format!("工作日（周一至周五）{time}"),
                    0b1000001 => format!("周末 {time}"),
                    mask => {
                        let days: Vec<&str> = [1u32, 2, 3, 4, 5, 6, 0]
                            .iter()
                            .filter(|d| mask & (1 << **d) != 0)
                            .map(|d| weekday_label(weekday_from_sunday(*d)))
                            .collect();
                        format!("每{} {time}", days.join("、"))
                    }
                };
            }
            if all_months && !self.days_any && self.weekdays_any {
                let days: Vec<String> = (1..=31).filter(|d| self.days & (1 << d) != 0).map(|d| d.to_string()).collect();
                return format!("每月 {} 日 {time}", days.join("、"));
            }
        }
        if all_months && self.days_any && self.weekdays_any {
            if let Some(step) = fields[0].strip_prefix("*/").and_then(|s| s.parse::<u32>().ok()) {
                if fields[1] == "*" {
                    return format!("每 {step} 分钟");
                }
            }
            if let Some(minute) = Self::single(self.minutes) {
                if fields[1] == "*" {
                    return format!("每小时第 {minute} 分");
                }
                if let Some(step) = fields[1].strip_prefix("*/").and_then(|s| s.parse::<u32>().ok()) {
                    return format!("每 {step} 小时（第 {minute} 分）");
                }
            }
        }
        format!("按 cron「{}」", self.source)
    }
}

impl Every {
    fn next_after_in<Tz: TimeZone>(&self, after: DateTime<Utc>, anchor: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Utc>> {
        let Some((hour, minute)) = self.at else {
            let step = self.unit.duration(self.value);
            let elapsed = after - anchor;
            if elapsed < Duration::zero() {
                return Some(anchor + step);
            }
            let periods = elapsed.num_seconds() / step.num_seconds().max(1) + 1;
            return Some(anchor + step * periods as i32);
        };
        let period_days = i64::from(self.value) * if self.unit == EveryUnit::Week { 7 } else { 1 };
        let anchor_date = anchor.with_timezone(tz).date_naive();
        let after_date = after.with_timezone(tz).date_naive();
        let mut k = ((after_date - anchor_date).num_days().max(0) + period_days - 1) / period_days;
        for _ in 0..3 {
            let date = anchor_date + Duration::days(k * period_days);
            let naive = date.and_hms_opt(hour, minute, 0)?;
            let at = local_to_utc(tz, naive).or_else(|| local_to_utc(tz, naive + Duration::hours(1)))?;
            if at > after {
                return Some(at);
            }
            k += 1;
        }
        None
    }

    fn describe_in<Tz: TimeZone>(&self, anchor: DateTime<Utc>, tz: &Tz) -> String {
        let n = self.value;
        let time = self.at.map(|(h, m)| format!("{h:02}:{m:02}"));
        match (self.unit, time) {
            (EveryUnit::Minute, _) => format!("每 {n} 分钟"),
            (EveryUnit::Hour, _) => format!("每 {n} 小时"),
            (EveryUnit::Day, Some(time)) if n == 1 => format!("每天 {time}"),
            (EveryUnit::Day, Some(time)) => format!("每 {n} 天 {time}"),
            (EveryUnit::Day, None) => format!("每 {n} 天"),
            (EveryUnit::Week, at) => {
                let weekday = weekday_label(anchor.with_timezone(tz).weekday());
                let time = at.map(|t| format!(" {t}")).unwrap_or_default();
                if n == 1 { format!("每{weekday}{time}") } else { format!("每 {n} 周的{weekday}{time}") }
            }
        }
    }
}

impl Schedule {
    /// 规范化后的表达，保存到任务里；一次性时间保存为带本机偏移的绝对时间。
    pub fn to_expr(&self) -> String {
        self.to_expr_in(&Local)
    }

    pub fn to_expr_in<Tz: TimeZone>(&self, tz: &Tz) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
        match self {
            Self::Once(at) => at.with_timezone(tz).format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            Self::Every(every) => {
                let base = format!("every {}{}", every.value, every.unit.short());
                match every.at {
                    Some((h, m)) => format!("{base} at {h:02}:{m:02}"),
                    None => base,
                }
            }
            Self::Cron(cron) => cron.source.clone(),
        }
    }

    /// 严格晚于 `after` 的下一次时间。`anchor` 是间隔类计划的起点（通常是创建时间）。
    pub fn next_after(&self, after: DateTime<Utc>, anchor: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.next_after_in(after, anchor, &Local)
    }

    pub fn next_after_in<Tz: TimeZone>(&self, after: DateTime<Utc>, anchor: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Utc>> {
        match self {
            Self::Once(at) => (*at > after).then_some(*at),
            Self::Every(every) => every.next_after_in(after, anchor, tz),
            Self::Cron(cron) => cron.next_after_in(after, tz),
        }
    }

    pub fn upcoming(&self, after: DateTime<Utc>, anchor: DateTime<Utc>, count: usize) -> Vec<DateTime<Utc>> {
        self.upcoming_in(after, anchor, count, &Local)
    }

    pub fn upcoming_in<Tz: TimeZone>(&self, after: DateTime<Utc>, anchor: DateTime<Utc>, count: usize, tz: &Tz) -> Vec<DateTime<Utc>> {
        let mut times = Vec::with_capacity(count);
        let mut cursor = after;
        while times.len() < count {
            let Some(next) = self.next_after_in(cursor, anchor, tz) else { break };
            times.push(next);
            cursor = next;
        }
        times
    }

    /// 给人看的描述，例如「每周四 16:00」「工作日（周一至周五）09:00」。
    pub fn describe(&self, anchor: DateTime<Utc>) -> String {
        self.describe_in(anchor, &Local)
    }

    pub fn describe_in<Tz: TimeZone>(&self, anchor: DateTime<Utc>, tz: &Tz) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
        match self {
            Self::Once(at) => format!("一次：{}", format_local_in(*at, tz)),
            Self::Every(every) => every.describe_in(anchor, tz),
            Self::Cron(cron) => cron.describe(),
        }
    }
}

/// 本机时间的展示格式，例如「2026-11-19 16:00 周四」。
pub fn format_local(at: DateTime<Utc>) -> String {
    format_local_in(at, &Local)
}

pub fn format_local_in<Tz: TimeZone>(at: DateTime<Utc>, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let local = at.with_timezone(tz);
    format!("{} {}", local.format("%Y-%m-%d %H:%M"), weekday_label(local.weekday()))
}

#[cfg(test)]
#[path = "expr_tests.rs"]
mod tests;
