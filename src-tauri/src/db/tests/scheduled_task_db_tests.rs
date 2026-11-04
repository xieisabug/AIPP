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
