use super::*;
use chrono::Duration;
use rusqlite::Connection;

fn task(now: DateTime<Utc>, due: DateTime<Utc>, schedule_type: &str) -> ScheduledTask {
    ScheduledTask {
        id: 42, name: "test".into(), is_enabled: true, schedule_type: schedule_type.into(),
        interval_value: Some(1), interval_unit: Some("hour".into()), start_time: None,
        week_days: None, month_days: None, run_at: Some(due), next_run_at: Some(due),
        last_run_at: None, assistant_id: 1, task_prompt: String::new(), notify_prompt: String::new(),
        butler_conversation_id: None, created_time: now, updated_time: now,
    }
}

#[test]
fn test_mobile_missed_occurrence_only_before_first_scan() {
    let now = Utc::now();
    assert!(is_missed_on_startup(true, now - Duration::seconds(1), now));
    assert!(!is_missed_on_startup(true, now, now));
    assert!(!is_missed_on_startup(false, now - Duration::seconds(1), now));
}

#[test]
fn test_mobile_missed_record_is_atomic_and_does_not_claim_success() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE scheduled_task (id INTEGER PRIMARY KEY, is_enabled INTEGER, next_run_at TEXT, last_run_at TEXT, updated_time TEXT);
        CREATE TABLE scheduled_task_run (task_id INTEGER, run_id TEXT UNIQUE, status TEXT, notify INTEGER, summary TEXT, error_message TEXT, started_time TEXT, finished_time TEXT, butler_followup_status TEXT);").unwrap();
    let now = Utc::now();
    let due = now - Duration::hours(3);
    let next = now + Duration::hours(1);
    let task = task(now, due, "interval");
    conn.execute("INSERT INTO scheduled_task VALUES (?1, 1, ?2, NULL, ?3)", rusqlite::params![task.id, due, now]).unwrap();
    record_missed_in_connection(&mut conn, &task, due, Some(next), now).unwrap();
    record_missed_in_connection(&mut conn, &task, due, Some(next), now).unwrap();
    let (count, status): (i64, String) = conn.query_row("SELECT count(*), status FROM scheduled_task_run", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!((count, status.as_str()), (1, "missed"));
    let (enabled, last): (bool, Option<String>) = conn.query_row("SELECT is_enabled, last_run_at FROM scheduled_task", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert!(enabled);
    assert!(last.is_none());
}

#[test]
fn test_mobile_missed_record_rolls_back_schedule_on_insert_failure() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE scheduled_task (id INTEGER PRIMARY KEY, is_enabled INTEGER, next_run_at TEXT, updated_time TEXT);
        CREATE TABLE scheduled_task_run (task_id INTEGER, run_id TEXT UNIQUE, status TEXT, notify INTEGER, summary TEXT, error_message TEXT, started_time TEXT, finished_time TEXT, butler_followup_status TEXT);").unwrap();
    let now = Utc::now();
    let due = now - Duration::minutes(1);
    let task = task(now, due, "once");
    conn.execute("INSERT INTO scheduled_task VALUES (?1, 1, ?2, ?3)", rusqlite::params![task.id, due, now]).unwrap();
    conn.execute("INSERT INTO scheduled_task_run (run_id) VALUES (?1)", [format!("missed:{}:{}", task.id, due.to_rfc3339())]).unwrap();
    assert!(record_missed_in_connection(&mut conn, &task, due, None, now).is_err());
    let enabled: bool = conn.query_row("SELECT is_enabled FROM scheduled_task WHERE id=42", [], |row| row.get(0)).unwrap();
    assert!(enabled);
}

#[test]
fn test_mobile_missed_one_shot_disables_without_recording_execution() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE scheduled_task (id INTEGER PRIMARY KEY, is_enabled INTEGER, next_run_at TEXT, last_run_at TEXT, updated_time TEXT);
        CREATE TABLE scheduled_task_run (task_id INTEGER, run_id TEXT UNIQUE, status TEXT, notify INTEGER, summary TEXT, error_message TEXT, started_time TEXT, finished_time TEXT, butler_followup_status TEXT);").unwrap();
    let now = Utc::now();
    let due = now - Duration::minutes(1);
    let task = task(now, due, "once");
    conn.execute("INSERT INTO scheduled_task VALUES (?1, 1, ?2, NULL, ?3)", rusqlite::params![task.id, due, now]).unwrap();
    record_missed_in_connection(&mut conn, &task, due, None, now).unwrap();
    let (enabled, next, last): (bool, Option<String>, Option<String>) = conn.query_row(
        "SELECT is_enabled, next_run_at, last_run_at FROM scheduled_task", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert!(!enabled);
    assert!(next.is_none() && last.is_none());
    let status: String = conn.query_row("SELECT status FROM scheduled_task_run", [], |row| row.get(0)).unwrap();
    assert_eq!(status, "missed");
}

#[test]
fn test_mobile_missed_accepts_equivalent_rfc3339_due_time() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE scheduled_task (id INTEGER PRIMARY KEY, is_enabled INTEGER, next_run_at TEXT, updated_time TEXT);
        CREATE TABLE scheduled_task_run (task_id INTEGER, run_id TEXT UNIQUE, status TEXT, notify INTEGER, summary TEXT, error_message TEXT, started_time TEXT, finished_time TEXT, butler_followup_status TEXT);").unwrap();
    let now = Utc::now();
    let due = now - Duration::minutes(1);
    let task = task(now, due, "once");
    conn.execute("INSERT INTO scheduled_task VALUES (42, 1, ?1, ?2)", rusqlite::params![due.to_rfc3339(), now]).unwrap();
    record_missed_in_connection(&mut conn, &task, due, None, now).unwrap();
    let status: String = conn.query_row("SELECT status FROM scheduled_task_run", [], |row| row.get(0)).unwrap();
    assert_eq!(status, "missed");
}
