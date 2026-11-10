use super::*;
use crate::api::ai::conversation::{build_message_list_with_metadata_from_db, BranchSelection};
use crate::db::conversation_db::{
    archive_butler_contact_conversation, ensure_conversation_table, ConversationRepository, BUTLER_CONTACT_ARCHIVE_KIND,
};
use rusqlite::Connection;
use crate::db::conversation_db::{Message, MessageRepository};

fn row(id: i64, kind: &str, text: &str) -> (Message, Option<crate::db::conversation_db::MessageAttachment>) {
    (Message {
        id, parent_id: None, conversation_id: 1, message_type: kind.into(), content: text.into(),
        llm_model_id: None, llm_model_name: None, created_time: chrono::DateTime::from_timestamp(id, 0).unwrap(),
        start_time: None, finish_time: None, token_count: 0, input_token_count: 7000, output_token_count: 0,
        generation_group_id: None, parent_group_id: None, tool_calls_json: None, metadata_json: None,
        first_token_time: None, ttft_ms: None,
    }, None)
}

#[test]
fn test_butler_contact_checkpoint_replays_same_prefix_after_append_and_edit() {
    let body = vec![("user".into(), "original".into(), vec![])];
    let summary = "<context_summary>one</context_summary>";
    let mut history = vec![row(1, "system", "stable"), row(2,"user","original"), row(3,"response","tail"),
        row(4,"system",&checkpoint_content(summary, &[2], &body)), row(5,"user","next")];
    let (msgs, meta) = build_message_list_with_metadata_from_db(&history, BranchSelection::LatestBranch);
    assert_eq!(meta.message_ids, [1,4,3,5]);
    assert_eq!(msgs.iter().map(|m| m.1.as_str()).collect::<Vec<_>>(), ["stable",summary,"tail","next"]);
    assert_eq!(meta.db_token_counts, [0; 4]); // per-request input must never be summed as per-message usage
    let second = "<context_summary>two</context_summary>";
    history.push(row(6,"system",&checkpoint_content(second,&[4,3],&msgs[1..3])));
    history.push(row(7,"user","again"));
    let (msgs, meta) = build_message_list_with_metadata_from_db(&history,BranchSelection::LatestBranch);
    assert_eq!(meta.message_ids,[1,6,5,7]);
    assert_eq!(msgs[1].1,second);
    history[1].0.content = "edited".into();
    let (_, meta) = build_message_list_with_metadata_from_db(&history,BranchSelection::LatestBranch);
    assert_eq!(meta.message_ids,[1,2,3,5,7]);
}

#[test]
fn test_butler_contact_open_reuses_only_the_assistants_private_chat() {
    let conn = Connection::open_in_memory().unwrap();
    ensure_conversation_table(&conn).unwrap();
    conn.execute("INSERT INTO conversation(name, assistant_id, conversation_kind) VALUES ('task',8,'butler_task')",[]).unwrap();
    let repo = ConversationRepository::new(conn);
    let id = repo.open_butler_contact(8,"writer").unwrap();
    assert_ne!(id,1);
    assert_eq!(id,repo.open_butler_contact(8,"renamed").unwrap());
    assert_ne!(id,repo.open_butler_contact(9,"writer").unwrap());
}

#[test]
fn test_butler_contact_checkpoint_retry_and_history_scope() {
    let conn = Connection::open_in_memory().unwrap();
    ensure_conversation_table(&conn).unwrap();
    conn.execute_batch(r#"CREATE TABLE message (id INTEGER PRIMARY KEY, conversation_id INTEGER,
        message_type TEXT, content TEXT, created_time TEXT, token_count INTEGER, input_token_count INTEGER,
        output_token_count INTEGER, metadata_json TEXT);
        CREATE TABLE butler_main_state(butler_conversation_id INTEGER);
        INSERT INTO conversation (id,name,assistant_id,conversation_kind) VALUES (1,'ours',8,'butler_contact'),(2,'other',9,'butler_contact');
        INSERT INTO message (id,conversation_id,message_type,content,input_token_count,metadata_json) VALUES
        (1,1,'user','cedar ours',0,NULL),(2,2,'user','cedar other',0,NULL),
        (3,1,'response','first reply',100,NULL),(4,1,'response','second reply',100,'{"cached_input_tokens":0}');"#).unwrap();
    let repo = MessageRepository::new(conn);
    let header = "<!-- compacted_ids: test -->";
    let first = format!("{header}\n<context_summary>first</context_summary>");
    assert_eq!(repo.get_or_create_checkpoint(1,header,&first).unwrap(),first);
    assert_eq!(repo.get_or_create_checkpoint(1,header,&format!("{header}\nsecond")).unwrap(),first);
    let hits = repo.search_own_history(1,"cedar").unwrap();
    assert_eq!(hits.len(),1);
    assert_eq!(hits[0]["excerpt"],"cedar ours");
    let activity = repo.recent_cache_activity(1).unwrap();
    assert_eq!(activity.len(),3);
    assert_eq!(activity[0]["checkpoint"],true);
    assert_eq!(activity[1]["cached"],0);
    assert!(activity[2]["cached"].is_null());
}

fn archive_database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    ensure_conversation_table(&conn).unwrap();
    conn.execute_batch("CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER, message_type TEXT, content TEXT);
        CREATE TABLE queued_conversation_message(id INTEGER PRIMARY KEY, conversation_id INTEGER, status TEXT);
        CREATE TABLE butler_main_state(butler_conversation_id INTEGER);").unwrap();
    conn
}

fn say(conn: &Connection, conversation: i64, kind: &str, text: &str) {
    conn.execute("INSERT INTO message(conversation_id,message_type,content) VALUES (?1,?2,?3)",
        rusqlite::params![conversation, kind, text]).unwrap();
}

#[test]
fn test_butler_contact_archive_reopens_private_chat() {
    let conn = archive_database();
    let (none, first) = archive_butler_contact_conversation(&conn, 8, "writer", "unused").unwrap();
    assert!(none.is_empty(), "first reset only opens a chat");
    let (_, other) = archive_butler_contact_conversation(&conn, 9, "reviewer", "unused").unwrap();
    conn.execute_batch(&format!("INSERT INTO queued_conversation_message(conversation_id,status) VALUES
        ({first},'queued'),({first},'paused'),({first},'dispatching'),({other},'queued');")).unwrap();

    let (archived, second) = archive_butler_contact_conversation(&conn, 8, "writer", "与writer的私聊（存档 X）").unwrap();
    assert_eq!(archived, vec![first]);
    let row = |id: i64| conn.query_row("SELECT conversation_kind,name FROM conversation WHERE id=?1", [id],
        |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?))).unwrap();
    assert_eq!(row(first), (BUTLER_CONTACT_ARCHIVE_KIND.to_string(), "与writer的私聊（存档 X）".to_string()));
    assert_eq!(row(second), ("butler_contact".to_string(), "writer".to_string()));
    assert_eq!(row(other).0, "butler_contact", "other assistants keep their chat");
    let current: Vec<i64> = conn.prepare("SELECT id FROM conversation WHERE assistant_id=8 AND conversation_kind='butler_contact'")
        .unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(current, vec![second], "exactly one current private chat per assistant");
    let queued: Vec<(i64, String)> = conn.prepare("SELECT conversation_id,status FROM queued_conversation_message ORDER BY id")
        .unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(queued, vec![(first, "dispatching".to_string()), (other, "queued".to_string())],
        "unsent messages of the archived chat are dropped; in-flight dispatch finishes on its own");
}

#[test]
fn test_butler_own_history_includes_archived_chats_marked() {
    let conn = archive_database();
    let (_, old) = archive_butler_contact_conversation(&conn, 8, "writer", "unused").unwrap();
    let (_, foreign) = archive_butler_contact_conversation(&conn, 9, "reviewer", "unused").unwrap();
    say(&conn, old, "user", "cedar plan");
    say(&conn, foreign, "user", "cedar foreign");
    archive_butler_contact_conversation(&conn, 9, "reviewer", "reviewer archive").unwrap();
    let (_, current) = archive_butler_contact_conversation(&conn, 8, "writer", "与writer的私聊（存档 X）").unwrap();
    say(&conn, current, "response", "cedar now");
    conn.execute_batch("INSERT INTO conversation (id,name,assistant_id,conversation_kind) VALUES
        (40,'总管家主会话（存档 X）',1,'butler_main'),(41,'总管家主会话',1,'butler_main');
        INSERT INTO butler_main_state VALUES (41);").unwrap();
    say(&conn, 40, "user", "cedar old main");
    say(&conn, 41, "user", "cedar main");

    let repo = MessageRepository::new(conn);
    let hits = repo.search_own_history(current, "cedar").unwrap();
    let excerpts: Vec<&str> = hits.iter().map(|h| h["excerpt"].as_str().unwrap()).collect();
    assert_eq!(excerpts, ["cedar now", "cedar plan"], "only this assistant's chats");
    assert!(hits[0].get("archived").is_none());
    assert_eq!(hits[1]["archived"], true);
    assert_eq!(hits[1]["conversation_id"], old);
    assert_eq!(hits[1]["conversation_name"], "与writer的私聊（存档 X）");

    let main = repo.search_own_history(41, "cedar").unwrap();
    let excerpts: Vec<&str> = main.iter().map(|h| h["excerpt"].as_str().unwrap()).collect();
    assert_eq!(excerpts, ["cedar main", "cedar old main"]);
    assert_eq!(main[1]["archived"], true);
}
