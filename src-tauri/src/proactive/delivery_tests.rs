use super::*;
use crate::proactive::tests::db;

#[test]
fn test_proactive_last_supported_lunar_year_still_delivers() {
    let db = db();
    let date = crate::proactive::lunar::anniversary(2100, 12, false, 30).unwrap();
    db.execute("INSERT INTO butler_proactive_item(uuid,kind,title,source_conversation_id,event_date,yearly,calendar_type,next_check_at,updated_at)
        VALUES('last','milestone','纪念日',10,'12-30',1,'lunar',?1,0)",[date.to_string()]).unwrap();
    let now = date.and_hms_opt(12, 0, 0).unwrap().and_local_timezone(Local).earliest().unwrap();
    schedule(&db, now, &Config::default()).unwrap();
    assert_eq!(
        db.query_row("SELECT stage FROM butler_proactive_delivery_queue", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "due"
    );
    assert!(db
        .query_row("SELECT next_check_at FROM butler_proactive_item", [], |r| r
            .get::<_, Option<String>>(0))
        .unwrap()
        .is_none());
}

#[test]
fn test_proactive_daily_budget_keeps_one_independent_blessing() {
    let db = db();
    let today = Local::now().date_naive();
    for _ in 0..3 {
        db.execute("INSERT INTO message(message_type,metadata_json) VALUES('response',json_object('origin','butler_proactive','blessing',0))",[]).unwrap();
    }
    assert!(!within_budget(&db, today, 3, false).unwrap());
    assert!(within_budget(&db, today, 3, true).unwrap());
    db.execute("INSERT INTO message(message_type,metadata_json) VALUES('response',json_object('origin','butler_proactive','blessing',1))",[]).unwrap();
    assert!(!within_budget(&db, today, 3, true).unwrap());
}
use chrono::TimeZone;

#[test]
fn test_proactive_night_queue_keeps_separate_annual_stages() {
    let db = db();
    let config = Config::default();
    db.execute_batch("INSERT INTO butler_proactive_item(uuid,kind,title,source_conversation_id,event_date,yearly,lead_days,next_check_at,updated_at)
        VALUES('x','milestone','生日',10,'11-25',1,2,'2026-11-23',0);").unwrap();
    let night = Local.with_ymd_and_hms(2026, 11, 23, 23, 10, 0).earliest().unwrap();
    schedule(&db, night, &config).unwrap();
    schedule(&db, night, &config).unwrap();
    let release = Local.with_ymd_and_hms(2026, 11, 24, 8, 30, 0).earliest().unwrap();
    let after: i64 = db
        .query_row(
            "SELECT deliver_after FROM butler_proactive_delivery_queue WHERE stage='prepare'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after, release.timestamp());
    schedule(&db, night + Duration::days(2), &config).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM butler_proactive_delivery_queue", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row("SELECT next_check_at FROM butler_proactive_item", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "2027-11-23"
    );
}

#[test]
fn test_proactive_batch_delivery_is_atomic_and_backoff_counts_messages() {
    let mut db = db();
    let now = Local::now();
    let cfg = Config {
        quiet_start: "00:00".into(),
        quiet_end: "00:00".into(),
        ..Config::default()
    };
    db.execute_batch("INSERT INTO butler_proactive_item(uuid,kind,title,source_conversation_id,source_message_id,next_check_at,updated_at)
        VALUES('a','open_loop','联系医生',10,1,'2000-01-01',0),('b','prep_topic','已整理资料',10,1,'2000-01-01',0);
        UPDATE butler_proactive_item SET high_priority=1;").unwrap();
    schedule(&db, now, &cfg).unwrap();
    let batch = due(&db, true, false).unwrap();
    assert_eq!(batch.len(), 2);
    db.execute("UPDATE message SET content='联系医生' WHERE id=1", []).unwrap();
    let summary = fallback_summary(&db, &batch).unwrap();
    assert!(!summary.contains("联系医生"));
    assert!(summary.contains("已整理资料"));
    assert!(publish(&mut db, 999, &batch, &summary, None, &cfg, now.date_naive()).is_err());
    let message =
        publish(&mut db, 10, &batch, &summary, Some("原助手不可用"), &cfg, now.date_naive())
            .unwrap();
    let cards: i64 = db
        .query_row(
            "SELECT json_array_length(metadata_json,'$.cards') FROM message WHERE id=?1",
            [message],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cards, 2);
    let (_, external) = external_summary(&db).unwrap().unwrap();
    assert!(
        external.contains("2 项提醒")
            && !external.contains("联系医生")
            && !external.contains("已整理资料")
    );
    assert!(due(&db, true, false).unwrap().is_empty());
    assert!(!backoff(&db).unwrap());
    assert!(publish(&mut db, 10, &batch, &summary, None, &cfg, now.date_naive()).is_err());
    db.execute("INSERT INTO message(conversation_id,message_type,content,metadata_json) SELECT conversation_id,message_type,content,metadata_json FROM message WHERE id=?1",[message]).unwrap();
    assert!(backoff(&db).unwrap());
    db.execute(
        "INSERT INTO message(conversation_id,message_type,content) VALUES(10,'user','我回来了')",
        [],
    )
    .unwrap();
    assert!(!backoff(&db).unwrap());
}

#[test]
fn test_proactive_invalid_annual_item_does_not_block_healthy_reminder() {
    let db=db();
    db.execute_batch("INSERT INTO butler_proactive_item(uuid,kind,title,source_conversation_id,yearly,next_check_at,updated_at)
        VALUES('missing','milestone','缺少日期',10,1,'2000-01-01',0),('ok','open_loop','复查',10,0,'2000-01-01',0);").unwrap();
    schedule(&db,Local::now(),&Config::default()).unwrap();
    assert_eq!(db.query_row("SELECT count(*) FROM butler_proactive_delivery_queue",[],|r|r.get::<_,usize>(0)).unwrap(),1);
    assert!(db.query_row("SELECT summary FROM butler_proactive_run WHERE status='failed'",[],|r|r.get::<_,String>(0)).unwrap().contains("年度事项缺少日期"));
}

#[test]
fn test_proactive_quiet_boundary_rechecked_after_contact_wording() {
    let now=Local.with_ymd_and_hms(2026,11,23,22,29,55).earliest().unwrap();
    assert!(can_publish(&Config::default(),now.date_naive(),now));
    assert!(!can_publish(&Config::default(),now.date_naive(),now+Duration::seconds(10)));
}

#[test]
fn test_proactive_synced_final_brief_materialized_on_delivery() {
    let mut db=db();
    let cfg=Config{quiet_start:"00:00".into(),quiet_end:"00:00".into(),..Config::default()};
    db.execute_batch("INSERT INTO butler_proactive_item(uuid,kind,title,detail,source_conversation_id,source_message_id,next_check_at,updated_at)
        VALUES('from-other-device','prep_topic','成都行前准备','最新预研全文',10,1,'2000-01-01',0);").unwrap();
    let now=Local::now();
    schedule(&db,now,&cfg).unwrap();
    let rows=due(&db,true,false).unwrap();
    assert!(rows[0].brief.is_none());
    let id=publish(&mut db,10,&rows,"行前简报",None,&cfg,now.date_naive()).unwrap();
    let content:String=db.query_row("SELECT b.content FROM message m JOIN butler_proactive_delivery_queue q ON q.message_id=m.id
        JOIN message b ON q.brief_message_id=b.id AND b.id=json_extract(m.metadata_json,'$.cards[0].brief') WHERE m.id=?1",[id],|r|r.get(0)).unwrap();
    assert_eq!(content,"最新预研全文");
}
