use super::*;
use crate::proactive::{delivery, tests::db};
use rusqlite::params;

fn trip(db: &Connection) -> i64 {
    db.execute("UPDATE message SET content='下个月去成都出差。我的私人备注。' WHERE id=1", [])
        .unwrap();
    db.execute("INSERT INTO butler_proactive_item(uuid,kind,title,source_conversation_id,source_message_id,owner_assistant_id,event_date,lead_days,next_check_at,updated_at)
        VALUES('trip','prep_topic','成都行程',10,1,8,?1,2,?2,0)",params![(Local::now().date_naive()+Duration::days(2)).to_string(),Local::now().date_naive().to_string()]).unwrap();
    let id = db.last_insert_rowid();
    let primary = delta::quote(db, &Config::default(), 10, 1, "去成都出差").unwrap().unwrap();
    link(db, id, &primary, &primary.text).unwrap();
    id
}

#[test]
fn test_proactive_wake_without_delta_and_quiet_result_survives_next_tick() {
    let mut db = db();
    let id = trip(&db);
    let cfg = Config::default();
    let all = delta::read(&db, &cfg, true).unwrap();
    delta::advance(&db, &all, true).unwrap();
    delivery::schedule(&db, Local::now(), &cfg).unwrap();
    assert!(delta::read(&db, &cfg, true).unwrap().is_empty());
    assert!(pending(&db, &cfg).unwrap());
    let queue =
        db.query_row("SELECT id FROM butler_proactive_delivery_queue", [], |r| r.get(0)).unwrap();
    let key: String = db
        .query_row("SELECT occurrence_key FROM butler_proactive_delivery_queue", [], |r| r.get(0))
        .unwrap();
    let refs = sources(&db, &cfg, id).unwrap();
    let outcome = || Brief { notify: false, summary: "".into(), brief: "已核实行程".into() };
    assert_eq!(finish_wake(&mut db, &refs, queue, id, 1, &key, outcome()).unwrap(), 1);
    delivery::schedule(&db, Local::now(), &cfg).unwrap();
    assert!(!pending(&db, &cfg).unwrap());
    assert_eq!(finish_wake(&mut db, &refs, queue, id, 1, &key, outcome()).unwrap(), 0);
    assert_eq!(
        db.query_row("SELECT status FROM butler_proactive_delivery_queue", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "quiet"
    );
}

#[test]
fn test_proactive_cross_chat_reschedule_cancel_and_delegate_scope() {
    let db = db();
    let id = trip(&db);
    let cfg = Config::default();
    delivery::schedule(&db, Local::now(), &cfg).unwrap();
    db.execute("UPDATE butler_chat_group_message SET body='成都行程改到下周。另一个项目明天发货。' WHERE seq=1",[]).unwrap();
    let rows = delta::read(&db, &cfg, true).unwrap();
    let next = (Local::now().date_naive() + Duration::days(7)).to_string();
    let change: Change = serde_json::from_value(
        json!({"item":id,"title":"成都行程","summary":"行程改为下周","date":next,
        "sources":[{"source":1,"quote":"成都行程改到下周"}]}),
    )
    .unwrap();
    apply(&db, &active(&db, &cfg).unwrap(), &rows, &[], &[change]).unwrap();
    let (when,owner,queued):(String,Option<i64>,usize)=db.query_row("SELECT event_date,owner_assistant_id,
        (SELECT count(*) FROM butler_proactive_delivery_queue WHERE status='queued') FROM butler_proactive_item",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!((when, owner, queued), (next, None, 0));
    let refs = sources(&db, &cfg, id).unwrap();
    assert_eq!(refs.len(), 2);
    assert!(refs.iter().all(|s| !s.text.contains("另一个") && !s.text.contains("备注")));
    let requested = Citation { source: 0, quote: "另一个项目".into() };
    assert!(cited(&refs, &[requested]).is_err());
    let private_only = Config { groups: false, ..cfg.clone() };
    assert_eq!(sources(&db, &private_only, id).unwrap().len(), 1);
    assert_eq!(active(&db, &cfg).unwrap()[0].tracking_summary, "行程改为下周");
    assert!(active(&db, &private_only).unwrap()[0].tracking_summary.is_empty());
    db.execute("UPDATE butler_chat_group_message SET body='成都行程取消' WHERE seq=1", []).unwrap();
    let rows = delta::read(&db, &cfg, true).unwrap();
    let cancel=serde_json::from_value(json!({"item":id,"title":"成都行程","cancelled":true,"sources":[{"source":1,"quote":"成都行程取消"}]})).unwrap();
    apply(&db, &active(&db, &cfg).unwrap(), &rows, &[], &[cancel]).unwrap();
    assert!(active(&db, &cfg).unwrap().is_empty());
    assert!(!pending(&db, &cfg).unwrap());
}

#[test]
fn test_proactive_same_quote_allows_two_findings_and_exact_replay() {
    let db = db();
    let cfg = Config::default();
    let source = delta::read(&db, &cfg, true).unwrap().remove(0);
    for title in ["预约复诊", "取化验报告", "预约复诊"] {
        let f = serde_json::from_value(
            json!({"source":0,"quote":source.text,"kind":"open_loop","title":title}),
        )
        .unwrap();
        runner::persist(&db, &source, &f, 8, 10, &cfg).unwrap();
    }
    assert_eq!(
        db.query_row("SELECT count(*) FROM butler_proactive_item", [], |r| r.get::<_, usize>(0))
            .unwrap(),
        2
    );
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
    db.execute("INSERT INTO butler_chat_group_message(group_id,seq,kind,body) VALUES(12,2,'message','成都之行取消')",[]).unwrap();
    assert!(review_before_delivery(&db, &cfg).unwrap());
    let rows = delta::read(&db, &cfg, true).unwrap();
    let item = active(&db, &cfg).unwrap();
    let cancel =
        serde_json::from_value(json!({"item":item[0].id,"title":"成都出差","cancelled":true,
        "sources":[{"source":0,"quote":"成都之行取消"}]}))
        .unwrap();
    apply(&db, &item, &rows, &[], &[cancel]).unwrap();
    delta::advance(&db, &rows, true).unwrap();
    assert_eq!(
        db.query_row("SELECT status FROM butler_proactive_delivery_queue", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "cancelled"
    );
}

#[test]
fn test_proactive_digest_creates_one_shared_topic_from_two_new_conversations() {
    let db = db();
    let cfg = Config::default();
    db.execute("UPDATE message SET content='周五要去成都' WHERE id=1", []).unwrap();
    db.execute("UPDATE butler_chat_group_message SET body='成都会议改在西站旁' WHERE seq=1", [])
        .unwrap();
    let rows = delta::read(&db, &cfg, true).unwrap();
    let change =
        serde_json::from_value(json!({"title":"成都出差","summary":"周五赴成都，在西站旁开会。",
        "sources":[{"source":0,"quote":"周五要去成都"},{"source":1,"quote":"成都会议改在西站旁"}]}))
        .unwrap();
    apply(&db, &[], &rows, &[], &[change]).unwrap();
    let all = active(&db, &cfg).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(sources(&db, &cfg, all[0].id).unwrap().len(), 2);
    delivery::schedule(&db, Local::now(), &cfg).unwrap();
    assert!(pending(&db, &cfg).unwrap());
}

#[test]
fn test_proactive_rescheduling_cancels_snoozes_and_releases_unused_occurrence_keys() {
    let db = db();
    let item = trip(&db);
    db.execute("INSERT INTO butler_proactive_delivery_queue(item_id,occurrence_key,occurrence,stage,status,deliver_after,delivered_at)
        VALUES(?1,'snoozed','2026-11-24','due','queued',1,1),(?1,'quiet','2026-11-24','prepare','quiet',1,NULL)",[item]).unwrap();
    reset_schedule(&db, item).unwrap();
    let state: (usize, String) = db
        .query_row("SELECT count(*),status FROM butler_proactive_delivery_queue", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(state, (1, "cancelled".into()));
}

#[test]
fn test_proactive_same_statement_and_title_keeps_two_scheduled_dates() {
    let db = db();
    let cfg = Config::default();
    let row = delta::read(&db, &cfg, true).unwrap().remove(0);
    for date in ["2027-01-12", "2027-01-15", "2027-01-15"] {
        let f = serde_json::from_value(
            json!({"source":0,"quote":row.text,"kind":"open_loop","title":"复查","date":date}),
        )
        .unwrap();
        runner::persist(&db, &row, &f, 8, 10, &cfg).unwrap();
    }
    let dates = db
        .prepare("SELECT event_date FROM butler_proactive_item ORDER BY event_date")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(dates, ["2027-01-12", "2027-01-15"]);
}

#[test]
fn test_proactive_coordinator_search_only_releases_current_linked_quote() {
    let db = db();
    let cfg = Config::default();
    let item = trip(&db);
    db.execute("UPDATE message SET content='成都返程周三，另有私人备注' WHERE id=1", []).unwrap();
    db.execute("UPDATE message SET content='成都返程不开放的普通聊天' WHERE id=2", []).unwrap();
    db.execute("UPDATE message SET content='成都返程来自助手' WHERE id=3", []).unwrap();
    db.execute("UPDATE message SET content='成都返程后台转述' WHERE id=4", []).unwrap();
    let candidates = delta::search(&db, &cfg, "成都返程").unwrap();
    assert_eq!(candidates.len(), 1);
    let selected =
        cited(&candidates, &[Citation { source: 0, quote: "成都返程周三".into() }]).unwrap();
    let quote = &selected[0];
    assert!(read_linked(&db, &cfg, item, 1, quote).is_err());
    link(&db, item, quote, &quote.text).unwrap();
    assert_eq!(read_linked(&db, &cfg, item, 1, quote).unwrap(), "成都返程周三");
    assert!(read_linked(&db, &cfg, item, 1, &candidates[0]).is_err());
    assert!(
        read_linked(&db, &Config { private_chats: false, ..cfg.clone() }, item, 1, quote).is_err()
    );
    db.execute("UPDATE butler_proactive_item SET status='dismissed' WHERE id=?1", [item]).unwrap();
    assert!(read_linked(&db, &cfg, item, 1, quote).is_err());
}
