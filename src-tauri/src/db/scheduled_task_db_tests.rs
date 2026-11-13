use super::*;

fn memory_db() -> ScheduledTaskDatabase {
    ScheduledTaskDatabase {
        conn: Connection::open_in_memory().unwrap(),
        write_lock: Arc::new(Mutex::new(())),
    }
}

fn legacy_table(db: &ScheduledTaskDatabase) {
    db.conn.execute_batch("\
        CREATE TABLE scheduled_task_run (\
            id INTEGER PRIMARY KEY AUTOINCREMENT,\
            task_id INTEGER NOT NULL, run_id TEXT NOT NULL,\
            status TEXT NOT NULL CHECK(status IN ('running', 'success', 'failed')),\
            notify BOOLEAN NOT NULL DEFAULT 0, summary TEXT, error_message TEXT,\
            started_time DATETIME DEFAULT CURRENT_TIMESTAMP, finished_time DATETIME,\
            butler_followup_status TEXT, sync_tag TEXT DEFAULT 'kept'\
        );\
        CREATE INDEX run_sync_tag ON scheduled_task_run(sync_tag);\
        CREATE TABLE sync_audit (run_id TEXT);\
        CREATE TRIGGER sync_run_insert AFTER INSERT ON scheduled_task_run \
        BEGIN INSERT INTO sync_audit VALUES (NEW.run_id); END;\
    ").unwrap();
}

fn add_task(db: &ScheduledTaskDatabase) {
    db.conn.execute("INSERT INTO scheduled_task(id, name, schedule_type, assistant_id, task_prompt, notify_prompt) VALUES (1, 'task', 'once', 1, 'prompt', 'notify')", []).unwrap();
}

fn missed(db: &ScheduledTaskDatabase, run_id: &str) -> ScheduledTaskRun {
    db.create_run(&ScheduledTaskRun {
        error_message: Some("missed at startup".into()),
        finished_time: Some(Utc::now()),
        ..ScheduledTaskRun::new(1, run_id, "missed", Utc::now())
    }).unwrap()
}

#[test]
fn test_scheduled_task_run_missed_new_schema() {
    let db = memory_db();
    db.create_tables().unwrap();
    add_task(&db);
    let run = missed(&db, "new-missed");
    assert!(run.id > 0);
    assert!(db.conn.execute("INSERT INTO scheduled_task_run(task_id, run_id, status) VALUES (1, 'bad', 'invalid')", []).is_err());
}

#[test]
fn test_scheduled_task_run_missed_legacy_migration_preserves_data_and_objects() {
    let db = memory_db();
    legacy_table(&db);
    db.conn.execute_batch("\
        INSERT INTO scheduled_task_run(id, task_id, run_id, status, summary, sync_tag) VALUES\
            (4, 1, 'old-running', 'running', 'one', 'tag1'),\
            (5, 1, 'old-success', 'success', 'two', 'tag2'),\
            (6, 1, 'old-failed', 'failed', 'three', 'tag3');\
        UPDATE sqlite_sequence SET seq=900 WHERE name='scheduled_task_run';\
    ").unwrap();
    let trigger: String = db.conn.query_row("SELECT sql FROM sqlite_master WHERE type='trigger' AND name='sync_run_insert'", [], |r| r.get(0)).unwrap();
    db.create_tables().unwrap();
    db.create_tables().unwrap();
    let rows: Vec<(i64, String, String, String)> = db.conn.prepare("SELECT id,status,summary,sync_tag FROM scheduled_task_run ORDER BY id").unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().collect::<Result<_>>().unwrap();
    assert_eq!(rows, vec![(4, "running".into(), "one".into(), "tag1".into()), (5, "success".into(), "two".into(), "tag2".into()), (6, "failed".into(), "three".into(), "tag3".into())]);
    let after: String = db.conn.query_row("SELECT sql FROM sqlite_master WHERE type='trigger' AND name='sync_run_insert'", [], |r| r.get(0)).unwrap();
    assert_eq!(after, trigger);
    assert_eq!(db.conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('run_sync_tag','idx_scheduled_task_run_run_id','idx_scheduled_task_run_task_time')", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
    assert_eq!(db.conn.query_row("SELECT count(*) FROM sync_audit", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
    add_task(&db);
    assert!(missed(&db, "old-missed").id > 900);
    assert_eq!(db.conn.query_row("SELECT count(*) FROM sync_audit WHERE run_id='old-missed'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert!(db.conn.execute("INSERT INTO scheduled_task_run(task_id, run_id, status) VALUES (1, 'bad', 'invalid')", []).is_err());
}

#[test]
fn test_scheduled_task_run_missed_migration_empty_preserves_sequence() {
    let db = memory_db();
    legacy_table(&db);
    db.conn.execute_batch("INSERT INTO scheduled_task_run(id, task_id, run_id, status) VALUES (500, 1, 'deleted', 'success'); DELETE FROM scheduled_task_run;").unwrap();
    db.create_tables().unwrap();
    add_task(&db);
    assert!(missed(&db, "after-empty").id > 500);
}

#[test]
fn test_scheduled_task_run_missed_migration_failure_preserves_legacy() {
    let db = memory_db();
    legacy_table(&db);
    db.conn.execute_batch("INSERT INTO scheduled_task_run(task_id, run_id, status) VALUES (1, 'old', 'success'); CREATE TABLE scheduled_task_run_status_migration (sentinel TEXT);").unwrap();
    assert!(db.create_tables().is_err());
    assert_eq!(db.conn.query_row("SELECT status FROM scheduled_task_run WHERE run_id='old'", [], |r| r.get::<_, String>(0)).unwrap(), "success");
    assert_eq!(db.conn.query_row("SELECT count(*) FROM sqlite_master WHERE name='sync_run_insert' AND type='trigger'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert!(db.conn.execute("INSERT INTO scheduled_task_run(task_id, run_id, status) VALUES (1, 'missed', 'missed')", []).is_err());
}

#[test]
fn test_scheduled_task_run_missed_migration_refuses_incoming_foreign_key() {
    let db = memory_db();
    legacy_table(&db);
    db.conn.execute_batch("CREATE TABLE dependent (run_id INTEGER REFERENCES scheduled_task_run(id)); INSERT INTO scheduled_task_run(id, task_id, run_id, status) VALUES (1, 1, 'old', 'success'); INSERT INTO dependent VALUES (1);").unwrap();
    assert!(db.create_tables().is_err());
    assert_eq!(db.conn.query_row("SELECT count(*) FROM dependent JOIN scheduled_task_run ON dependent.run_id=scheduled_task_run.id", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn test_scheduled_task_run_missed_migration_refuses_unknown_non_autoincrement_schema() {
    let db = memory_db();
    db.conn.execute_batch("CREATE TABLE scheduled_task_run (id INTEGER PRIMARY KEY, task_id INTEGER NOT NULL, run_id TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('running', 'success', 'failed')), notify BOOLEAN NOT NULL DEFAULT 0, summary TEXT, error_message TEXT, started_time DATETIME, finished_time DATETIME);").unwrap();
    let error = db.create_tables().unwrap_err().to_string();
    assert!(error.contains("without AUTOINCREMENT"), "{error}");
}

#[test]
fn test_scheduled_task_run_previous_missed_schema_gains_new_statuses() {
    let db = memory_db();
    db.conn.execute_batch("\
        CREATE TABLE scheduled_task_run (\
            id INTEGER PRIMARY KEY AUTOINCREMENT,\
            task_id INTEGER NOT NULL, run_id TEXT NOT NULL,\
            status TEXT NOT NULL CHECK(status IN ('running', 'success', 'failed', 'missed')),\
            notify BOOLEAN NOT NULL DEFAULT 0, summary TEXT, error_message TEXT,\
            started_time DATETIME DEFAULT CURRENT_TIMESTAMP, finished_time DATETIME\
        );\
        INSERT INTO scheduled_task_run(task_id, run_id, status) VALUES (1, 'old', 'missed');\
    ").unwrap();
    db.create_tables().unwrap();
    add_task(&db);
    for status in ["delivered", "quiet", "skipped", "cancelled"] {
        db.conn.execute("INSERT INTO scheduled_task_run(task_id, run_id, status) VALUES (1, ?1, ?1)", [status]).unwrap();
    }
    assert_eq!(db.conn.query_row("SELECT status FROM scheduled_task_run WHERE run_id='old'", [], |r| r.get::<_, String>(0)).unwrap(), "missed");
}
