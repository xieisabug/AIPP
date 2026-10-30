//! A device that only has install defaults may join an existing account; a
//! device with real user data still cannot upload without a baseline.
use super::*;
use crate::db::assistant_db::AssistantDatabase;
use crate::db::llm_db::LLMDatabase;
use crate::db::mcp_db::MCPDatabase;
use crate::db::system_db::SystemDatabase;

/// All synced tables in one connection, initialised by the real install code.
fn fresh_install() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    let llm = LLMDatabase { conn };
    llm.create_tables().unwrap(); // seeds the official providers
    let assistant = AssistantDatabase { conn: llm.conn, mcp_conn: Connection::open_in_memory().unwrap() };
    assistant.create_tables().unwrap(); // seeds the quick assistant + prompt/model/configs
    let system = SystemDatabase { conn: assistant.conn };
    system.create_tables().unwrap();
    system.init_feature_config().unwrap(); // default summary settings
    let mcp = MCPDatabase { conn: system.conn };
    mcp.create_tables().unwrap();
    // Built-in tool sets, as init_builtin_mcp_servers writes them.
    let server = mcp.upsert_mcp_server_with_builtin("搜索", Some("builtin"), "stdio", Some("aipp:search"),
        None, None, None, Some(20000), false, true, true, false, false).unwrap();
    let tools = crate::mcp::builtin_mcp::get_builtin_tools_for_command("aipp:search");
    assert!(!tools.is_empty());
    for tool in &tools {
        mcp.upsert_mcp_server_tool(server, &tool.name, Some(&tool.description), Some(&tool.input_schema.to_string())).unwrap();
    }
    let conn = mcp.conn;
    // Conversation/artifact tables start empty on a fresh install.
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS conversation(id INTEGER PRIMARY KEY, name TEXT, assistant_id INTEGER, parent_butler_conversation_id INTEGER);
        CREATE TABLE IF NOT EXISTS message(id INTEGER PRIMARY KEY, conversation_id INTEGER, message_type TEXT, content TEXT, finish_time TEXT);
        CREATE TABLE IF NOT EXISTS message_attachment(id INTEGER PRIMARY KEY, message_id INTEGER);
        CREATE TABLE IF NOT EXISTS conversation_summary(id INTEGER PRIMARY KEY, conversation_id INTEGER);
        CREATE TABLE IF NOT EXISTS conversation_todo(id INTEGER PRIMARY KEY, conversation_id INTEGER);
        CREATE TABLE IF NOT EXISTS artifacts_collection(id INTEGER PRIMARY KEY, name TEXT, assistant_id INTEGER);").unwrap();
    conn
}

fn is_empty(data: &Connection) -> bool {
    seed::local_scope_is_empty(|_| Ok(data)).unwrap()
}

fn sync_db() -> Connection {
    let sync = Connection::open_in_memory().unwrap();
    sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    sync
}

#[test]
fn test_fresh_install_only_has_seed_rows_and_may_join_by_pulling() {
    let data = fresh_install();
    // The seeds are really there (this is what used to block the join).
    for table in ["assistant_prompt", "assistant_model", "assistant_model_config", "mcp_server_tool", "feature_config"] {
        let n: i64 = data.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
        assert!(n > 0, "{table} should be seeded");
    }
    assert!(is_empty(&data), "a device with only install defaults is an empty sync scope");
    let sync = sync_db();
    ensure_initial_sync_is_safe(&sync, false, is_empty(&data)).unwrap();
    assert!(!has_local_identities(&sync).unwrap(), "fresh device pulls before scanning");
}

#[test]
fn test_fresh_device_without_api_keys_still_joins_by_pulling() {
    let data = fresh_install();
    // Key rows saved on official providers (blank or not yet synced) are the
    // encrypted-key object type; they must not make the device look populated.
    data.execute_batch("INSERT INTO llm_provider_config(name, llm_provider_id, value) VALUES ('api_key', 1, '');
        INSERT INTO llm_provider_config(name, llm_provider_id, value) VALUES ('api_key', 2, '');").unwrap();
    let secret_spec = specs().into_iter().find(|s| s.object_type == secrets::OBJECT_TYPE).unwrap();
    assert!(seed::user_row_count(&data, &secret_spec).unwrap() > 0, "key rows are present");
    assert!(is_empty(&data), "API-key rows alone do not count as user data");
    ensure_initial_sync_is_safe(&sync_db(), false, is_empty(&data)).unwrap();
}

#[test]
fn test_device_with_real_user_data_is_not_empty_and_stays_blocked() {
    let edits: &[(&str, &str)] = &[
        ("custom provider", "INSERT INTO llm_provider(name, api_type, is_official) VALUES ('My relay', 'openai_api', 0)"),
        ("quick prompt edited", "UPDATE assistant_prompt SET prompt='Answer in Chinese.' WHERE assistant_id=1"),
        ("quick model chosen", "UPDATE assistant_model SET provider_id=1, model_code='gpt-4o' WHERE assistant_id=1"),
        ("temperature changed", "UPDATE assistant_model_config SET value='0.2' WHERE assistant_id=1 AND name='temperature'"),
        ("builtin tool auto-run", "UPDATE mcp_server_tool SET is_auto_run=1 WHERE id=(SELECT MIN(id) FROM mcp_server_tool)"),
        ("summary prompt edited", "UPDATE feature_config SET value='自定义' WHERE feature_code='conversation_summary' AND key='prompt'"),
        ("other setting", "INSERT INTO feature_config(feature_code, key, value, data_type) VALUES ('display', 'theme', 'dark', 'string')"),
        ("own assistant", "INSERT INTO assistant(name, description) VALUES ('写作助手', '')"),
        ("conversation", "INSERT INTO conversation(name, assistant_id) VALUES ('history', 1)"),
    ];
    for (what, sql) in edits {
        let data = fresh_install();
        data.execute_batch(sql).unwrap();
        assert!(!is_empty(&data), "{what} is user data");
        // Joining an account that already has data still needs the baseline.
        let err = ensure_initial_sync_is_safe(&sync_db(), false, is_empty(&data)).unwrap_err();
        assert!(err.contains("同步基线"), "{what}: {err}");
    }
}

// ---- end-to-end join: pull first, seeds merge, nothing duplicated is uploaded ----

const JOIN_TYPES: &[&str] = &["llm.provider", "assistant", "assistant.prompt", "assistant.model", "assistant.model_config", "system.feature_config"];

fn scan(sync: &Connection, data: &Connection, device: &str) -> Vec<LocalObjectSnapshot> {
    quick_assistant::prepare(sync, data).unwrap();
    seed::drop_superseded_seeds(sync, data).unwrap();
    let mut out = Vec::new();
    for spec in specs().into_iter().filter(|s| JOIN_TYPES.contains(&s.object_type)) {
        let (rows, _) = read_local_snapshots_from_db(data, &spec, sync, &mut DanglingFkScanReport::default(), |kind, id| {
            let spec = specs().into_iter().find(|s| s.object_type == kind).unwrap();
            row_exists_by_id(data, &spec, id)
        }).unwrap();
        for row in &rows { enqueue_snapshot_if_changed(sync, row, device).unwrap(); }
        out.extend(rows);
    }
    out
}

fn outbox(sync: &Connection) -> Vec<(String, String)> {
    sync.prepare("SELECT object_type, payload_json FROM sync_outbox").unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default()))).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap()
}

#[test]
fn test_fresh_device_joins_account_without_duplicating_install_seeds() {
    // Device A: the account owner, with a customised quick assistant.
    let (a_sync, a_data) = (sync_db(), fresh_install());
    a_data.execute_batch("UPDATE assistant_prompt SET prompt='Answer in Chinese.' WHERE assistant_id=1;
        UPDATE assistant_model_config SET value='0.3' WHERE assistant_id=1 AND name='temperature';").unwrap();
    let history: Vec<PullChange> = scan(&a_sync, &a_data, "device-a").into_iter().enumerate().map(|(i, row)| PullChange {
        seq: i as i64 + 1, event_id: format!("e{i}"), device_id: "device-a".into(), object_type: row.object_type,
        object_id: row.object_id, operation: "upsert".into(), version: 1,
        payload: Some(serde_json::from_str(&row.payload_json).unwrap()), deleted_at: None, created_at: "now".into(),
    }).collect();

    // Device B: a fresh install joins. It pulls first (no identities yet).
    let (b_sync, b_data) = (sync_db(), fresh_install());
    assert!(is_empty(&b_data) && !has_local_identities(&b_sync).unwrap());
    apply_pull_page(&b_sync, &history, |change| {
        let spec = specs().into_iter().find(|s| s.object_type == change.object_type).unwrap();
        apply_change_to_db(&b_sync, &b_data, &spec, change, "device-b", |_| Ok(()))
    }).unwrap();
    assert_eq!(seed::drop_superseded_seeds(&b_sync, &b_data).unwrap(), 1, "the default prompt yields to the pulled one");

    let prompts: Vec<String> = b_data.prepare("SELECT prompt FROM assistant_prompt WHERE assistant_id=1").unwrap()
        .query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    assert_eq!(prompts, vec!["Answer in Chinese.".to_string()]);
    let temperature: String = b_data.query_row("SELECT value FROM assistant_model_config WHERE assistant_id=1 AND name='temperature'", [], |r| r.get(0)).unwrap();
    assert_eq!(temperature, "0.3");
    for (table, n) in [("assistant_model_config", 4), ("assistant_model", 1), ("feature_config", 2)] {
        assert_eq!(b_data.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).unwrap(), n, "{table}");
    }

    // B's next scan uploads nothing: its seeds merged into A's rows.
    scan(&b_sync, &b_data, "device-b");
    let pending = outbox(&b_sync);
    assert!(pending.is_empty(), "fresh device must not upload duplicate seeds: {pending:?}");

    // Re-opening the app does not bring the default prompt back.
    let db = AssistantDatabase { conn: b_data, mcp_conn: Connection::open_in_memory().unwrap() };
    db.init_assistant().unwrap();
    assert_eq!(db.get_assistant_prompt(1).unwrap().len(), 1);
}

#[test]
fn test_seed_cleanup_never_touches_user_or_synced_prompts() {
    let (sync, data) = (sync_db(), fresh_install());
    // No synced prompt yet: the default stays (fresh device before any pull).
    assert_eq!(seed::drop_superseded_seeds(&sync, &data).unwrap(), 0);
    // Once scanned (synced identity), the default prompt is the user's own and stays.
    scan(&sync, &data, "device-a");
    data.execute("INSERT INTO assistant_prompt(assistant_id, prompt) VALUES (1, 'You are a helpful assistant.')", []).unwrap();
    data.execute("INSERT INTO assistant_prompt(assistant_id, prompt) VALUES (1, 'my own second prompt')", []).unwrap();
    assert_eq!(seed::drop_superseded_seeds(&sync, &data).unwrap(), 1, "only the unsynced pristine copy");
    let left: i64 = data.query_row("SELECT COUNT(*) FROM assistant_prompt WHERE assistant_id=1", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 2);
}
