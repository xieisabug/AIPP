use super::*;
use crate::api::ai::conversation::{build_message_list_with_metadata_from_db, BranchSelection};
use crate::db::conversation_db::{ensure_conversation_table, ConversationRepository};
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
    conn.execute_batch(r#"CREATE TABLE message (id INTEGER PRIMARY KEY, conversation_id INTEGER,
        message_type TEXT, content TEXT, created_time TEXT, token_count INTEGER, input_token_count INTEGER,
        output_token_count INTEGER, metadata_json TEXT);
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
