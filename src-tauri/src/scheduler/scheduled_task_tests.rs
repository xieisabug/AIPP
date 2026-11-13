use super::*;
use crate::db::scheduled_task_db::{
    RunFinish, ScheduledTaskDatabase, DELIVER_OWNER_CHAT, RUN_FAILED, RUN_MISSED, RUN_QUIET, RUN_RUNNING,
    RUN_SKIPPED, SKIP_OVERLAP,
};
use crate::scheduler::service::{build_task, TaskDefaults, TaskInput};
use chrono::Duration;
use rusqlite::Connection;

fn t0() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-12T08:00:00Z").unwrap().with_timezone(&Utc)
}

fn hourly(policy: &str) -> ScheduledTask {
    let input = TaskInput {
        name: "检查".into(),
        schedule: "every 1h".into(),
        prompt: "看看".into(),
        assistant_id: 40,
        missed_policy: Some(policy.into()),
        ..Default::default()
    };
    let defaults = TaskDefaults { deliver_to: DELIVER_OWNER_CHAT.into(), butler_mode: true, ..Default::default() };
    build_task(&input, &defaults, t0()).unwrap()
}

fn db() -> ScheduledTaskDatabase {
    let db = ScheduledTaskDatabase::from_connection(Connection::open_in_memory().unwrap());
    db.create_tables().unwrap();
    db
}

fn claim(plan: &OccurrencePlan, run_id: &str, now: DateTime<Utc>) -> OccurrenceClaim {
    OccurrenceClaim {
        run_id: run_id.into(),
        scheduled_for: plan.scheduled_for,
        next_run_at: plan.next_run_at,
        missed: plan.missed.clone(),
        main_missed: plan.main_missed,
        skip_reason: None,
        now,
    }
}

#[test]
fn test_plan_on_time_runs_the_due_slot_and_advances() {
    let task = hourly("run_once");
    let schedule = task.schedule().unwrap();
    let now = t0() + Duration::hours(1) + Duration::seconds(20);
    let plan = plan_occurrence(&task, &schedule, now, t0(), false).unwrap();
    assert_eq!(plan.scheduled_for, t0() + Duration::hours(1));
    assert!(plan.missed.is_empty() && !plan.main_missed);
    assert_eq!(plan.next_run_at, Some(t0() + Duration::hours(2)));
}

#[test]
fn test_plan_after_downtime_runs_latest_once_and_records_earlier_as_missed() {
    let task = hourly("run_once");
    let schedule = task.schedule().unwrap();
    let now = t0() + Duration::hours(5) + Duration::minutes(30);
    let plan = plan_occurrence(&task, &schedule, now, t0(), false).unwrap();
    assert_eq!(plan.scheduled_for, t0() + Duration::hours(5));
    assert_eq!(plan.missed.len(), 4);
    assert!(!plan.main_missed);
    assert_eq!(plan.next_run_at, Some(t0() + Duration::hours(6)));
}

#[test]
fn test_plan_skip_policy_marks_late_slot_missed_but_runs_on_time() {
    let task = hourly(MISSED_SKIP);
    let schedule = task.schedule().unwrap();
    let late = plan_occurrence(&task, &schedule, t0() + Duration::hours(2) + Duration::minutes(30), t0(), false).unwrap();
    assert!(late.main_missed);
    let on_time = plan_occurrence(&task, &schedule, t0() + Duration::hours(1) + Duration::minutes(1), t0(), false).unwrap();
    assert!(!on_time.main_missed);
}

#[test]
fn test_plan_mobile_does_not_replay_slots_before_process_start() {
    let task = hourly("run_once");
    let schedule = task.schedule().unwrap();
    let started = t0() + Duration::hours(3) + Duration::seconds(30);
    let plan = plan_occurrence(&task, &schedule, started + Duration::seconds(5), started, true).unwrap();
    assert!(plan.main_missed, "移动端启动前错过的时段只记录");
    assert!(!plan_occurrence(&task, &schedule, started + Duration::seconds(5), started, false).unwrap().main_missed);
}

#[test]
fn test_plan_missed_records_are_capped() {
    let task = hourly("run_once");
    let schedule = task.schedule().unwrap();
    let plan = plan_occurrence(&task, &schedule, t0() + Duration::days(3), t0(), false).unwrap();
    assert_eq!(plan.missed.len(), MISSED_RECORD_LIMIT);
    assert_eq!(*plan.missed.last().unwrap(), plan.scheduled_for - Duration::hours(1));
}

#[test]
fn test_plan_respects_max_runs_and_ends_task() {
    let task = ScheduledTask { max_runs: Some(3), run_count: 2, ..hourly("run_once") };
    let schedule = task.schedule().unwrap();
    let plan = plan_occurrence(&task, &schedule, t0() + Duration::hours(1), t0(), false).unwrap();
    assert_eq!(plan.next_run_at, None, "第 3 次运行后结束");
}

#[test]
fn test_claim_registers_run_and_advances_even_if_execution_later_fails() {
    let mut db = db();
    let task = db.create_task(&hourly("run_once")).unwrap();
    let schedule = task.schedule().unwrap();
    let now = t0() + Duration::hours(1) + Duration::seconds(3);
    let plan = plan_occurrence(&task, &schedule, now, t0(), false).unwrap();
    let due = task.next_run_at.unwrap();
    assert_eq!(db.claim_occurrence(task.id, due, &claim(&plan, "r1", now)).unwrap(), ClaimResult::Execute);
    // 同一时点再次扫描：任务已推进，不会重复处理
    assert_eq!(db.claim_occurrence(task.id, due, &claim(&plan, "r1b", now)).unwrap(), ClaimResult::Stale);
    let stored = db.read_task(task.id).unwrap().unwrap();
    assert_eq!(stored.next_run_at, Some(t0() + Duration::hours(2)));
    assert_eq!((stored.run_count, stored.last_run_at), (1, Some(now)));
    let run = db.read_run("r1").unwrap().unwrap();
    assert_eq!((run.status.as_str(), run.scheduled_for), (RUN_RUNNING, Some(plan.scheduled_for)));

    // 准入检查失败（例如执行助手已删除）：记为跳过并写明原因，计入连续失败
    let failures = db
        .save_run_outcome("r1", &RunFinish {
            status: RUN_SKIPPED.into(),
            error_message: Some("执行助手 #40 不存在".into()),
            finished_time: Some(now),
            ..Default::default()
        }, "{}")
        .unwrap();
    assert_eq!(failures, 1);
    let run = db.read_run("r1").unwrap().unwrap();
    assert_eq!((run.status.as_str(), run.error_message.as_deref()), (RUN_SKIPPED, Some("执行助手 #40 不存在")));
}

#[test]
fn test_claim_records_overlap_as_skipped_without_executing() {
    let mut db = db();
    let task = db.create_task(&hourly("run_once")).unwrap();
    let schedule = task.schedule().unwrap();
    let first_now = t0() + Duration::hours(1);
    let plan = plan_occurrence(&task, &schedule, first_now, t0(), false).unwrap();
    db.claim_occurrence(task.id, task.next_run_at.unwrap(), &claim(&plan, "r1", first_now)).unwrap();
    let task = db.read_task(task.id).unwrap().unwrap();
    let second_now = t0() + Duration::hours(2);
    let plan = plan_occurrence(&task, &schedule, second_now, t0(), false).unwrap();
    let result = db.claim_occurrence(task.id, task.next_run_at.unwrap(), &claim(&plan, "r2", second_now)).unwrap();
    assert_eq!(result, ClaimResult::Skipped(SKIP_OVERLAP.to_string()));
    let stored = db.read_task(task.id).unwrap().unwrap();
    assert_eq!(stored.run_count, 1, "跳过不计入运行次数");
    assert_eq!(stored.next_run_at, Some(t0() + Duration::hours(3)));
    // 重叠跳过不算失败
    let failures = db.save_run_outcome("r1", &RunFinish { status: RUN_QUIET.into(), finished_time: Some(second_now), ..Default::default() }, "{}").unwrap();
    assert_eq!(failures, 0);
}

#[test]
fn test_claim_writes_missed_rows_and_once_task_finishes() {
    let mut db = db();
    let task = db.create_task(&hourly("run_once")).unwrap();
    let schedule = task.schedule().unwrap();
    let now = t0() + Duration::hours(4) + Duration::minutes(10);
    let plan = plan_occurrence(&task, &schedule, now, t0(), false).unwrap();
    db.claim_occurrence(task.id, task.next_run_at.unwrap(), &claim(&plan, "late", now)).unwrap();
    let runs = db.list_runs_by_task(task.id, 10).unwrap();
    assert_eq!(runs.iter().filter(|r| r.status == RUN_MISSED).count(), 3);

    let once_input = TaskInput { schedule: "in 30m".into(), ..TaskInput { name: "一次".into(), prompt: "x".into(), assistant_id: 40, ..Default::default() } };
    let once = db.create_task(&build_task(&once_input, &TaskDefaults { deliver_to: DELIVER_OWNER_CHAT.into(), ..Default::default() }, t0()).unwrap()).unwrap();
    let once_schedule = once.schedule().unwrap();
    let now = t0() + Duration::minutes(31);
    let plan = plan_occurrence(&once, &once_schedule, now, t0(), false).unwrap();
    assert_eq!(plan.next_run_at, None);
    db.claim_occurrence(once.id, once.next_run_at.unwrap(), &claim(&plan, "once", now)).unwrap();
    let stored = db.read_task(once.id).unwrap().unwrap();
    assert!(!stored.is_enabled);
    assert_eq!(stored.paused_reason.as_deref(), Some("已完成"));
}

#[test]
fn test_consecutive_failures_reset_after_success() {
    let mut db = db();
    let task = db.create_task(&hourly("run_once")).unwrap();
    for (i, status) in [RUN_FAILED, RUN_FAILED].iter().enumerate() {
        let run_id = format!("f{i}");
        db.claim_manual_run(task.id, &run_id, t0()).unwrap();
        db.save_run_outcome(&run_id, &RunFinish { status: (*status).into(), error_message: Some("模型调用失败".into()), ..Default::default() }, "{}").unwrap();
    }
    assert_eq!(db.read_task(task.id).unwrap().unwrap().consecutive_failures, 2);
    db.claim_manual_run(task.id, "ok", t0()).unwrap();
    let failures = db.save_run_outcome("ok", &RunFinish { status: crate::db::scheduled_task_db::RUN_DELIVERED.into(), ..Default::default() }, "{}").unwrap();
    assert_eq!(failures, 0);
}

#[test]
fn test_manual_run_rejected_while_previous_still_running() {
    let mut db = db();
    let task = db.create_task(&hourly("run_once")).unwrap();
    assert_eq!(db.claim_manual_run(task.id, "m1", t0()).unwrap(), ClaimResult::Execute);
    assert_eq!(db.claim_manual_run(task.id, "m2", t0()).unwrap(), ClaimResult::Skipped(SKIP_OVERLAP.to_string()));
    let stored = db.read_task(task.id).unwrap().unwrap();
    assert_eq!(stored.next_run_at, task.next_run_at, "手动运行不改变计划时间");
}
