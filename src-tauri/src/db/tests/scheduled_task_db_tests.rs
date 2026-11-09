//! Production task writes must not recreate children after task deletion.
use super::*;

#[test]
fn test_cascade_delete_scheduled_task_rejects_late_logs_and_runs() {
    let db=ScheduledTaskDatabase { conn: Connection::open_in_memory().unwrap(), write_lock: Arc::new(Mutex::new(())) };
    db.create_tables().unwrap();
    db.conn.execute_batch("INSERT INTO scheduled_task(id,name,schedule_type,assistant_id,task_prompt,notify_prompt)
        VALUES(1,'delete','once',1,'work','notify'),(2,'keep','once',1,'work','notify');").unwrap();
    let log=ScheduledTaskLog { id:0,task_id:1,run_id:"run-1".into(),message_type:"start".into(),content:"log".into(),created_time:Utc::now() };
    let run=ScheduledTaskRun { id:0,task_id:1,run_id:"run-1".into(),status:"running".into(),notify:false,summary:None,error_message:None,
        started_time:Utc::now(),finished_time:None,butler_followup_status:None };
    db.add_log(&log).unwrap(); db.create_run(&run).unwrap();
    db.add_log(&ScheduledTaskLog{task_id:2,run_id:"run-2".into(),..log.clone()}).unwrap();
    db.delete_task(1).unwrap();
    assert!(db.add_log(&log).unwrap_err().to_string().contains("run-1"));
    assert!(db.create_run(&run).unwrap_err().to_string().contains("run-1"));
    assert!(db.list_logs_by_task(1,100).unwrap().is_empty());
    assert!(db.list_runs_by_task(1,100).unwrap().is_empty());
    assert_eq!(db.list_logs_by_task(2,100).unwrap().len(),1);
}

#[test]
fn test_mark_interrupted_runs_only_closes_orphan_running() {
    let db=ScheduledTaskDatabase { conn: Connection::open_in_memory().unwrap(), write_lock: Arc::new(Mutex::new(())) };
    db.create_tables().unwrap();
    db.conn.execute_batch("INSERT INTO scheduled_task(id,name,schedule_type,assistant_id,task_prompt,notify_prompt,is_enabled)
        VALUES(1,'once','once',1,'work','notify',0);").unwrap();
    let base=ScheduledTaskRun { id:0,task_id:1,run_id:"orphan".into(),status:"running".into(),notify:false,summary:None,error_message:None,
        started_time:Utc::now(),finished_time:None,butler_followup_status:Some("pending".into()) };
    db.create_run(&base).unwrap();
    db.create_run(&ScheduledTaskRun{run_id:"local".into(),..base.clone()}).unwrap();
    db.create_run(&ScheduledTaskRun{run_id:"done".into(),status:"success".into(),finished_time:Some(Utc::now()),..base.clone()}).unwrap();

    let closed=db.mark_interrupted_runs(&["local".to_string()],"应用退出，运行中断",Utc::now()).unwrap();
    assert_eq!(closed.len(),1);
    assert_eq!(closed[0].run_id,"orphan");
    assert_eq!(closed[0].status,"failed");
    assert!(closed[0].error_message.as_deref().unwrap().contains("run_id=orphan"));
    assert_eq!(closed[0].butler_followup_status.as_deref(),Some("pending"));

    let runs=db.list_runs_by_task(1,10).unwrap();
    let status=|id:&str| runs.iter().find(|r| r.run_id==id).unwrap().status.clone();
    assert_eq!(status("orphan"),"failed");
    assert_eq!(status("local"),"running");
    assert_eq!(status("done"),"success");
    // 再次执行不会重复收尾
    assert!(db.mark_interrupted_runs(&["local".to_string()],"x",Utc::now()).unwrap().is_empty());
}

/// 回报前或续跑中退出的已结束运行，重启后需要补报
#[test]
fn test_list_runs_awaiting_butler_followup() {
    let db=ScheduledTaskDatabase { conn: Connection::open_in_memory().unwrap(), write_lock: Arc::new(Mutex::new(())) };
    db.create_tables().unwrap();
    db.conn.execute_batch("INSERT INTO scheduled_task(id,name,schedule_type,assistant_id,task_prompt,notify_prompt)
        VALUES(1,'t','once',1,'work','notify');").unwrap();
    let base=ScheduledTaskRun { id:0,task_id:1,run_id:"a".into(),status:"success".into(),notify:false,summary:None,error_message:None,
        started_time:Utc::now(),finished_time:Some(Utc::now()),butler_followup_status:Some("pending".into()) };
    db.create_run(&base).unwrap();
    db.create_run(&ScheduledTaskRun{run_id:"b".into(),status:"failed".into(),butler_followup_status:Some("continuing".into()),..base.clone()}).unwrap();
    db.create_run(&ScheduledTaskRun{run_id:"c".into(),butler_followup_status:Some("done".into()),..base.clone()}).unwrap();
    db.create_run(&ScheduledTaskRun{run_id:"d".into(),status:"running".into(),finished_time:None,..base.clone()}).unwrap();
    db.create_run(&ScheduledTaskRun{run_id:"e".into(),butler_followup_status:None,..base.clone()}).unwrap();
    db.create_run(&ScheduledTaskRun{run_id:"f".into(),butler_followup_status:Some("failed".into()),..base.clone()}).unwrap();
    db.create_run(&ScheduledTaskRun{run_id:"g".into(),butler_followup_status:Some("retry".into()),..base.clone()}).unwrap();
    db.create_run(&ScheduledTaskRun{run_id:"h".into(),butler_followup_status:Some("exhausted".into()),..base.clone()}).unwrap();
    db.create_run(&ScheduledTaskRun{run_id:"i".into(),status:"running".into(),finished_time:None,butler_followup_status:Some("failed".into()),..base.clone()}).unwrap();
    let ids: Vec<String> = db.list_runs_awaiting_butler_followup().unwrap().into_iter().map(|r| r.run_id).collect();
    assert_eq!(ids, vec!["a".to_string(), "b".to_string(), "f".to_string(), "g".to_string()]);
}

fn outcome_test_db() -> ScheduledTaskDatabase {
    let db = ScheduledTaskDatabase {
        conn: Connection::open_in_memory().unwrap(),
        write_lock: Arc::new(Mutex::new(())),
    };
    db.create_tables().unwrap();
    db.conn.execute(
        "INSERT INTO scheduled_task(id, name, schedule_type, assistant_id, task_prompt, notify_prompt, butler_conversation_id)
         VALUES (1, 'outcome', 'once', 1, 'work', 'notify', 42)",
        [],
    ).unwrap();
    db.create_run(&ScheduledTaskRun {
        id: 0,
        task_id: 1,
        run_id: "outcome-run".into(),
        status: "running".into(),
        notify: false,
        summary: None,
        error_message: None,
        started_time: Utc::now(),
        finished_time: None,
        butler_followup_status: None,
    }).unwrap();
    db
}

/// 按 run_id 精确读取运行；缺失或已删除的运行返回 None。
#[test]
fn test_scheduled_run_read_existing_missing_and_deleted() {
    let db = outcome_test_db();
    let run = db.read_run("outcome-run").unwrap().unwrap();
    assert_eq!(run.task_id, 1);
    assert_eq!(run.run_id, "outcome-run");
    assert_eq!(run.status, "running");
    assert!(run.finished_time.is_none());
    assert!(run.butler_followup_status.is_none());
    assert!(db.read_run("missing-run").unwrap().is_none());

    db.delete_task(1).unwrap();
    assert!(db.read_run("outcome-run").unwrap().is_none());
}

/// 完整结果和终态一起保存；重启读取不能丢失执行详情、结束原因或通知判定诊断。
#[test]
fn test_scheduled_run_outcome_preserves_full_payload_and_queues_report() {
    let db = outcome_test_db();
    let finished = Utc::now();
    let outcome = serde_json::json!({
        "status": "failed",
        "end_reason": "timeout",
        "notify": false,
        "summary": null,
        "error": "执行达到总时限，部分工具操作已完成",
        "detail": "第一步已完成\n保留输出，不可从摘要重建。".repeat(400),
        "judged_task_state": "partial",
        "notify_error": "通知判定模型返回无效 JSON"
    });
    let payload = serde_json::to_string(&outcome).unwrap();
    assert!(db.read_run_outcome("outcome-run").unwrap().is_none());

    db.save_run_outcome(
        "outcome-run", "failed", false, None,
        Some("执行达到总时限，部分工具操作已完成"), Some(finished), &payload,
    ).unwrap();
    // Re-ensuring tables models startup schema initialization without replacing
    // the payload or requiring a file-backed database.
    db.create_tables().unwrap();

    assert_eq!(db.read_run_outcome("outcome-run").unwrap().as_deref(), Some(payload.as_str()));
    let saved: serde_json::Value = serde_json::from_str(
        &db.read_run_outcome("outcome-run").unwrap().unwrap(),
    ).unwrap();
    assert_eq!(saved, outcome);
    let run = db.list_runs_by_task(1, 10).unwrap().remove(0);
    assert_eq!(run.status, "failed");
    assert!(!run.notify);
    assert!(run.summary.is_none());
    assert_eq!(run.error_message.as_deref(), Some("执行达到总时限，部分工具操作已完成"));
    assert_eq!(run.finished_time, Some(finished));
    assert_eq!(run.butler_followup_status.as_deref(), Some("pending"));
    assert_eq!(db.list_runs_awaiting_butler_followup().unwrap().len(), 1);
}

/// 插入完整结果失败时，终态、完成时间和待回报状态也必须回滚。
#[test]
fn test_scheduled_run_outcome_insert_failure_rolls_back_terminal_update() {
    let db = outcome_test_db();
    db.conn.execute_batch(
        "CREATE TRIGGER reject_outcome_insert BEFORE INSERT ON scheduled_task_run_outcome
         BEGIN SELECT RAISE(ABORT, 'outcome insert rejected'); END;",
    ).unwrap();

    let error = db.save_run_outcome(
        "outcome-run", "success", true, Some("complete"), None, Some(Utc::now()),
        r#"{"status":"success","detail":"full output"}"#,
    ).unwrap_err();
    assert!(error.to_string().contains("outcome insert rejected"));
    let run = db.list_runs_by_task(1, 10).unwrap().remove(0);
    assert_eq!(run.status, "running");
    assert!(!run.notify);
    assert!(run.summary.is_none());
    assert!(run.error_message.is_none());
    assert!(run.finished_time.is_none());
    assert!(run.butler_followup_status.is_none());
    assert!(db.read_run_outcome("outcome-run").unwrap().is_none());
    assert!(db.list_runs_awaiting_butler_followup().unwrap().is_empty());
    assert!(db.conn.is_autocommit());
}

/// 重复保存支持 upsert，失败时保留旧结果，且不重开已确认的回报。
#[test]
fn test_scheduled_run_outcome_update_failure_preserves_saved_result() {
    let db = outcome_test_db();
    let finished = Utc::now();
    let original = r#"{"status":"success","detail":"original output","notify_error":"judgement failed"}"#;
    db.save_run_outcome(
        "outcome-run", "success", false, None, None, Some(finished), original,
    ).unwrap();
    db.update_run_butler_followup_status("outcome-run", "done").unwrap();
    // A successful repeat must not enqueue an already confirmed report again.
    db.save_run_outcome(
        "outcome-run", "success", false, None, None, Some(finished), original,
    ).unwrap();
    assert!(db.list_runs_awaiting_butler_followup().unwrap().is_empty());
    db.conn.execute_batch(
        "CREATE TRIGGER reject_outcome_update BEFORE UPDATE ON scheduled_task_run_outcome
         BEGIN SELECT RAISE(ABORT, 'outcome update rejected'); END;",
    ).unwrap();

    let error = db.save_run_outcome(
        "outcome-run", "failed", true, Some("different"), Some("new failure"), Some(Utc::now()),
        r#"{"status":"failed","detail":"replacement"}"#,
    ).unwrap_err();
    assert!(error.to_string().contains("outcome update rejected"));
    assert_eq!(db.read_run_outcome("outcome-run").unwrap().as_deref(), Some(original));
    let run = db.list_runs_by_task(1, 10).unwrap().remove(0);
    assert_eq!(run.status, "success");
    assert!(!run.notify);
    assert!(run.summary.is_none());
    assert!(run.error_message.is_none());
    assert_eq!(run.finished_time, Some(finished));
    assert_eq!(run.butler_followup_status.as_deref(), Some("done"));
    assert!(db.conn.is_autocommit());
}

/// 非管家任务仍保存完整结果，但不加入总管家回报队列。
#[test]
fn test_scheduled_run_outcome_without_butler_does_not_queue_report() {
    let db = outcome_test_db();
    db.conn.execute("UPDATE scheduled_task SET butler_conversation_id = NULL WHERE id = 1", []).unwrap();
    db.save_run_outcome(
        "outcome-run", "success", false, None, None, Some(Utc::now()),
        r#"{"status":"success","detail":"complete"}"#,
    ).unwrap();
    assert!(db.read_run_outcome("outcome-run").unwrap().is_some());
    assert!(db.list_runs_by_task(1, 10).unwrap()[0].butler_followup_status.is_none());
    assert!(db.list_runs_awaiting_butler_followup().unwrap().is_empty());
}

/// 删除任务同时清理完整结果，迟到的保存不能创建孤儿结果（不依赖外键开关）。
#[test]
fn test_scheduled_run_outcome_delete_cleans_payload_and_rejects_late_save() {
    for foreign_keys in [false, true] {
        let db = outcome_test_db();
        db.conn.pragma_update(None, "foreign_keys", foreign_keys).unwrap();
        db.save_run_outcome(
            "outcome-run", "success", false, None, None, Some(Utc::now()),
            r#"{"status":"success","detail":"complete"}"#,
        ).unwrap();
        db.delete_task(1).unwrap();

        assert!(db.read_run_outcome("outcome-run").unwrap().is_none());
        let error = db.save_run_outcome(
            "outcome-run", "success", false, None, None, Some(Utc::now()),
            r#"{"status":"success","detail":"late"}"#,
        ).unwrap_err();
        assert!(error.to_string().contains("outcome-run"));
        assert!(db.read_run_outcome("outcome-run").unwrap().is_none());
        assert!(db.conn.is_autocommit());
    }
}
