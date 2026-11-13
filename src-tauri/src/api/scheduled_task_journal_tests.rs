use super::*;
use crate::db::scheduled_task_db::{mark_interrupted_runs_in_connection, ScheduledTaskDatabase};
use rusqlite::{params, Connection};

const RUN_ID: &str = "f74d5c52-400d-40bc-ae68-c70660ce745e";

fn running_fixture() -> Connection {
    let db = ScheduledTaskDatabase::from_connection(Connection::open_in_memory().unwrap());
    db.create_tables().unwrap();
    db.conn
        .execute(
            "INSERT INTO scheduled_task_run (task_id, run_id, status, notify, started_time) VALUES (8, ?1, 'running', 0, ?2)",
            params![RUN_ID, Utc::now()],
        )
        .unwrap();
    db.conn
}

fn valid_journal(run_id: &str) -> Vec<u8> {
    serde_json::to_vec(&ScheduledOutcomeJournal {
        task_id: 8,
        run_id: run_id.to_string(),
        finished_time: Utc::now(),
        outcome: ScheduledRunOutcome::failed("timeout", "运行超时".into(), Some("部分成果".into())),
    })
    .unwrap()
}

#[test]
fn test_scheduled_journal_truncated_temporary_file_does_not_leave_running_record() {
    let conn = running_fixture();
    let bytes = valid_journal(RUN_ID);
    let mut protected = Vec::new();
    assert!(accept_scheduled_outcome_journal(RUN_ID, &bytes[..bytes.len() / 2], &mut protected).is_err());
    assert!(protected.is_empty());
    let recovered = mark_interrupted_runs_in_connection(&conn, &protected, "应用退出，结果日志不完整", Utc::now()).unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].run_id, RUN_ID);
    assert_eq!(recovered[0].status, "failed");
    assert!(recovered[0].finished_time.is_some());
    assert!(recovered[0].error_message.as_deref().unwrap().contains("结果日志不完整"));
}

#[test]
fn test_scheduled_journal_mismatched_identity_does_not_protect_running_record() {
    let conn = running_fixture();
    let mut protected = Vec::new();
    assert!(accept_scheduled_outcome_journal(RUN_ID, &valid_journal("another-run"), &mut protected).is_err());
    assert!(protected.is_empty());
    assert_eq!(mark_interrupted_runs_in_connection(&conn, &protected, "结果日志身份不符", Utc::now()).unwrap().len(), 1);
}

#[test]
fn test_scheduled_journal_valid_fallback_protects_recoverable_outcome() {
    let conn = running_fixture();
    let mut protected = Vec::new();
    assert!(accept_scheduled_outcome_journal(RUN_ID, b"{", &mut protected).is_err());
    let entry = accept_scheduled_outcome_journal(RUN_ID, &valid_journal(RUN_ID), &mut protected).unwrap();
    assert_eq!(entry.outcome.text.as_deref(), Some("部分成果"));
    assert_eq!(protected, vec![RUN_ID.to_string()]);
    assert!(mark_interrupted_runs_in_connection(&conn, &protected, "应用退出", Utc::now()).unwrap().is_empty());
    let status: String = conn.query_row("SELECT status FROM scheduled_task_run", [], |row| row.get(0)).unwrap();
    assert_eq!(status, "running");
}

#[test]
fn test_scheduled_outcome_deserialization_does_not_borrow_input() {
    fn require_owned<T: serde::de::DeserializeOwned>() {}
    require_owned::<ScheduledRunOutcome>();
    require_owned::<ScheduledOutcomeJournal>();
    let recovered = {
        let bytes = valid_journal(RUN_ID);
        serde_json::from_slice::<ScheduledOutcomeJournal>(&bytes).unwrap()
    };
    assert_eq!(recovered.outcome.end_reason, "timeout");
    assert_eq!(recovered.outcome.text.as_deref(), Some("部分成果"));
    assert_eq!(recovered.outcome.status, "failed");
    assert_eq!(recovered.outcome.error.as_deref(), Some("运行超时"));
}
