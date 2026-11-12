use super::*;
use chrono::Duration;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-12T02:00:00Z").unwrap().with_timezone(&Utc)
}

fn input(schedule: &str) -> TaskInput {
    TaskInput {
        name: "周四休息提醒".into(),
        schedule: schedule.into(),
        prompt: "提醒用户今天休息".into(),
        assistant_id: 40,
        ..Default::default()
    }
}

fn butler_defaults() -> TaskDefaults {
    TaskDefaults {
        created_in_conversation_id: Some(1520),
        butler_mode: true,
        deliver_to: DELIVER_OWNER_CHAT.into(),
        deliver_conversation_id: None,
    }
}

#[test]
fn test_build_task_normalizes_schedule_and_computes_next() {
    let task = build_task(&input("every 30 minutes"), &butler_defaults(), now()).unwrap();
    assert_eq!(task.schedule_expr, "every 30m");
    assert_eq!(task.next_run_at, Some(now() + Duration::minutes(30)));
    assert_eq!(task.deliver_to, DELIVER_OWNER_CHAT);
    assert_eq!(task.deliver_policy, DELIVER_ALWAYS);
    assert_eq!(task.missed_policy, MISSED_RUN_ONCE);
    assert!(task.butler_mode && task.is_enabled);
    assert_eq!(task.created_in_conversation_id, Some(1520));
}

#[test]
fn test_build_task_rejects_bad_input_with_specific_errors() {
    let defaults = butler_defaults();
    let cases: Vec<(TaskInput, &str)> = vec![
        (TaskInput { name: " ".into(), ..input("every 1h") }, "任务名称"),
        (TaskInput { prompt: "".into(), ..input("every 1h") }, "任务指令"),
        (input("每周四16点"), "0 16 * * 4"),
        (input("2020-01-01 09:00"), "已经过去"),
        (TaskInput { deliver_policy: Some("sometimes".into()), ..input("every 1h") }, "deliver"),
        (TaskInput { deliver_to: Some("conversation".into()), ..input("every 1h") }, "deliver_conversation_id"),
        (TaskInput { max_runs: Some(0), ..input("every 1h") }, "max_runs"),
        (TaskInput { assistant_id: 0, ..input("every 1h") }, "执行助手"),
    ];
    for (case, expected) in cases {
        let error = build_task(&case, &defaults, now()).unwrap_err();
        assert!(error.contains(expected), "expected「{expected}」in「{error}」");
    }
}

#[test]
fn test_build_task_rejects_schedule_that_never_runs_before_end() {
    let ends_soon = TaskInput { ends_at: Some("in 10m".into()), ..input("every 1h") };
    assert!(build_task(&ends_soon, &butler_defaults(), now()).unwrap_err().contains("不会再运行"));
}

#[test]
fn test_next_run_respects_max_runs_and_end_time() {
    let task = build_task(&TaskInput { max_runs: Some(2), ..input("every 1h") }, &butler_defaults(), now()).unwrap();
    let schedule = task.schedule().unwrap();
    assert!(next_run(&ScheduledTask { run_count: 1, ..task.clone() }, &schedule, now()).is_some());
    assert!(next_run(&ScheduledTask { run_count: 2, ..task.clone() }, &schedule, now()).is_none());
    let bounded = ScheduledTask { ends_at: Some(now() + Duration::minutes(90)), ..task };
    assert_eq!(next_run(&bounded, &schedule, now()), Some(now() + Duration::hours(1)));
    assert!(next_run(&bounded, &schedule, now() + Duration::hours(1)).is_none());
}

#[test]
fn test_apply_patch_resume_recomputes_from_now_without_replay() {
    let task = build_task(&input("every 1h"), &butler_defaults(), now()).unwrap();
    let paused = ScheduledTask {
        is_enabled: false,
        paused_reason: Some("连续失败 5 次，已自动暂停".into()),
        consecutive_failures: 5,
        next_run_at: Some(now() + Duration::hours(1)),
        ..task
    };
    let later = now() + Duration::hours(30) + Duration::minutes(10);
    let resumed = apply_patch(paused, &TaskPatch { enabled: Some(true), ..Default::default() }, later).unwrap();
    assert!(resumed.is_enabled);
    assert_eq!(resumed.paused_reason, None);
    assert_eq!(resumed.consecutive_failures, 0);
    assert!(resumed.next_run_at.unwrap() > later, "恢复时不补跑暂停期间的时段");
}

#[test]
fn test_apply_patch_schedule_change_resets_anchor_and_keeps_unchanged_fields() {
    let task = build_task(&input("every 1h"), &butler_defaults(), now()).unwrap();
    let later = now() + Duration::minutes(20);
    let patched = apply_patch(task.clone(), &TaskPatch { schedule: Some("every 15m".into()), ..Default::default() }, later).unwrap();
    assert_eq!(patched.schedule_expr, "every 15m");
    assert_eq!(patched.schedule_anchor, later);
    assert_eq!(patched.next_run_at, Some(later + Duration::minutes(15)));
    assert_eq!(patched.task_prompt, task.task_prompt);
    let renamed = apply_patch(task.clone(), &TaskPatch { name: Some("新名字".into()), ..Default::default() }, later).unwrap();
    assert_eq!(renamed.next_run_at, task.next_run_at, "只改名字不动计划");
}

#[test]
fn test_apply_patch_clears_end_conditions_and_validates_conversation_target() {
    let task = build_task(&TaskInput { max_runs: Some(3), ends_at: Some("in 30d".into()), ..input("every 1h") }, &butler_defaults(), now()).unwrap();
    let cleared = apply_patch(task.clone(), &TaskPatch { max_runs: Some(0), ends_at: Some(String::new()), ..Default::default() }, now()).unwrap();
    assert_eq!((cleared.max_runs, cleared.ends_at), (None, None));
    let error = apply_patch(task.clone(), &TaskPatch { deliver_to: Some(DELIVER_CONVERSATION.into()), ..Default::default() }, now()).unwrap_err();
    assert!(error.contains("deliver_conversation_id"));
    let moved = apply_patch(task, &TaskPatch { deliver_to: Some(DELIVER_CONVERSATION.into()), deliver_conversation_id: Some(9), ..Default::default() }, now()).unwrap();
    assert_eq!(moved.deliver_conversation_id, Some(9));
}

#[test]
fn test_task_view_flags_attention_and_lists_upcoming() {
    let task = build_task(&input("every 1h"), &butler_defaults(), now()).unwrap();
    let names = ViewNames {
        assistants: [(40, "健身教练".to_string())].into_iter().collect(),
        conversations: HashMap::new(),
        butler_assistant_id: Some(1),
    };
    let view = task_view(ScheduledTask { id: 6, ..task.clone() }, None, &names, now(), &|_| false);
    assert_eq!(view.upcoming.len(), 3);
    assert_eq!(view.deliver_target_text, "与健身教练的私聊");
    assert!(!view.needs_attention);
    let paused = ScheduledTask { is_enabled: false, paused_reason: Some("连续失败 5 次，已自动暂停".into()), ..task.clone() };
    let view = task_view(paused, None, &names, now(), &|_| false);
    assert!(view.needs_attention && view.upcoming.is_empty());
    let manual = ScheduledTask { is_enabled: false, paused_reason: Some("已手动暂停".into()), ..task };
    assert!(!task_view(manual, None, &names, now(), &|_| false).needs_attention);
}

#[test]
fn test_status_text_maps_legacy_success_by_notify() {
    assert_eq!(status_text(RUN_LEGACY_SUCCESS, true), "已发送");
    assert_eq!(status_text(RUN_LEGACY_SUCCESS, false), "无新内容");
    assert_eq!(status_text(RUN_SKIPPED, false), "跳过");
}
