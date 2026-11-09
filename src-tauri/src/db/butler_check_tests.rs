use super::*;
fn p()->CheckPolicy { CheckPolicy {mode:"read_only".into(),sources:vec!["https://example.com/check".into()],expires_at:"2099-01-01T00:00:00Z".into(),paused:false,stopped:false,stop_current_on_pause:true} }
#[test] fn test_check_missing_contract_denies_execution(){assert!(ensure_active(None,Utc::now()).is_err());}
#[test] fn test_check_modes_and_expiry_are_enforced(){let mut p=p();p.mode="send".into();assert!(validate(&p,Utc::now()).is_err());p.mode="draft".into();assert!(validate(&p,Utc::now()).is_ok());p.expires_at="2000-01-01T00:00:00Z".into();assert!(ensure_active(Some(&p),Utc::now()).is_err());}
#[test] fn test_check_readonly_exact_sources(){let p=p();assert!(ensure_tool(&p,true,"aipp:search","fetch_url",r#"{"url":"https://example.com/check"}"#,Utc::now()).is_ok());assert!(ensure_tool(&p,true,"aipp:search","fetch_url",r#"{"url":"https://other.com"}"#,Utc::now()).is_err());assert!(ensure_tool(&p,false,"aipp:search","fetch_url",r#"{"url":"https://example.com/check"}"#,Utc::now()).is_err());assert!(ensure_tool(&p,true,"aipp:operation","write_file","{}",Utc::now()).is_err());}
#[test] fn test_check_pause_and_close_survive_reload(){let conn=Connection::open_in_memory().unwrap();let mut p=p();p.paused=true;save(&conn,3,&p).unwrap();assert!(ensure_active(read(&conn,3).unwrap().as_ref(),Utc::now()).is_err());p.paused=false;p.stopped=true;save(&conn,3,&p).unwrap();assert!(ensure_active(read(&conn,3).unwrap().as_ref(),Utc::now()).is_err());assert!(read(&conn,4).unwrap().is_none());}

fn policy_fixture() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TABLE scheduled_task(id INTEGER PRIMARY KEY,butler_conversation_id INTEGER,is_enabled INTEGER,updated_time TEXT);
        CREATE TABLE scheduled_task_run(task_id INTEGER,status TEXT);
        INSERT INTO scheduled_task VALUES(3,10,0,'old');").unwrap();
    c
}
#[test]
fn test_check_running_admission_blocks_reconfiguration() {
    let mut c = policy_fixture();
    configure(&mut c,3,&p(),Utc::now()).unwrap();
    c.execute("INSERT INTO scheduled_task_run VALUES(3,'running')",[]).unwrap();
    let mut new = p(); new.sources = vec!["https://new.example".into()];
    assert!(configure(&mut c,3,&new,Utc::now()).is_err());
    assert_eq!(read(&c,3).unwrap().unwrap().sources,p().sources);
    c.execute("UPDATE scheduled_task_run SET status='success'",[]).unwrap();
    configure(&mut c,3,&new,Utc::now()).unwrap();
    assert_eq!(read(&c,3).unwrap().unwrap().sources,new.sources);
}
#[test]
fn test_check_policy_and_switch_roll_back_together() {
    let mut c = policy_fixture();
    configure(&mut c,3,&p(),Utc::now()).unwrap();
    c.execute_batch("CREATE TRIGGER reject_switch BEFORE UPDATE ON scheduled_task BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(control(&mut c,3,"stop",true,Utc::now()).is_err());
    assert!(!read(&c,3).unwrap().unwrap().stopped);
    assert!(c.query_row("SELECT is_enabled FROM scheduled_task",[],|r|r.get::<_,bool>(0)).unwrap());
}
#[test]
fn test_check_control_keeps_running_contract_and_rejects_ordinary_schedule() {
    let mut c = policy_fixture();
    configure(&mut c,3,&p(),Utc::now()).unwrap();
    c.execute("INSERT INTO scheduled_task_run VALUES(3,'running')",[]).unwrap();
    control(&mut c,3,"pause",false,Utc::now()).unwrap();
    let policy=read(&c,3).unwrap().unwrap();
    assert!(policy.paused); assert_eq!(policy.sources,p().sources);
    assert!(!c.query_row("SELECT is_enabled FROM scheduled_task",[],|r|r.get::<_,bool>(0)).unwrap());
    c.execute("UPDATE scheduled_task SET butler_conversation_id=NULL",[]).unwrap();
    assert!(configure(&mut c,3,&p(),Utc::now()).is_err());
}
