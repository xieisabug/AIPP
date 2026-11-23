use super::*;
use crate::proactive::{delivery, tests::db};
use chrono::Duration;
use rusqlite::params;

fn trip(db: &Connection) -> i64 {
    db.execute("UPDATE message SET content='下个月去成都出差。我的私人备注。' WHERE id=1", [])
        .unwrap();
    db.execute(
        "INSERT INTO butler_proactive_item(uuid,kind,title,source_conversation_id,source_message_id,owner_assistant_id,event_date,lead_days,next_check_at,updated_at)
        VALUES('trip','prep_topic','成都行程',10,1,8,?1,2,?2,0)",
        params![
            (Local::now().date_naive() + Duration::days(2)).to_string(),
            Local::now().date_naive().to_string()
        ],
    )
    .unwrap();
    let id = db.last_insert_rowid();
    let primary = delta::quote(db, &Config::default(), 10, 1, "去成都出差").unwrap().unwrap();
    link(db, id, &primary, &primary.text).unwrap();
    id
}

#[test]
fn test_proactive_wake_without_delta_and_quiet_result_survives_next_tick() {
    let db = db();
    let _id = trip(&db);
    let cfg = Config::default();
    let all = delta::read(&db, &cfg, true).unwrap();
    delta::advance(&db, &all, true).unwrap();
    delivery::schedule(&db, Local::now(), &cfg).unwrap();
    assert!(delta::read(&db, &cfg, true).unwrap().is_empty());
    assert!(pending(&db, &cfg).unwrap());
    let (stages, jobs) = waking_jobs(&db, &cfg, None).unwrap();
    assert_eq!(stages.len(), 1);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["title"], "成都行程");
    finalize_woken(&db, &stages).unwrap();
    delivery::schedule(&db, Local::now(), &cfg).unwrap();
    assert!(!pending(&db, &cfg).unwrap());
    assert_eq!(
        db.query_row("SELECT status FROM butler_proactive_delivery_queue", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "quiet"
    );
}

#[test]
fn test_proactive_reschedule_during_wake_preserves_replacement_stage() {
    let db = db();
    let id = trip(&db);
    let cfg = Config::default();
    let now = Local::now();
    delivery::schedule(&db, now, &cfg).unwrap();
    let (original, _) = waking_jobs(&db, &cfg, None).unwrap();
    let original_id: i64 = db.query_row("SELECT id FROM butler_proactive_delivery_queue", [], |r| r.get(0)).unwrap();
    let new_date = (now.date_naive() + Duration::days(1)).to_string();
    // First change the date, then change just the lead time (the occurrence key stays the same).
    let mut snapshot = original;
    for lead in [2, 3] {
        db.execute("UPDATE butler_proactive_item SET event_date=?2,lead_days=?3,next_check_at=?4,revision=revision+1 WHERE id=?1",
            params![id, new_date, lead, now.date_naive().to_string()]).unwrap();
        reset_schedule(&db, id).unwrap();
        delivery::schedule(&db, now, &cfg).unwrap();
        finalize_woken(&db, &snapshot).unwrap();
        let (replacement, jobs) = waking_jobs(&db, &cfg, Some(id)).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["occurrence"], new_date);
        assert_eq!(jobs[0]["stage"], "prepare");
        assert_ne!(replacement, snapshot);
        assert_eq!(db.query_row("SELECT id FROM butler_proactive_delivery_queue", [], |r| r.get::<_, i64>(0)).unwrap(), original_id);
        snapshot = replacement;
    }
    finalize_woken(&db, &snapshot).unwrap();
    assert!(!pending(&db, &cfg).unwrap());
}

#[test]
fn test_proactive_release_waits_for_messages_since_last_digest() {
    let db = db();
    let cfg = Config::default();
    trip(&db);
    delivery::schedule(&db, Local::now(), &cfg).unwrap();
    db.execute("UPDATE butler_proactive_delivery_queue SET researched=1,deliver_after=0", [])
        .unwrap();
    let rows = delta::read(&db, &cfg, true).unwrap();
    delta::advance(&db, &rows, true).unwrap();
    assert!(!review_before_delivery(&db, &cfg).unwrap());
    db.execute(
        "INSERT INTO butler_chat_group_message(group_id,seq,kind,body) VALUES(12,2,'message','成都之行取消')",
        [],
    )
    .unwrap();
    assert!(review_before_delivery(&db, &cfg).unwrap());
}

#[test]
fn test_proactive_rescheduling_cancels_snoozes_and_releases_unused_occurrence_keys() {
    let db = db();
    let item = trip(&db);
    db.execute(
        "INSERT INTO butler_proactive_delivery_queue(item_id,occurrence_key,occurrence,stage,status,deliver_after,delivered_at)
        VALUES(?1,'snoozed','2026-11-24','due','queued',1,1),(?1,'quiet','2026-11-24','prepare','quiet',1,NULL)",
        [item],
    )
    .unwrap();
    reset_schedule(&db, item).unwrap();
    let state: (usize, String) = db
        .query_row("SELECT count(*),status FROM butler_proactive_delivery_queue", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(state, (1, "cancelled".into()));
}

#[test]
fn test_proactive_search_respects_scope_and_hides_assistant_or_relayed_messages() {
    let db = db();
    let cfg = Config::default();
    db.execute("UPDATE message SET content='成都返程周三，另有私人备注' WHERE id=1", []).unwrap();
    db.execute("UPDATE message SET content='成都返程不开放的普通聊天' WHERE id=2", []).unwrap();
    db.execute("UPDATE message SET content='成都返程来自助手' WHERE id=3", []).unwrap();
    db.execute("UPDATE message SET content='成都返程后台转述' WHERE id=4", []).unwrap();
    let candidates = delta::search(&db, &cfg, "成都返程").unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].conversation, 10);
    let private_off = Config { private_chats: false, ..cfg };
    assert!(delta::search(&db, &private_off, "成都返程").unwrap().is_empty());
}
