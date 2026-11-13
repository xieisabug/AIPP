//! 到期任务的登记与派发：先登记、后检查。
//!
//! 每个到期时点在一个事务里写入运行记录并推进下一次时间（见
//! [`ScheduledTaskDatabase::claim_occurrence`]），之后的任何检查失败都只会把这条记录
//! 结束为「跳过」并写明原因，不会让同一时点被反复处理。
use std::sync::{Arc, OnceLock};

use chrono::{DateTime, Utc};
use tauri::Emitter;
use tokio::sync::Semaphore;
use tracing::warn;
use uuid::Uuid;

use crate::api::scheduled_task_api::{execute_claimed_run, SCHEDULED_TASK_CHANGED_EVENT};
use crate::db::scheduled_task_db::{ClaimResult, OccurrenceClaim, ScheduledTask, ScheduledTaskDatabase};
use crate::scheduler::expr::Schedule;
use crate::scheduler::service::{self, MISSED_SKIP};
use crate::scheduler::ScheduledRunKind;

/// 同时执行的定时任务上限，避免同一时刻撞上模型限流。
const MAX_CONCURRENT_RUNS: usize = 3;
/// 每次最多记录多少个更早错过的时段（每分钟一次的任务停了一天也只记这么多）。
const MISSED_RECORD_LIMIT: usize = 20;
/// 列举错过时段的上限。
const SLOT_SCAN_LIMIT: usize = 10_000;
/// 设为「错过不补跑」的任务，晚于计划这么久就按错过处理（正常调度有 1 分钟内的抖动）。
const SKIP_GRACE: chrono::Duration = chrono::Duration::minutes(5);

// The first scan happens when the process starts; mobile does not replay earlier occurrences.
static FIRST_SCHEDULER_SCAN: OnceLock<DateTime<Utc>> = OnceLock::new();

fn run_slots() -> Arc<Semaphore> {
    static SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SLOTS.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_RUNS))).clone()
}

/// 一个到期任务这次怎么处理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OccurrencePlan {
    /// 这次处理的计划时间（到现在为止最近的一次）。
    pub scheduled_for: DateTime<Utc>,
    /// 更早错过、只记录的时段（最多 [`MISSED_RECORD_LIMIT`] 个，取最近的）。
    pub missed: Vec<DateTime<Utc>>,
    /// 这次本身也按错过处理。
    pub main_missed: bool,
    pub next_run_at: Option<DateTime<Utc>>,
}

pub(crate) fn plan_occurrence(
    task: &ScheduledTask,
    schedule: &Schedule,
    now: DateTime<Utc>,
    first_scan: DateTime<Utc>,
    is_mobile: bool,
) -> Option<OccurrencePlan> {
    let due = task.next_run_at.filter(|due| *due <= now)?;
    let mut slots = vec![due];
    let mut cursor = due;
    while slots.len() < SLOT_SCAN_LIMIT {
        let Some(next) = schedule.next_after(cursor, task.schedule_anchor) else { break };
        if next > now || task.ends_at.is_some_and(|ends_at| next > ends_at) {
            break;
        }
        slots.push(next);
        cursor = next;
    }
    let scheduled_for = *slots.last()?;
    let earlier = &slots[..slots.len() - 1];
    let missed = earlier[earlier.len().saturating_sub(MISSED_RECORD_LIMIT)..].to_vec();
    let main_missed = (task.missed_policy == MISSED_SKIP && now - scheduled_for > SKIP_GRACE)
        // 移动端进程启动前错过的时段不补跑（系统可能长时间挂起应用）。
        || (is_mobile && scheduled_for < first_scan);
    let probe = ScheduledTask { run_count: task.run_count + i64::from(!main_missed), ..task.clone() };
    let next_run_at = service::next_run(&probe, schedule, scheduled_for);
    Some(OccurrencePlan { scheduled_for, missed, main_missed, next_run_at })
}

fn emit_changed(app: &tauri::AppHandle, task_id: i64) {
    let _ = app.emit(SCHEDULED_TASK_CHANGED_EVENT, serde_json::json!({ "taskId": task_id }));
}

fn spawn_execution(app: &tauri::AppHandle, task: ScheduledTask, run_id: String, scheduled_for: DateTime<Utc>, kind: ScheduledRunKind) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _permit = run_slots().acquire_owned().await;
        execute_claimed_run(app, task, run_id, scheduled_for, kind).await;
    });
}

pub async fn run_scheduled_tasks(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let mut db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let now = Utc::now();
    let first_scan = *FIRST_SCHEDULER_SCAN.get_or_init(|| now);
    for task in db.list_due_tasks(now).map_err(|e| e.to_string())? {
        let Some(due) = task.next_run_at else { continue };
        let schedule = match task.schedule() {
            Ok(schedule) => schedule,
            Err(error) => {
                // 无法计算时间的任务暂停并写明原因，而不是每分钟重复报错。
                let _ = db.pause_task(task.id, &format!("时间设置无效（{error}），已暂停"), now);
                emit_changed(app_handle, task.id);
                continue;
            }
        };
        let Some(plan) = plan_occurrence(&task, &schedule, now, first_scan, cfg!(mobile)) else { continue };
        let run_id = Uuid::new_v4().to_string();
        let claim = OccurrenceClaim {
            run_id: run_id.clone(),
            scheduled_for: plan.scheduled_for,
            next_run_at: plan.next_run_at,
            missed: plan.missed,
            main_missed: plan.main_missed,
            skip_reason: None,
            now,
        };
        match db.claim_occurrence(task.id, due, &claim) {
            Ok(ClaimResult::Execute) => {
                spawn_execution(app_handle, task, run_id, plan.scheduled_for, ScheduledRunKind::Scheduled);
            }
            Ok(ClaimResult::Skipped(_)) | Ok(ClaimResult::Missed) => emit_changed(app_handle, task.id),
            Ok(ClaimResult::Stale) => {}
            Err(error) => warn!(task_id = task.id, error = %error, "登记定时任务运行失败，下一轮重试"),
        }
    }
    Ok(())
}

/// 立即运行一次（不改变计划时间）。上一次还在运行时拒绝。
pub async fn run_now(app_handle: &tauri::AppHandle, task_id: i64) -> Result<String, String> {
    let mut db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let task = db.read_task(task_id).map_err(|e| e.to_string())?.ok_or_else(|| format!("定时任务 #{task_id} 不存在"))?;
    let run_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    match db.claim_manual_run(task_id, &run_id, now).map_err(|e| e.to_string())? {
        ClaimResult::Execute => {
            spawn_execution(app_handle, task, run_id.clone(), now, ScheduledRunKind::Manual);
            Ok(run_id)
        }
        ClaimResult::Skipped(reason) => Err(format!("{reason}，请等它结束或先停止")),
        ClaimResult::Stale | ClaimResult::Missed => Err(format!("定时任务 #{task_id} 不存在")),
    }
}

#[cfg(test)]
#[path = "scheduled_task_tests.rs"]
mod tests;
