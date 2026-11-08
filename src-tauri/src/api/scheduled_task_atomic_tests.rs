use super::*;
use chrono::Duration as ChronoDuration;

fn fixture() -> (rusqlite::Connection, ScheduledTask, ScheduledTask, ScheduledTaskRun) {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE scheduled_task (id INTEGER PRIMARY KEY, is_enabled INTEGER, next_run_at TEXT, last_run_at TEXT, updated_time TEXT);
        CREATE TABLE scheduled_task_run (id INTEGER PRIMARY KEY, task_id INTEGER, run_id TEXT UNIQUE, status TEXT, notify INTEGER, summary TEXT, error_message TEXT, started_time TEXT, finished_time TEXT, butler_followup_status TEXT);").unwrap();
    let now = Utc::now();
    let original = ScheduledTask {
        id: 8, name: "test".into(), is_enabled: true, schedule_type: "once".into(),
        interval_value: None, interval_unit: None, start_time: None, week_days: None, month_days: None,
        run_at: Some(now), next_run_at: Some(now), last_run_at: None, assistant_id: 1,
        task_prompt: String::new(), notify_prompt: String::new(), butler_conversation_id: None,
        created_time: now, updated_time: now,
    };
    let updated = ScheduledTask { is_enabled: false, next_run_at: None, last_run_at: Some(now), updated_time: now + ChronoDuration::seconds(1), ..original.clone() };
    let run = ScheduledTaskRun { id: 0, task_id: 8, run_id: "run-8".into(), status: "running".into(), notify: false, summary: None, error_message: None, started_time: now, finished_time: None, butler_followup_status: None };
    conn.execute("INSERT INTO scheduled_task VALUES (?1, 1, ?2, NULL, ?3)", rusqlite::params![original.id, original.next_run_at, original.updated_time]).unwrap();
    (conn, original, updated, run)
}

#[test]
fn test_scheduled_run_commits_schedule_and_running_record_together() {
    let (mut conn, original, updated, run) = fixture();
    create_scheduled_run_atomically(&mut conn, &original, &updated, &run).unwrap();
    let (enabled, count): (bool, i64) = conn.query_row("SELECT is_enabled, (SELECT count(*) FROM scheduled_task_run) FROM scheduled_task", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert!(!enabled);
    assert_eq!(count, 1);
    assert!(create_scheduled_run_atomically(&mut conn, &original, &updated, &run).is_err());
}

#[test]
fn test_scheduled_run_insert_failure_rolls_back_schedule() {
    let (mut conn, original, updated, run) = fixture();
    conn.execute("INSERT INTO scheduled_task_run (run_id) VALUES (?1)", [&run.run_id]).unwrap();
    assert!(create_scheduled_run_atomically(&mut conn, &original, &updated, &run).is_err());
    let (enabled, last): (bool, Option<String>) = conn.query_row("SELECT is_enabled, last_run_at FROM scheduled_task", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert!(enabled && last.is_none());
}

#[test]
fn test_scheduled_run_rejects_changed_task_without_run() {
    let (mut conn, original, updated, run) = fixture();
    conn.execute("UPDATE scheduled_task SET updated_time=?1", [updated.updated_time]).unwrap();
    assert!(create_scheduled_run_atomically(&mut conn, &original, &updated, &run).is_err());
    let count: i64 = conn.query_row("SELECT count(*) FROM scheduled_task_run", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn test_scheduled_run_accepts_equivalent_rfc3339_timestamp() {
    let (mut conn, original, updated, run) = fixture();
    conn.execute("UPDATE scheduled_task SET updated_time=?1", [original.updated_time.to_rfc3339()]).unwrap();
    assert!(create_scheduled_run_atomically(&mut conn, &original, &updated, &run).is_ok());
}

#[test]
fn test_scheduled_run_registration_is_removed_on_drop() {
    let registration = register_scheduled_run_cancel_token("test-registration-drop");
    assert!(scheduled_run_cancel_registry().lock().unwrap().contains_key("test-registration-drop"));
    drop(registration);
    assert!(!scheduled_run_cancel_registry().lock().unwrap().contains_key("test-registration-drop"));
}

#[test]
fn test_scheduled_admission_rechecks_paused_contract_inside_transaction() {
    let (mut conn, mut original, mut updated, run) = fixture();
    original.butler_conversation_id=Some(1);
    updated.butler_conversation_id=Some(1);
    let policy=crate::db::butler_check::CheckPolicy {
        mode:"read_only".into(),sources:vec!["https://example.com".into()],
        expires_at:"2099-01-01T00:00:00Z".into(),paused:true,stopped:false,stop_current_on_pause:false,
    };
    crate::db::butler_check::save(&conn,original.id,&policy).unwrap();
    assert!(create_scheduled_run_atomically(&mut conn,&original,&updated,&run).is_err());
    assert_eq!(conn.query_row("SELECT count(*) FROM scheduled_task_run",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert!(conn.query_row("SELECT is_enabled FROM scheduled_task",[],|r|r.get::<_,bool>(0)).unwrap());
}
