//! Production task writes must not recreate children after task deletion; legacy schedules convert safely.
use super::*;
use chrono::{FixedOffset, NaiveDateTime, TimeZone};

fn memory_db() -> ScheduledTaskDatabase {
    let db = ScheduledTaskDatabase { conn: Connection::open_in_memory().unwrap(), write_lock: Arc::new(Mutex::new(())) };
    db.create_tables().unwrap();
    db
}

fn insert_legacy_tasks(db: &ScheduledTaskDatabase) {
    db.conn.execute_batch("INSERT INTO scheduled_task(id,name,schedule_type,assistant_id,task_prompt,notify_prompt,schedule_expr)
        VALUES(1,'delete','once',1,'work','notify','every 1h'),(2,'keep','once',1,'work','notify','every 1h');").unwrap();
}

fn run(task_id: i64, run_id: &str) -> ScheduledTaskRun {
    ScheduledTaskRun::new(task_id, run_id, RUN_RUNNING, Utc::now())
}

fn finish(status: &str) -> RunFinish {
    RunFinish { status: status.into(), finished_time: Some(Utc::now()), ..Default::default() }
}

#[test]
fn test_cascade_delete_scheduled_task_rejects_late_logs_and_runs() {
    let db = memory_db();
    insert_legacy_tasks(&db);
    let log = ScheduledTaskLog { id: 0, task_id: 1, run_id: "run-1".into(), message_type: "start".into(), content: "log".into(), created_time: Utc::now() };
    db.add_log(&log).unwrap();
    db.create_run(&run(1, "run-1")).unwrap();
    db.add_log(&ScheduledTaskLog { task_id: 2, run_id: "run-2".into(), ..log.clone() }).unwrap();
    db.delete_task(1).unwrap();
    assert!(db.add_log(&log).unwrap_err().to_string().contains("run-1"));
    assert!(db.create_run(&run(1, "run-1")).unwrap_err().to_string().contains("run-1"));
    assert!(db.list_logs_by_task(1, 100).unwrap().is_empty());
    assert!(db.list_runs_by_task(1, 100).unwrap().is_empty());
    assert_eq!(db.list_logs_by_task(2, 100).unwrap().len(), 1);
}

#[test]
fn test_mark_interrupted_runs_only_closes_orphan_running() {
    let db = memory_db();
    insert_legacy_tasks(&db);
    db.create_run(&run(1, "orphan")).unwrap();
    db.create_run(&run(1, "local")).unwrap();
    db.create_run(&ScheduledTaskRun { status: RUN_DELIVERED.into(), finished_time: Some(Utc::now()), ..run(1, "done") }).unwrap();
    let closed = db.mark_interrupted_runs(&["local".to_string()], "应用退出", Utc::now()).unwrap();
    assert_eq!(closed.len(), 1);
    assert_eq!((closed[0].run_id.as_str(), closed[0].status.as_str()), ("orphan", RUN_FAILED));
    assert_eq!(db.read_run("local").unwrap().unwrap().status, RUN_RUNNING);
    assert_eq!(db.read_run("done").unwrap().unwrap().status, RUN_DELIVERED);
}

#[test]
fn test_scheduled_run_read_existing_missing_and_deleted() {
    let db = memory_db();
    insert_legacy_tasks(&db);
    db.create_run(&run(1, "outcome-run")).unwrap();
    let stored = db.read_run("outcome-run").unwrap().unwrap();
    assert_eq!((stored.task_id, stored.status.as_str()), (1, RUN_RUNNING));
    assert!(db.read_run("missing-run").unwrap().is_none());
    db.delete_task(1).unwrap();
    assert!(db.read_run("outcome-run").unwrap().is_none());
}

/// 结束状态、投递信息与完整结果一起保存。
#[test]
fn test_save_run_outcome_records_delivery_and_payload() {
    let db = memory_db();
    insert_legacy_tasks(&db);
    db.create_run(&run(1, "outcome-run")).unwrap();
    db.set_run_conversation("outcome-run", 30).unwrap();
    let payload = r#"{"status":"delivered","end_reason":"completed","text":"今天休息"}"#;
    db.save_run_outcome("outcome-run", &RunFinish {
        status: RUN_DELIVERED.into(),
        notify: true,
        summary: Some("今天休息".into()),
        finished_time: Some(Utc::now()),
        conversation_id: Some(30),
        final_message_id: Some(31),
        delivered_message_id: Some(900),
        delivery_conversation_id: Some(1529),
        ..Default::default()
    }, payload).unwrap();
    let stored = db.read_run("outcome-run").unwrap().unwrap();
    assert_eq!((stored.status.as_str(), stored.notify), (RUN_DELIVERED, true));
    assert_eq!((stored.conversation_id, stored.final_message_id), (Some(30), Some(31)));
    assert_eq!((stored.delivered_message_id, stored.delivery_conversation_id), (Some(900), Some(1529)));
    assert_eq!(db.read_run_outcome("outcome-run").unwrap().as_deref(), Some(payload));
    db.create_tables().unwrap();
    assert_eq!(db.read_run_outcome("outcome-run").unwrap().as_deref(), Some(payload), "重新初始化表不影响已保存结果");
    assert_eq!(db.latest_runs().unwrap()[&1].run_id, "outcome-run");
}

/// 插入完整结果失败时，终态与失败计数也必须回滚。
#[test]
fn test_save_run_outcome_insert_failure_rolls_back_terminal_update() {
    let db = memory_db();
    insert_legacy_tasks(&db);
    db.create_run(&run(1, "outcome-run")).unwrap();
    db.conn.execute_batch(
        "CREATE TRIGGER reject_outcome_insert BEFORE INSERT ON scheduled_task_run_outcome
         BEGIN SELECT RAISE(ABORT, 'outcome insert rejected'); END;",
    ).unwrap();
    let error = db.save_run_outcome("outcome-run", &RunFinish { error_message: Some("x".into()), ..finish(RUN_FAILED) }, "{}").unwrap_err();
    assert!(error.to_string().contains("outcome insert rejected"));
    let stored = db.read_run("outcome-run").unwrap().unwrap();
    assert_eq!(stored.status, RUN_RUNNING);
    assert!(stored.finished_time.is_none());
    assert_eq!(db.read_task(1).unwrap().unwrap().consecutive_failures, 0);
    assert!(db.conn.is_autocommit());
}

/// 删除任务同时清理完整结果，迟到的保存不能创建孤儿结果（不依赖外键开关）。
#[test]
fn test_save_run_outcome_after_delete_is_rejected() {
    for foreign_keys in [false, true] {
        let db = memory_db();
        insert_legacy_tasks(&db);
        db.conn.pragma_update(None, "foreign_keys", foreign_keys).unwrap();
        db.create_run(&run(1, "outcome-run")).unwrap();
        db.save_run_outcome("outcome-run", &finish(RUN_QUIET), "{}").unwrap();
        db.delete_task(1).unwrap();
        assert!(db.read_run_outcome("outcome-run").unwrap().is_none());
        let error = db.save_run_outcome("outcome-run", &finish(RUN_QUIET), "{}").unwrap_err();
        assert!(error.to_string().contains("outcome-run"));
        assert!(db.conn.is_autocommit());
    }
}

#[test]
fn test_new_run_statuses_are_accepted_and_unknown_rejected() {
    let db = memory_db();
    insert_legacy_tasks(&db);
    for (i, status) in RUN_STATUSES.iter().enumerate() {
        db.conn.execute(
            "INSERT INTO scheduled_task_run(task_id, run_id, status) VALUES (1, ?1, ?2)",
            rusqlite::params![format!("s{i}"), status],
        ).unwrap();
    }
    assert!(db.conn.execute("INSERT INTO scheduled_task_run(task_id, run_id, status) VALUES (1, 'bad', 'invalid')", []).is_err());
}

// ── 旧时间设置转换 ──────────────────────────────────────────────────

fn tz() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}

fn at(text: &str) -> DateTime<Utc> {
    let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap();
    tz().from_local_datetime(&naive).unwrap().with_timezone(&Utc)
}

fn legacy(unit: &str, value: i64, start: Option<&str>, week_days: Option<&str>, month_days: Option<&str>, next: &str) -> LegacySchedule {
    LegacySchedule {
        schedule_type: "interval".into(),
        interval_value: Some(value),
        interval_unit: Some(unit.into()),
        start_time: start.map(str::to_string),
        week_days: week_days.map(str::to_string),
        month_days: month_days.map(str::to_string),
        run_at: None,
        next_run_at: Some(at(next)),
    }
}

#[test]
fn test_convert_legacy_weekly_reminders_losslessly() {
    let now = at("2026-11-12 18:00");
    // 用户的训练提醒：每周一 16:00、每周二/六 21:00
    let monday = convert_legacy_schedule(&legacy("week", 1, Some("16:00"), Some("[1]"), None, "2026-11-16 16:00"), now, &tz()).unwrap();
    assert_eq!((monday.expr.as_str(), monday.needs_review.as_ref()), ("0 16 * * 1", None));
    let twice = convert_legacy_schedule(&legacy("week", 1, Some("21:00"), Some("[2,6]"), None, "2026-11-14 21:00"), now, &tz()).unwrap();
    assert_eq!(twice.expr, "0 21 * * 2,6");
    let next = crate::scheduler::expr::parse_in(&twice.expr, now, &tz()).unwrap().next_after_in(now, twice.anchor, &tz()).unwrap();
    assert_eq!(next, at("2026-11-14 21:00"));
}

#[test]
fn test_convert_legacy_other_shapes() {
    let now = at("2026-11-12 18:00");
    let daily = convert_legacy_schedule(&legacy("day", 1, Some("9:05"), None, None, "2026-11-13 09:05"), now, &tz()).unwrap();
    assert_eq!(daily.expr, "5 9 * * *");
    let every3 = convert_legacy_schedule(&legacy("day", 3, Some("22:00"), None, None, "2026-11-14 22:00"), now, &tz()).unwrap();
    assert_eq!(every3.expr, "every 3d at 22:00");
    let schedule = crate::scheduler::expr::parse_in(&every3.expr, now, &tz()).unwrap();
    assert_eq!(schedule.next_after_in(now, every3.anchor, &tz()), Some(at("2026-11-14 22:00")), "保持原来的下一次时间");
    let minutes = convert_legacy_schedule(&legacy("minute", 30, None, None, None, "2026-11-12 18:20"), now, &tz()).unwrap();
    assert_eq!(minutes.expr, "every 30m");
    let schedule = crate::scheduler::expr::parse_in(&minutes.expr, now, &tz()).unwrap();
    assert_eq!(schedule.next_after_in(now, minutes.anchor, &tz()), Some(at("2026-11-12 18:20")));
    let monthly = convert_legacy_schedule(&legacy("month", 1, Some("09:00"), None, Some("[1,15]"), "2026-11-15 09:00"), now, &tz()).unwrap();
    assert_eq!(monthly.expr, "0 9 1,15 * *");
    let biweekly = convert_legacy_schedule(&legacy("week", 2, Some("16:00"), Some("[4]"), None, "2026-11-19 16:00"), now, &tz()).unwrap();
    assert_eq!((biweekly.expr.as_str(), biweekly.needs_review.is_none()), ("every 2w at 16:00", true));
    let biweekly_multi = convert_legacy_schedule(&legacy("week", 2, Some("16:00"), Some("[1,4]"), None, "2026-11-19 16:00"), now, &tz()).unwrap();
    assert!(biweekly_multi.needs_review.is_some(), "无法精确表达时要求用户确认");
    let once = LegacySchedule { schedule_type: "once".into(), run_at: Some(at("2026-11-20 08:00")), ..Default::default() };
    assert_eq!(convert_legacy_schedule(&once, now, &tz()).unwrap().expr, "2026-11-20T08:00:00+08:00");
    assert!(convert_legacy_schedule(&LegacySchedule { schedule_type: "interval".into(), ..Default::default() }, now, &tz()).is_err());
}

#[test]
fn test_migrate_legacy_rows_sets_delivery_merges_notify_rules_and_does_not_replay() {
    let db = memory_db();
    db.conn.execute_batch("
        INSERT INTO scheduled_task(id,name,is_enabled,schedule_type,interval_value,interval_unit,start_time,week_days,next_run_at,assistant_id,task_prompt,notify_prompt,butler_conversation_id,created_time,updated_time)
        VALUES (6,'周四休息提醒',1,'interval',1,'week','16:00','[4]','2020-01-02 08:00:00+00:00',40,'提醒用户今天休息','用户明确要求提醒，必须通知',1520,'2020-01-01 00:00:00+00:00','2020-01-01 00:00:00+00:00'),
               (7,'日报',1,'interval',1,'day','09:00',NULL,'2020-01-02 01:00:00+00:00',3,'整理日报','',NULL,'2020-01-01 00:00:00+00:00','2020-01-01 00:00:00+00:00');
        UPDATE scheduled_task SET schedule_expr=NULL;
    ").unwrap();
    db.create_tables().unwrap();
    let reminder = db.read_task(6).unwrap().unwrap();
    assert_eq!(reminder.schedule_expr, "0 16 * * 4");
    assert_eq!((reminder.deliver_to.as_str(), reminder.butler_mode, reminder.is_enabled), (DELIVER_OWNER_CHAT, true, true));
    assert!(reminder.task_prompt.contains("提醒用户今天休息") && reminder.task_prompt.contains("必须通知"));
    assert!(reminder.next_run_at.unwrap() > Utc::now(), "迁移前错过的时段不补跑");
    let report = db.read_task(7).unwrap().unwrap();
    assert_eq!((report.deliver_to.as_str(), report.butler_mode), (DELIVER_RUN_CONVERSATION, false));
    assert_eq!(report.task_prompt, "整理日报", "没有通知规则时指令不变");
    // 再次初始化不会重复转换
    db.create_tables().unwrap();
    assert_eq!(db.read_task(6).unwrap().unwrap().task_prompt, reminder.task_prompt);
}
