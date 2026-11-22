//! 管家群聊：公开消息、成员与回复队列。
//!
//! 群聊像普通聊天软件一样只有一条公开消息流。每个成员在群里拥有独立的执行会话：
//! 轮到它回复时，把它上次回复以来其他人的公开发言整理成纯文本追加进这个会话，
//! 它的最终回复再作为公开消息发回群里。工具调用、思考与执行细节只留在成员自己的会话中。
//! 私聊和其他群的上下文不会进入群成员会话。
use anyhow::{ensure, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::Serialize;

use crate::utils::reply_quote;

pub const GROUP_KIND: &str = "butler_group";
pub const MEMBER_KIND: &str = "butler_group_member";
pub const MAX_MEMBERS: usize = 30;
pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_BODY_CHARS: usize = 20_000;
/// 从用户发言后的首次成员间接力开始计数。
const RELAY_REVIEW_INTERVAL: i64 = 10;
/// 用户要求成员讨论时最多进行的轮数。
pub const MAX_DISCUSSION_ROUNDS: i64 = 5;
/// 单次投递给成员的群消息上限；更早的消息只保留数量说明。
const INPUT_MESSAGE_LIMIT: usize = 80;
const INPUT_CHAR_LIMIT: usize = 60_000;
const INPUT_PER_MESSAGE_CHARS: usize = 20_000;
/// 成员引用原话时，向前查找的群消息条数。
const QUOTE_SEARCH_LIMIT: i64 = 200;
pub const EVERYONE_MENTIONS: [&str; 2] = ["所有人", "全体成员"];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS butler_chat_group (
    id INTEGER PRIMARY KEY REFERENCES conversation(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    seq INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS butler_chat_group_member (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id INTEGER NOT NULL REFERENCES butler_chat_group(id) ON DELETE CASCADE,
    assistant_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    conversation_id INTEGER NOT NULL UNIQUE REFERENCES conversation(id) ON DELETE CASCADE,
    joined_after INTEGER NOT NULL,
    delivered_through INTEGER NOT NULL,
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_butler_chat_group_active_member
    ON butler_chat_group_member(group_id, assistant_id) WHERE active=1;
CREATE TABLE IF NOT EXISTS butler_chat_group_message (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id INTEGER NOT NULL REFERENCES butler_chat_group(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    sender_member INTEGER,
    sender_assistant INTEGER,
    sender_name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('message','notice','error')),
    body TEXT NOT NULL,
    reply_to INTEGER,
    request_key TEXT,
    route_state TEXT CHECK(route_state IN ('pending','done','failed','cancelled')),
    source_conversation_id INTEGER,
    source_message_id INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(group_id, seq)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_butler_chat_group_message_request
    ON butler_chat_group_message(group_id, request_key) WHERE request_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_butler_chat_group_message_route
    ON butler_chat_group_message(route_state) WHERE route_state='pending';
CREATE TABLE IF NOT EXISTS butler_chat_group_turn (
    id INTEGER PRIMARY KEY,
    group_id INTEGER NOT NULL REFERENCES butler_chat_group(id) ON DELETE CASCADE,
    member_id INTEGER NOT NULL REFERENCES butler_chat_group_member(id) ON DELETE CASCADE,
    trigger_seq INTEGER NOT NULL,
    origin_seq INTEGER NOT NULL,
    depth INTEGER NOT NULL DEFAULT 0,
    round INTEGER NOT NULL DEFAULT 1,
    rounds INTEGER NOT NULL DEFAULT 1,
    state TEXT NOT NULL DEFAULT 'queued'
        CHECK(state IN ('queued','running','done','failed','cancelled','skipped')),
    input_through INTEGER,
    input_message_id INTEGER,
    reply_seq INTEGER,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_butler_chat_group_turn_state
    ON butler_chat_group_turn(state, member_id, id);
CREATE INDEX IF NOT EXISTS idx_butler_chat_group_turn_reply
    ON butler_chat_group_turn(group_id, reply_seq) WHERE reply_seq IS NOT NULL;
";

/// 第一版协作群的队列表（投递包、席位、依赖、原生插队等）。新实现不再使用，迁移公开消息后按子表到父表的顺序删除。
const LEGACY_TABLES: [&str; 19] = [
    "butler_group_background_summary", "butler_group_preparation", "butler_group_native_input",
    "butler_group_supplement", "butler_group_resume", "butler_group_packet_share",
    "butler_group_dependency", "butler_group_dispatch", "butler_group_inbox", "butler_group_round",
    "butler_group_independent", "butler_group_capability", "butler_group_stop_report",
    "butler_group_share", "butler_group_execution", "butler_group_event", "butler_group_packet",
    "butler_group_member", "butler_group",
];

pub fn schema(conn: &Connection) -> rusqlite::Result<()> {
    // Savepoint 在初始化方已开启事务时同样可用；迁移失败时整体回滚，不留下半套表。
    conn.execute_batch("SAVEPOINT butler_chat_group_schema")?;
    match migrate(conn) {
        Ok(()) => conn.execute_batch("RELEASE butler_chat_group_schema"),
        Err(error) => {
            conn.execute_batch("ROLLBACK TO butler_chat_group_schema; RELEASE butler_chat_group_schema")?;
            Err(error)
        }
    }
}

fn table_exists(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [name], |r| r.get(0))
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1)"),
        [column], |r| r.get(0))?;
    if !exists {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"), [])?;
    }
    Ok(())
}

/// Give public messages an integer identity for the generic sync journal. The existing
/// (group, seq) key still addresses quotes and local execution turns.
fn install_sync_schema(conn: &Connection) -> rusqlite::Result<()> {
    if !conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_chat_group_message') WHERE name='id')", [], |r| r.get::<_,bool>(0))? {
        conn.execute_batch("ALTER TABLE butler_chat_group_message RENAME TO butler_chat_group_message_old")?;
        // The two named indexes move with the old table. Recreate them after copying.
        conn.execute_batch(SCHEMA)?;
        let columns = "group_id,seq,sender_member,sender_assistant,sender_name,kind,body,reply_to,request_key,route_state,source_conversation_id,source_message_id,created_at";
        conn.execute_batch(&format!("INSERT INTO butler_chat_group_message({columns}) SELECT {columns} FROM butler_chat_group_message_old ORDER BY group_id,seq;
            DROP TABLE butler_chat_group_message_old;"))?;
        conn.execute_batch(SCHEMA)?;
    }
    if !table_exists(conn, "butler_group_position")? {
        conn.execute_batch("CREATE TABLE butler_group_position (
            group_id INTEGER NOT NULL, local_seq INTEGER NOT NULL, wire_seq INTEGER NOT NULL, uid TEXT NOT NULL,
            PRIMARY KEY(group_id,local_seq), UNIQUE(group_id,uid));
            INSERT INTO butler_group_position SELECT group_id,seq,seq,lower(hex(randomblob(16))) FROM butler_chat_group_message;")?;
    }
    conn.execute_batch("
        CREATE TRIGGER IF NOT EXISTS group_message_position AFTER INSERT ON butler_chat_group_message BEGIN
            INSERT INTO butler_group_position(group_id,local_seq,wire_seq,uid) VALUES(NEW.group_id,NEW.seq,NEW.seq,lower(hex(randomblob(16))))
                ON CONFLICT(group_id,local_seq) DO NOTHING;
        END;
        CREATE TRIGGER IF NOT EXISTS group_message_tail AFTER INSERT ON butler_chat_group_message BEGIN
            UPDATE butler_chat_group SET seq=max(seq,NEW.seq), updated_at=max(updated_at,NEW.created_at) WHERE id=NEW.group_id;
        END;
        CREATE TRIGGER IF NOT EXISTS group_conversation_name AFTER UPDATE OF name ON conversation
        WHEN NEW.conversation_kind='butler_group' BEGIN
            UPDATE butler_chat_group SET name=NEW.name WHERE id=NEW.id;
        END;
        CREATE TABLE IF NOT EXISTS butler_read_reply (
            id INTEGER PRIMARY KEY AUTOINCREMENT, message_id INTEGER NOT NULL UNIQUE REFERENCES message(id) ON DELETE CASCADE);
        CREATE TABLE IF NOT EXISTS butler_read_group_reply (
            id INTEGER PRIMARY KEY AUTOINCREMENT, message_id INTEGER NOT NULL UNIQUE REFERENCES butler_chat_group_message(id) ON DELETE CASCADE);
        CREATE TRIGGER IF NOT EXISTS group_read_cleanup AFTER DELETE ON butler_chat_group_message BEGIN
            DELETE FROM butler_read_group_reply WHERE message_id=OLD.id;
        END;
        CREATE TRIGGER IF NOT EXISTS group_parent_cleanup AFTER DELETE ON conversation BEGIN
            DELETE FROM butler_chat_group_turn WHERE member_id IN (SELECT id FROM butler_chat_group_member WHERE conversation_id=OLD.id);
            DELETE FROM butler_chat_group_member WHERE conversation_id=OLD.id OR group_id=OLD.id;
            DELETE FROM butler_chat_group_message WHERE group_id=OLD.id;
            DELETE FROM butler_chat_group_turn WHERE group_id=OLD.id;
            DELETE FROM butler_group_position WHERE group_id=OLD.id;
            DELETE FROM butler_chat_group WHERE id=OLD.id;
        END;
    ")?;
    if table_exists(conn, "message")? {
        conn.execute_batch("CREATE TRIGGER IF NOT EXISTS contact_read_cleanup AFTER DELETE ON message BEGIN
            DELETE FROM butler_read_reply WHERE message_id=OLD.id; END;")?;
    }
    Ok(())
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    // 早期版本按顺序串行回复（after_turn），现在同一轮并发回复、多轮讨论按 round/rounds 推进。
    ensure_column(conn, "butler_chat_group_turn", "round", "INTEGER NOT NULL DEFAULT 1")?;
    ensure_column(conn, "butler_chat_group_turn", "rounds", "INTEGER NOT NULL DEFAULT 1")?;
    ensure_column(conn, "butler_chat_group_turn", "relay_review_pending", "INTEGER NOT NULL DEFAULT 0")?;
    // 成员发言对应它执行会话里的那条最终回复，界面据此复用私聊的消息信息、代码预览与附件。
    ensure_column(conn, "butler_chat_group_message", "source_conversation_id", "INTEGER")?;
    ensure_column(conn, "butler_chat_group_message", "source_message_id", "INTEGER")?;
    install_sync_schema(conn)?;
    if !table_exists(conn, "butler_group_event")? {
        return Ok(());
    }
    // 保留旧群的名称、成员与公开发言；旧成员不回放积压消息，读取位置直接对齐到最新消息。
    conn.execute_batch(
        "PRAGMA defer_foreign_keys=ON;
        INSERT OR IGNORE INTO butler_chat_group(id,name,seq)
            SELECT g.id,g.name,g.seq FROM butler_group g WHERE g.id IN (SELECT id FROM conversation);
        INSERT OR IGNORE INTO butler_chat_group_member(id,group_id,assistant_id,name,conversation_id,joined_after,delivered_through,active)
            SELECT m.id,m.group_id,m.assistant_id,m.name,m.conversation_id,m.joined_after,g.seq,m.active
            FROM butler_group_member m JOIN butler_chat_group g ON g.id=m.group_id
            WHERE m.conversation_id IN (SELECT id FROM conversation);
        INSERT OR IGNORE INTO butler_chat_group_message(group_id,seq,sender_member,sender_assistant,sender_name,kind,body,reply_to,created_at)
            SELECT e.group_id,e.seq,e.sender_member,m.assistant_id,
                CASE WHEN e.kind='notice' THEN '系统' WHEN e.sender_member IS NULL THEN '我' ELSE e.sender_name END,
                CASE WHEN e.kind='notice' THEN 'notice' ELSE 'message' END,e.body,e.reply_to,e.created_at
            FROM butler_group_event e JOIN butler_chat_group g ON g.id=e.group_id
            LEFT JOIN butler_group_member m ON m.id=e.sender_member;",
    )?;
    for table in LEGACY_TABLES {
        conn.execute_batch(&format!("DROP TABLE IF EXISTS {table}"))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub seq: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Member {
    pub id: i64,
    pub group_id: i64,
    pub assistant_id: i64,
    pub name: String,
    pub conversation_id: i64,
    pub joined_after: i64,
    pub delivered_through: i64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Message {
    pub group_id: i64,
    pub seq: i64,
    pub sender_member: Option<i64>,
    pub sender_assistant: Option<i64>,
    pub sender_name: String,
    /// `message` 为用户或成员发言；`notice`/`error` 为系统提示。
    pub kind: String,
    pub body: String,
    pub reply_to: Option<i64>,
    pub created_at: String,
    /// 成员发言来自哪个执行会话的哪条回复；用户发言和系统提示为空。
    pub source_conversation_id: Option<i64>,
    pub source_message_id: Option<i64>,
}

impl Message {
    pub fn from_user(&self) -> bool {
        self.kind == "message" && self.sender_member.is_none()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Turn {
    pub id: i64,
    pub group_id: i64,
    pub member_id: i64,
    pub trigger_seq: i64,
    pub origin_seq: i64,
    pub depth: i64,
    /// 多轮讨论的第几轮（从 1 开始）与计划轮数；普通回复为 1/1。
    pub round: i64,
    pub rounds: i64,
    pub state: String,
    pub input_through: Option<i64>,
    pub input_message_id: Option<i64>,
    pub reply_seq: Option<i64>,
    pub error: Option<String>,
}

/// Stable public-message anchor for a credential request in a member execution conversation.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestAnchor { pub group_id: i64, pub seq: i64 }

pub fn credential_request_anchor(conn: &Connection, conversation: i64, source_message: i64) -> Result<Option<RequestAnchor>> {
    // A published source has an exact counterpart. Never approximate with a later source ID.
    let published = conn.query_row("SELECT group_id,seq FROM butler_chat_group_message
        WHERE source_conversation_id=?1 AND source_message_id=?2 AND kind='message' LIMIT 1",
        params![conversation, source_message], |r| Ok(RequestAnchor { group_id: r.get(0)?, seq: r.get(1)? })).optional()?;
    if published.is_some() { return Ok(published); }
    // During execution there is no public assistant reply yet. Pin to the turn's triggering
    // group message now, before another user message or a later turn can change the live tail.
    Ok(conn.query_row("SELECT t.group_id,t.trigger_seq FROM butler_chat_group_turn t
        JOIN butler_chat_group_member m ON m.id=t.member_id
        JOIN message source ON source.id=?2 AND source.conversation_id=m.conversation_id
        WHERE m.conversation_id=?1 AND m.active=1 AND t.state='running'
          AND t.input_message_id IS NOT NULL AND source.id>=t.input_message_id
        ORDER BY t.id DESC LIMIT 1", params![conversation, source_message],
        |r| Ok(RequestAnchor { group_id: r.get(0)?, seq: r.get(1)? })).optional()?)
}

const GROUP_COLUMNS: &str = "id,name,seq,created_at,updated_at";
const MEMBER_COLUMNS: &str = "id,group_id,assistant_id,name,conversation_id,joined_after,delivered_through,active";
const MESSAGE_COLUMNS: &str = "group_id,seq,sender_member,sender_assistant,sender_name,kind,body,reply_to,created_at,source_conversation_id,source_message_id";
const TURN_COLUMNS: &str = "id,group_id,member_id,trigger_seq,origin_seq,depth,round,rounds,state,input_through,input_message_id,reply_seq,error";

fn group_row(r: &Row<'_>) -> rusqlite::Result<Group> {
    Ok(Group { id: r.get(0)?, name: r.get(1)?, seq: r.get(2)?, created_at: r.get(3)?, updated_at: r.get(4)? })
}
fn member_row(r: &Row<'_>) -> rusqlite::Result<Member> {
    Ok(Member {
        id: r.get(0)?, group_id: r.get(1)?, assistant_id: r.get(2)?, name: r.get(3)?, conversation_id: r.get(4)?,
        joined_after: r.get(5)?, delivered_through: r.get(6)?, active: r.get(7)?,
    })
}
fn message_row(r: &Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        group_id: r.get(0)?, seq: r.get(1)?, sender_member: r.get(2)?, sender_assistant: r.get(3)?,
        sender_name: r.get(4)?, kind: r.get(5)?, body: r.get(6)?, reply_to: r.get(7)?, created_at: r.get(8)?,
        source_conversation_id: r.get(9)?, source_message_id: r.get(10)?,
    })
}
fn turn_row(r: &Row<'_>) -> rusqlite::Result<Turn> {
    Ok(Turn {
        id: r.get(0)?, group_id: r.get(1)?, member_id: r.get(2)?, trigger_seq: r.get(3)?, origin_seq: r.get(4)?,
        depth: r.get(5)?, round: r.get(6)?, rounds: r.get(7)?, state: r.get(8)?, input_through: r.get(9)?,
        input_message_id: r.get(10)?, reply_seq: r.get(11)?, error: r.get(12)?,
    })
}

fn collect<T>(conn: &Connection, sql: &str, args: impl rusqlite::Params, map: fn(&Row<'_>) -> rusqlite::Result<T>) -> Result<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(args, map)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Group> {
    conn.query_row(&format!("SELECT {GROUP_COLUMNS} FROM butler_chat_group WHERE id=?1"), [id], group_row)
        .optional()?.with_context(|| format!("群聊 #{id} 不存在"))
}

pub fn list(conn: &Connection) -> Result<Vec<Group>> {
    collect(conn, &format!("SELECT {GROUP_COLUMNS} FROM butler_chat_group ORDER BY updated_at DESC, id DESC"), [], group_row)
}

pub fn members(conn: &Connection, group: i64) -> Result<Vec<Member>> {
    collect(conn, &format!("SELECT {MEMBER_COLUMNS} FROM butler_chat_group_member WHERE group_id=?1 ORDER BY id"), [group], member_row)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    query: Option<String>,
    before_seq: Option<i64>,
    limit: Option<usize>,
}

pub fn read_history(conn: &Connection, conversation: i64, request: HistoryRequest) -> Result<serde_json::Value> {
    let (group, joined_after): (i64, i64) = conn.query_row(
        "SELECT group_id,joined_after FROM butler_chat_group_member WHERE conversation_id=?1 AND active=1",
        [conversation], |r| Ok((r.get(0)?, r.get(1)?))).optional()?.context("只有当前群的在群成员可以查看发言")?;
    let limit = request.limit.unwrap_or(15);
    ensure!((1..=50).contains(&limit), "limit 须为 1 至 50");
    let before = request.before_seq.unwrap_or(i64::MAX);
    ensure!(before > 0, "before_seq 须为正整数");
    let query = request.query.as_deref().map(str::trim);
    if let Some(query) = query {
        ensure!((1..=512).contains(&query.chars().count()), "query 须为 1 至 512 个字符");
    }
    let needle = query.map(str::to_lowercase);
    let mut stmt = conn.prepare(&format!("{} WHERE group_id=?1 AND seq>?2 AND seq<?3 AND kind='message' ORDER BY seq DESC", public_messages_sql()))?;
    let rows = stmt.query_map(params![group, joined_after, before], public_message_row)?;
    let mut messages = Vec::new();
    let mut chars = 0;
    let mut oldest = None;
    let mut next = None;
    for row in rows {
        let row = row?;
        if needle.as_ref().is_some_and(|needle| !row.body.to_lowercase().contains(needle)) { continue; }
        chars += row.body.chars().count();
        if messages.len() == limit || (!messages.is_empty() && chars > INPUT_CHAR_LIMIT) {
            next = oldest;
            break;
        }
        oldest = Some(row.seq);
        messages.push(serde_json::json!({"seq":row.seq,"speaker":speaker(&row),"body":row.body,
            "reply_to":row.reply_to,"created_at":row.created_at}));
    }
    messages.reverse();
    Ok(serde_json::json!({"messages":messages,"next_before_seq":next}))
}

pub fn active_members(conn: &Connection, group: i64) -> Result<Vec<Member>> {
    Ok(members(conn, group)?.into_iter().filter(|m| m.active).collect())
}

pub fn member(conn: &Connection, id: i64) -> Result<Member> {
    conn.query_row(&format!("SELECT {MEMBER_COLUMNS} FROM butler_chat_group_member WHERE id=?1"), [id], member_row)
        .optional()?.with_context(|| format!("群成员 #{id} 不存在"))
}

pub fn message(conn: &Connection, group: i64, seq: i64) -> Result<Message> {
    message_or_none(conn, group, seq)?.with_context(|| format!("群聊 #{group} 的消息 #{seq} 不存在"))
}

fn public_messages_sql() -> String {
    format!("SELECT {MESSAGE_COLUMNS},source.content,source.metadata_json FROM butler_chat_group_message
        LEFT JOIN message source ON source.id=source_message_id AND source.conversation_id=source_conversation_id")
}

fn public_message_row(r: &Row<'_>) -> rusqlite::Result<Message> {
    let mut row = message_row(r)?;
    if let (Some(body), Some(meta)) = (r.get::<_, Option<String>>(11)?, r.get::<_, Option<String>>(12)?) {
        if row.body == body.trim() {
            if let Some(text) = super::butler_execution::agent_final_text(&body, Some(&meta)) {
                row.body = text.trim().to_owned();
            }
        }
    }
    Ok(row)
}

/// 按时间正序返回最新一页；`before` 用于向上翻页。
pub fn messages(conn: &Connection, group: i64, before: Option<i64>, limit: usize) -> Result<Vec<Message>> {
    ensure!((1..=200).contains(&limit), "群消息分页参数无效：limit={limit}");
    let mut rows = collect(conn, &format!("{} WHERE group_id=?1 AND seq<?2 ORDER BY seq DESC LIMIT ?3", public_messages_sql()),
        params![group, before.unwrap_or(i64::MAX), limit as i64], public_message_row)?;
    rows.reverse();
    Ok(rows)
}

/// Input boundaries identify the owning turn even when a member stops without a reply.
#[derive(Debug, Clone, Serialize)]
pub struct InteractionTurn {
    conversation_id: i64, input_id: i64, trigger_seq: i64, reply_seq: Option<i64>, ended_at: Option<String>,
}

pub fn interaction_turns(conn: &Connection, group: i64) -> Result<Vec<InteractionTurn>> {
    collect(conn, "SELECT m.conversation_id,t.input_message_id,t.trigger_seq,t.reply_seq,
            CASE WHEN t.state!='running' THEN t.updated_at END
        FROM butler_chat_group_turn t JOIN butler_chat_group_member m ON m.id=t.member_id
        WHERE t.group_id=?1 AND t.input_message_id IS NOT NULL
        ORDER BY t.input_message_id DESC", [group], |r| Ok(InteractionTurn {
            conversation_id: r.get(0)?, input_id: r.get(1)?, trigger_seq: r.get(2)?, reply_seq: r.get(3)?, ended_at: r.get(4)?,
        }))
}

fn messages_between(conn: &Connection, group: i64, after: i64, through: i64) -> Result<Vec<Message>> {
    collect(conn, &format!("{} WHERE group_id=?1 AND seq>?2 AND seq<=?3 ORDER BY seq", public_messages_sql()),
        params![group, after, through], public_message_row)
}

/// 联系人列表用的最后一条可见消息。
pub fn last_message(conn: &Connection, group: i64) -> Result<Option<Message>> {
    Ok(conn.query_row(&format!("SELECT {MESSAGE_COLUMNS} FROM butler_chat_group_message WHERE group_id=?1 ORDER BY seq DESC LIMIT 1"),
        [group], message_row).optional()?)
}

pub fn turn(conn: &Connection, id: i64) -> Result<Turn> {
    conn.query_row(&format!("SELECT {TURN_COLUMNS} FROM butler_chat_group_turn WHERE id=?1"), [id], turn_row)
        .optional()?.with_context(|| format!("群回复 #{id} 不存在"))
}

/// 尚未结束的回复（排队或进行中），供界面显示“正在输入”。
pub fn open_turns(conn: &Connection, group: i64) -> Result<Vec<Turn>> {
    collect(conn, &format!("SELECT {TURN_COLUMNS} FROM butler_chat_group_turn
        WHERE group_id=?1 AND state IN ('queued','running') ORDER BY id"), [group], turn_row)
}

pub fn running_turns(conn: &Connection) -> Result<Vec<Turn>> {
    collect(conn, &format!("SELECT {TURN_COLUMNS} FROM butler_chat_group_turn WHERE state='running' ORDER BY id"), [], turn_row)
}

/// 可以开始的回复：不同成员同时回复，同一成员按先来后到一次只处理一项。
pub fn runnable_turns(conn: &Connection) -> Result<Vec<Turn>> {
    collect(conn, &format!("SELECT {} FROM butler_chat_group_turn t
        JOIN butler_chat_group_member m ON m.id=t.member_id AND m.active=1
        WHERE t.state='queued'
          AND NOT EXISTS(SELECT 1 FROM butler_chat_group_turn o WHERE o.member_id=t.member_id
              AND (o.state='running' OR (o.state='queued' AND o.id<t.id)))
        ORDER BY t.id", TURN_COLUMNS.split(',').map(|c| format!("t.{c}")).collect::<Vec<_>>().join(",")), [], turn_row)
}

/// 是否还有消息在等待判断由谁回复。
pub fn has_pending_route(conn: &Connection, group: i64) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM butler_chat_group_message WHERE group_id=?1 AND route_state='pending')",
        [group], |r| r.get(0))?)
}

pub fn pending_routes(conn: &Connection) -> Result<Vec<Message>> {
    collect(conn, &format!("SELECT {MESSAGE_COLUMNS} FROM butler_chat_group_message WHERE route_state='pending' ORDER BY group_id,seq"), [], message_row)
}

pub(crate) fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();
    ensure!(!name.is_empty() && name.chars().count() <= MAX_NAME_CHARS, "群聊名称须为 1–{MAX_NAME_CHARS} 个字符");
    Ok(name.to_string())
}

fn now() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

fn insert_member(tx: &Connection, group: &Group, assistant: i64, name: &str) -> Result<i64> {
    ensure!(assistant > 0 && !name.trim().is_empty(), "助手身份或名称无效");
    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_chat_group_member WHERE group_id=?1 AND assistant_id=?2 AND active=1)",
        params![group.id, assistant], |r| r.get(0))?;
    ensure!(!exists, "{name} 已经在群聊里");
    let stamp = Utc::now();
    tx.execute("INSERT INTO conversation(name,assistant_id,created_time,updated_time,conversation_kind,parent_butler_conversation_id,is_hidden_from_normal_chat_list)
        VALUES(?1,?2,?3,?3,?4,?5,1)", params![format!("{name} · {}", group.name), assistant, stamp, MEMBER_KIND, group.id])?;
    let conversation = tx.last_insert_rowid();
    tx.execute("INSERT INTO butler_chat_group_member(group_id,assistant_id,name,conversation_id,joined_after,delivered_through)
        VALUES(?1,?2,?3,?4,?5,?5)", params![group.id, assistant, name, conversation, group.seq])?;
    Ok(tx.last_insert_rowid())
}

enum Sender<'a> {
    User,
    Member(&'a Member),
    System,
}

fn append(tx: &Connection, group: i64, sender: Sender<'_>, kind: &str, body: &str, reply_to: Option<i64>,
    request_key: Option<&str>, route: bool) -> Result<Message> {
    // The low bits distinguish offline writers at the same logical millisecond.
    // Stay within the exact integer range supported by the WebView.
    let previous = get(tx, group)?.seq;
    let tick = (Utc::now().timestamp_millis() - 1_735_689_600_000).max((previous >> 10) + 1);
    let salt = i64::from(u16::from_le_bytes(rand::random::<[u8; 2]>()) & 0x03ff);
    let seq = tick.checked_mul(1 << 10).and_then(|n| n.checked_add(salt))
        .filter(|n| *n < (1i64 << 53) - 1).context("群聊消息位置超出可表示范围")?;
    let seq = free_position(tx, group, seq)?;
    let (member, assistant, name) = match sender {
        Sender::User => (None, None, "我".to_string()),
        Sender::Member(m) => (Some(m.id), Some(m.assistant_id), m.name.clone()),
        Sender::System => (None, None, "系统".to_string()),
    };
    let stamp = now();
    tx.execute("INSERT INTO butler_chat_group_message(group_id,seq,sender_member,sender_assistant,sender_name,kind,body,reply_to,request_key,route_state,created_at)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![group, seq, member, assistant, name, kind, body, reply_to, request_key, route.then_some("pending"), stamp])?;
    tx.execute("UPDATE butler_chat_group SET seq=?1, updated_at=?2 WHERE id=?3", params![seq, stamp, group])?;
    tx.execute("UPDATE conversation SET updated_time=?1 WHERE id=?2", params![Utc::now(), group])?;
    message(tx, group, seq)
}

/// A wire position can be reserved by a quote or join boundary before its
/// message arrives. Existing local positions never move under an active turn.
pub(crate) fn free_position(conn: &Connection, group: i64, preferred: i64) -> rusqlite::Result<i64> {
    let mut next = preferred;
    while conn.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_position WHERE group_id=?1 AND local_seq=?2)",
        params![group,next], |r|r.get::<_,bool>(0))? {
        next += 1;
        if next >= (1_i64 << 53) - 1 { return Err(rusqlite::Error::IntegralValueOutOfRange(0, next)); }
    }
    Ok(next)
}

fn notice(tx: &Connection, group: i64, kind: &str, body: &str) -> Result<Message> {
    append(tx, group, Sender::System, kind, body, None, None, false)
}

fn join_names(names: &[String]) -> String {
    names.join("、")
}

/// 创建群聊：成员各自获得一个只属于本群的执行会话。
pub fn create(conn: &mut Connection, name: &str, invited: &[(i64, String)]) -> Result<Group> {
    let name = validate_name(name)?;
    ensure!(!invited.is_empty(), "请至少选择一位成员");
    ensure!(invited.len() <= MAX_MEMBERS, "群聊最多 {MAX_MEMBERS} 位成员");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stamp = Utc::now();
    tx.execute("INSERT INTO conversation(name,created_time,updated_time,conversation_kind,is_hidden_from_normal_chat_list) VALUES(?1,?2,?2,?3,1)",
        params![name, stamp, GROUP_KIND])?;
    let id = tx.last_insert_rowid();
    tx.execute("INSERT INTO butler_chat_group(id,name,created_at,updated_at) VALUES(?1,?2,?3,?3)", params![id, name, now()])?;
    let group = get(&tx, id)?;
    for (assistant, label) in invited {
        insert_member(&tx, &group, *assistant, label)?;
    }
    notice(&tx, id, "notice", &format!("你邀请 {} 加入了群聊", join_names(&invited.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>())))?;
    let result = get(&tx, id)?;
    tx.commit()?;
    Ok(result)
}

pub fn rename(conn: &mut Connection, group: i64, name: &str) -> Result<Group> {
    let name = validate_name(name)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = get(&tx, group)?;
    if current.name != name {
        tx.execute("UPDATE butler_chat_group SET name=?1 WHERE id=?2", params![name, group])?;
        tx.execute("UPDATE conversation SET name=?1 WHERE id=?2", params![name, group])?;
        notice(&tx, group, "notice", &format!("你将群聊名称改为「{name}」"))?;
    }
    let result = get(&tx, group)?;
    tx.commit()?;
    Ok(result)
}

pub fn add_members(conn: &mut Connection, group: i64, invited: &[(i64, String)]) -> Result<Vec<Member>> {
    ensure!(!invited.is_empty(), "请至少选择一位成员");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let room = get(&tx, group)?;
    ensure!(active_members(&tx, group)?.len() + invited.len() <= MAX_MEMBERS, "群聊最多 {MAX_MEMBERS} 位成员");
    let mut ids = Vec::new();
    for (assistant, label) in invited {
        ids.push(insert_member(&tx, &room, *assistant, label)?);
    }
    notice(&tx, group, "notice", &format!("你邀请 {} 加入了群聊", join_names(&invited.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>())))?;
    let added = ids.iter().map(|id| member(&tx, *id)).collect::<Result<Vec<_>>>()?;
    tx.commit()?;
    Ok(added)
}

fn cancel_member_turns(tx: &Connection, member_id: i64, reason: &str) -> Result<Vec<Turn>> {
    let running = collect(tx, &format!("SELECT {TURN_COLUMNS} FROM butler_chat_group_turn WHERE member_id=?1 AND state='running'"), [member_id], turn_row)?;
    tx.execute("UPDATE butler_chat_group_turn SET state='cancelled', error=?2, updated_at=?3
        WHERE member_id=?1 AND state IN ('queued','running')", params![member_id, reason, now()])?;
    Ok(running)
}

/// 移出成员：撤销后续投递，返回仍在进行、需要打断的回复。
pub fn remove_member(conn: &mut Connection, group: i64, member_id: i64, reason: &str) -> Result<(Member, Vec<Turn>)> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let m = member(&tx, member_id)?;
    ensure!(m.group_id == group, "成员不属于该群聊");
    if !m.active {
        return Ok((m, Vec::new()));
    }
    tx.execute("UPDATE butler_chat_group_member SET active=0 WHERE id=?1", [member_id])?;
    let running = cancel_member_turns(&tx, member_id, "成员已移出群聊")?;
    notice(&tx, group, "notice", reason)?;
    let result = member(&tx, member_id)?;
    tx.commit()?;
    Ok((result, running))
}

/// 成员显示名跟随助手改名；只更新以后的发言署名，已发出的消息保持原样。
pub fn sync_member_names(conn: &Connection, group: i64, names: &std::collections::HashMap<i64, String>) -> Result<bool> {
    let mut changed = false;
    for m in active_members(conn, group)? {
        if let Some(name) = names.get(&m.assistant_id).filter(|name| **name != m.name) {
            conn.execute("UPDATE butler_chat_group_member SET name=?1 WHERE id=?2", params![name, m.id])?;
            changed = true;
        }
    }
    Ok(changed)
}

/// 用户发言。相同 request_key 的重试返回同一条消息，不重复派发。
pub fn post_user(conn: &mut Connection, group: i64, request_key: &str, body: &str, reply_to: Option<i64>) -> Result<Message> {
    let key = request_key.trim();
    ensure!(!key.is_empty() && key.len() <= 128, "需要稳定的消息请求标识（1–128 字节）");
    let body = body.trim();
    ensure!(!body.is_empty(), "消息不能为空");
    ensure!(body.chars().count() <= MAX_BODY_CHARS, "消息过长，最多 {MAX_BODY_CHARS} 个字符");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    get(&tx, group)?;
    let previous: Option<i64> = tx.query_row("SELECT seq FROM butler_chat_group_message WHERE group_id=?1 AND request_key=?2",
        params![group, key], |r| r.get(0)).optional()?;
    if let Some(seq) = previous {
        let existing = message(&tx, group, seq)?;
        ensure!(existing.body == body && existing.reply_to == reply_to, "消息请求标识已用于另一条消息，请重新发送");
        return Ok(existing);
    }
    ensure!(!active_members(&tx, group)?.is_empty(), "群里没有成员，请先邀请助手");
    if let Some(seq) = reply_to {
        message(&tx, group, seq)?;
    }
    let result = append(&tx, group, Sender::User, "message", body, reply_to, Some(key), true)?;
    tx.commit()?;
    Ok(result)
}

pub fn post_notice(conn: &mut Connection, group: i64, kind: &str, body: &str) -> Result<Message> {
    ensure!(matches!(kind, "notice" | "error"), "未知的系统提示类型：{kind}");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let result = notice(&tx, group, kind, body)?;
    tx.commit()?;
    Ok(result)
}

struct NewTurn {
    group: i64,
    member: i64,
    trigger: i64,
    origin: i64,
    depth: i64,
    round: i64,
    rounds: i64,
}

fn insert_turn(tx: &Connection, turn: NewTurn) -> Result<i64> {
    tx.execute("INSERT INTO butler_chat_group_turn(group_id,member_id,trigger_seq,origin_seq,depth,round,rounds) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![turn.group, turn.member, turn.trigger, turn.origin, turn.depth, turn.round, turn.rounds])?;
    Ok(tx.last_insert_rowid())
}

/// 记录调度结果：被选中的成员同时回复（看不到彼此正在写的内容，之后的回复里会补上）。
/// `rounds` 大于 1 表示用户要求成员讨论：每轮所有人说完后，带着彼此的新发言进入下一轮。
/// 只处理仍在等待调度的消息，调度期间被停止的消息不会再派发。
pub fn route(conn: &mut Connection, group: i64, seq: i64, responders: &[i64], rounds: i64) -> Result<Vec<i64>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let state: Option<String> = tx.query_row("SELECT route_state FROM butler_chat_group_message WHERE group_id=?1 AND seq=?2",
        params![group, seq], |r| r.get(0)).optional()?.flatten();
    if state.as_deref() != Some("pending") {
        return Ok(Vec::new());
    }
    let active = active_members(&tx, group)?;
    let mut selected = Vec::new();
    for id in responders {
        if !selected.contains(id) && active.iter().any(|m| m.id == *id) {
            selected.push(*id);
        }
    }
    ensure!(!selected.is_empty(), "调度结果没有有效的群成员");
    let rounds = if selected.len() > 1 { rounds.clamp(1, MAX_DISCUSSION_ROUNDS) } else { 1 };
    let mut created = Vec::new();
    for member in selected {
        created.push(insert_turn(&tx, NewTurn { group, member, trigger: seq, origin: seq, depth: 0, round: 1, rounds })?);
    }
    tx.execute("UPDATE butler_chat_group_message SET route_state='done' WHERE group_id=?1 AND seq=?2", params![group, seq])?;
    tx.commit()?;
    Ok(created)
}

/// 调度失败：标记消息并在群里说明具体原因。
pub fn fail_route(conn: &mut Connection, group: i64, seq: i64, reason: &str) -> Result<bool> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = tx.execute("UPDATE butler_chat_group_message SET route_state='failed' WHERE group_id=?1 AND seq=?2 AND route_state='pending'",
        params![group, seq])?;
    if changed > 0 {
        notice(&tx, group, "error", &format!("没能安排成员回复这条消息：{reason}"))?;
    }
    tx.commit()?;
    Ok(changed > 0)
}

pub enum Start {
    /// `peers` 是同一轮里同时回复的其他成员。
    Run { turn: Turn, member: Member, group: Group, input: String, peers: Vec<String> },
    Review(RelayReview),
    Skipped,
}

pub struct RelayReview {
    pub origin: Message,
    pub history: Vec<Message>,
    pub target: String,
}

/// 只沿本次邀请的公开消息回溯，群里同时进行的其他接力不会混进审核依据。
fn relay_review(conn: &Connection, t: &Turn, target: String) -> Result<RelayReview> {
    let mut seq = t.trigger_seq;
    let mut history = Vec::new();
    for _ in 0..RELAY_REVIEW_INTERVAL {
        history.push(message(conn, t.group_id, seq)?);
        seq = conn.query_row("SELECT trigger_seq FROM butler_chat_group_turn
            WHERE group_id=?1 AND reply_seq=?2 AND origin_seq=?3", params![t.group_id, seq, t.origin_seq], |r| r.get(0))?;
    }
    history.reverse();
    Ok(RelayReview { origin: message(conn, t.group_id, t.origin_seq)?, history, target })
}

/// 审核结果只作用于仍在排队的这一项。停止、移出、重复回执均不会重新发起接力。
pub fn finish_relay_review(conn: &mut Connection, id: i64, verdict: Result<Option<String>, String>) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let t = turn(&tx, id)?;
    let paused = matches!(verdict, Ok(Some(_)));
    let updated = tx.execute("UPDATE butler_chat_group_turn SET relay_review_pending=0, state=?2, updated_at=CURRENT_TIMESTAMP
        WHERE id=?1 AND state='queued' AND relay_review_pending=1", params![id, if paused { "skipped" } else { "queued" }])?;
    if updated > 0 {
        let target = member(&tx, t.member_id)?.name;
        let round = t.depth - 1;
        match verdict {
            Ok(Some(reason)) => {
                notice(&tx, t.group_id, "notice", &format!("第 {round} 轮接力审核确认出现循环，已暂停这次给 {target} 的接力。{reason}。需要继续时可以直接 @ 对方。"))?;
            }
            Err(error) => {
                notice(&tx, t.group_id, "error", &format!("第 {round} 轮接力审核失败（回复 #{id}）：{error}。这次给 {target} 的接力继续。"))?;
            }
            Ok(None) => {}
        }
    }
    tx.commit()?;
    Ok(())
}

/// 开始一次回复：固定本次可见的群消息上界，整理成员上次回复后其他人的公开发言。
/// 触发消息已经在更早的回复里送达时跳过，避免同一条消息被同一位成员回复两次。
pub fn start_turn(conn: &mut Connection, id: i64) -> Result<Start> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let t = turn(&tx, id)?;
    if t.state != "queued" {
        return Ok(Start::Skipped);
    }
    let mut m = member(&tx, t.member_id)?;
    // 用户停止时，原生进程可能还在收尾。下一轮开始前再检查上次输入对应的持久化输出。
    let previous = tx.query_row(&format!("SELECT {TURN_COLUMNS} FROM butler_chat_group_turn
        WHERE member_id=?1 AND id<?2 AND input_through>?3 AND state IN ('cancelled','failed')
        ORDER BY id DESC LIMIT 1"), params![m.id, id, m.delivered_through], turn_row).optional()?;
    let mut background_through = None;
    if let Some(previous) = previous {
        if received_input(&tx, &previous, &m)? {
            confirm_delivery(&tx, &previous)?;
            m = member(&tx, t.member_id)?;
        } else {
            background_through = previous.input_through;
        }
    }
    // 跳过的回复也算这一轮说完了，讨论可能因此进入下一轮。
    let skip = |tx: &Connection, state: &str| -> Result<()> {
        tx.execute("UPDATE butler_chat_group_turn SET state=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?1", params![id, state])?;
        advance_discussion(tx, &t)?;
        Ok(())
    };
    if !m.active {
        skip(&tx, "cancelled")?;
        tx.commit()?;
        return Ok(Start::Skipped);
    }
    if m.delivered_through >= t.trigger_seq {
        skip(&tx, "skipped")?;
        tx.commit()?;
        return Ok(Start::Skipped);
    }
    let pending: bool = tx.query_row("SELECT relay_review_pending FROM butler_chat_group_turn WHERE id=?1", [id], |r| r.get(0))?;
    if pending {
        let review = relay_review(&tx, &t, m.name)?;
        tx.commit()?;
        return Ok(Start::Review(review));
    }
    let room = get(&tx, t.group_id)?;
    let delta = messages_between(&tx, room.id, m.delivered_through.max(m.joined_after), room.seq)?
        .into_iter().filter(|message| message.sender_member != Some(m.id)).collect::<Vec<_>>();
    if delta.is_empty() {
        skip(&tx, "skipped")?;
        tx.commit()?;
        return Ok(Start::Skipped);
    }
    let input = format_input(&tx, &room, &delta, t.trigger_seq, background_through)?;
    tx.execute("UPDATE butler_chat_group_turn SET state='running', input_through=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?1",
        params![id, room.seq])?;
    let started = turn(&tx, id)?;
    let peers = if t.depth == 0 {
        collect(&tx, "SELECT m.name FROM butler_chat_group_turn o JOIN butler_chat_group_member m ON m.id=o.member_id
            WHERE o.group_id=?1 AND o.origin_seq=?2 AND o.depth=0 AND o.round=?3 AND o.member_id!=?4 ORDER BY o.id",
            params![t.group_id, t.origin_seq, t.round, t.member_id], |r| r.get(0))?
    } else {
        Vec::new()
    };
    tx.commit()?;
    Ok(Start::Run { turn: started, member: m, group: room, input, peers })
}

/// 写入本地执行记录。原生会话此时可能还没启动，读取位置须等实际输出确认。
pub fn write_turn_input(conn: &mut Connection, id: i64, conversation: i64, text: &str, model_id: Option<i64>, model: Option<&str>) -> Result<i64> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let t = turn(&tx, id)?;
    ensure!(t.state == "running", "群回复 #{id} 已{}，未发送给成员", state_label(&t.state));
    ensure!(t.input_message_id.is_none(), "群回复 #{id} 的输入已写入，不能重复发送");
    let m = member(&tx, t.member_id)?;
    ensure!(m.conversation_id == conversation, "群回复 #{id} 不属于会话 #{conversation}");
    let through = t.input_through.context("群回复缺少本次消息上界")?;
    let metadata = serde_json::json!({"origin":"butler_group","group_id":t.group_id,"member_id":m.id,"turn_id":id,"through_seq":through});
    let stamp = Utc::now();
    tx.execute("INSERT INTO message(conversation_id,message_type,content,created_time,llm_model_id,llm_model_name,token_count,input_token_count,output_token_count,metadata_json)
        VALUES(?1,'user',?2,?3,?4,?5,0,0,0,?6)", params![conversation, text, stamp, model_id, model, metadata.to_string()])?;
    let input = tx.last_insert_rowid();
    tx.execute("UPDATE butler_chat_group_turn SET input_message_id=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?1", params![id, input])?;
    tx.execute("UPDATE conversation SET updated_time=?1 WHERE id=?2", params![stamp, conversation])?;
    tx.commit()?;
    Ok(input)
}

fn state_label(state: &str) -> &'static str {
    match state {
        "queued" => "在排队",
        "running" => "在进行",
        "done" => "完成",
        "failed" => "失败",
        "cancelled" => "停止",
        _ => "跳过",
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Evidence {
    Pending,
    Answer(String),
    Silent,
    Failed(String),
}

/// 从成员会话中输入之后的消息判断这次回复的结果：只认已完成、不含待执行工具调用的正文。
pub fn evidence(conn: &Connection, t: &Turn) -> Result<Evidence> {
    let Some(input) = t.input_message_id else { return Ok(Evidence::Pending) };
    let conversation = member(conn, t.member_id)?.conversation_id;
    let row: Option<(i64, String, String, bool, Option<String>, Option<String>)> = conn.query_row(
        "SELECT id,message_type,content,finish_time IS NOT NULL,tool_calls_json,metadata_json FROM message
         WHERE conversation_id=?1 AND id>?2 AND message_type IN ('assistant','response','error','tool_result')
         ORDER BY id DESC LIMIT 1",
        params![conversation, input], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).optional()?;
    let Some((id, kind, body, finished, calls, metadata)) = row else { return Ok(Evidence::Pending) };
    if kind == "error" {
        let reason = if body.trim().is_empty() { "执行通道记录了错误，但没有保存错误正文".to_string() } else { body };
        return Ok(Evidence::Failed(format!("{reason}（会话 #{conversation}，消息 #{id}）")));
    }
    if !finished || !matches!(kind.as_str(), "response" | "assistant") {
        return Ok(Evidence::Pending);
    }
    let end_reason = metadata.as_deref().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("end_reason").and_then(|r| r.as_str()).map(str::to_string));
    if let Some(reason) = end_reason.filter(|r| matches!(r.as_str(), "interrupted" | "cancelled" | "failed")) {
        return Ok(Evidence::Failed(format!("回复已中断（end_reason={reason}，会话 #{conversation}，消息 #{id}）")));
    }
    if calls.as_deref().is_some_and(|s| !matches!(s.trim(), "" | "[]" | "null")) {
        return Ok(Evidence::Pending);
    }
    let projected = super::butler_execution::agent_final_text(&body, metadata.as_deref());
    let body = projected.as_deref().unwrap_or(&body);
    if body.trim() == "[[NO_REPLY]]" {
        return Ok(Evidence::Silent);
    }
    if super::butler_execution::is_completed_content(&kind, body, finished, calls.as_deref()) {
        return Ok(Evidence::Answer(body.trim().to_string()));
    }
    Ok(Evidence::Pending)
}

pub enum Outcome {
    Reply(String),
    Silent,
    Failed(String),
}

fn confirm_delivery(conn: &Connection, t: &Turn) -> Result<()> {
    if let (Some(_), Some(through)) = (t.input_message_id, t.input_through) {
        conn.execute("UPDATE butler_chat_group_member SET delivered_through=max(delivered_through,?2) WHERE id=?1",
            params![t.member_id, through])?;
    }
    Ok(())
}

/// 空回复占位和本地报错都不能证明收到输入。只查看该成员这次输入至下一次输入之间的输出。
fn received_input(conn: &Connection, t: &Turn, m: &Member) -> Result<bool> {
    let Some(input) = t.input_message_id else { return Ok(false); };
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM message WHERE conversation_id=?1 AND id>?2
        AND id<coalesce((SELECT min(input_message_id) FROM butler_chat_group_turn
            WHERE member_id=?3 AND input_message_id>?2),9223372036854775807)
        AND (message_type='tool_result' OR (message_type IN ('assistant','response','reasoning') AND
            (trim(content)!='' OR coalesce(trim(tool_calls_json),'') NOT IN ('','null','[]')))
            OR CASE WHEN json_valid(metadata_json) THEN json_array_length(metadata_json,'$.agent_activities')>0 END))",
        params![m.conversation_id, input, m.id], |r| r.get(0))?)
}

#[derive(Debug, Default)]
pub struct Finished {
    pub reply: Option<Message>,
    pub relayed: Vec<i64>,
}

/// 结束一次回复：公开最终回复，并按回复里的 @ 邀请其他成员接着说；
/// 讨论中的一轮全部说完后进入下一轮。已被停止或移出的回复不会再公开迟到的结果。
pub fn finish_turn(conn: &mut Connection, id: i64, outcome: Outcome) -> Result<Option<Finished>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let t = turn(&tx, id)?;
    if t.state != "running" {
        return Ok(None);
    }
    let m = member(&tx, t.member_id)?;
    let mut finished = Finished::default();
    if !matches!(outcome, Outcome::Failed(_)) || received_input(&tx, &t, &m)? {
        confirm_delivery(&tx, &t)?;
    }
    match outcome {
        Outcome::Reply(body) => {
            // 回复开头的 `[[QUOTE:原话]]` 是成员在引用某条群消息：去掉标记，找到那条消息后记为回复对象。
            let (body, reply_to) = match reply_quote::split(&body) {
                Some((fragment, rest)) => {
                    let quoted = quoted_seq(&tx, &t, &m, fragment)?;
                    if quoted.is_none() {
                        tracing::warn!(group_id = t.group_id, turn_id = id, member = %m.name, fragment,
                            "group member quoted text that matches no visible group message; showing the reply without a quote");
                    }
                    (rest.to_string(), quoted)
                }
                None => (body, None),
            };
            let reply = append(&tx, t.group_id, Sender::Member(&m), "message", &body, reply_to, None, false)?;
            let source: Option<i64> = match t.input_message_id {
                Some(input) => tx.query_row("SELECT id FROM message WHERE conversation_id=?1 AND id>?2
                    AND message_type IN ('response','assistant') AND finish_time IS NOT NULL ORDER BY id DESC LIMIT 1",
                    params![m.conversation_id, input], |r| r.get(0)).optional()?,
                None => None,
            };
            tx.execute("UPDATE butler_chat_group_message SET source_conversation_id=?3, source_message_id=?4 WHERE group_id=?1 AND seq=?2",
                params![t.group_id, reply.seq, m.conversation_id, source])?;
            let reply = message(&tx, t.group_id, reply.seq)?;
            tx.execute("UPDATE butler_chat_group_turn SET state='done', reply_seq=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?1", params![id, reply.seq])?;
            finished.relayed = relay(&tx, &t, &m, &reply)?;
            finished.reply = Some(reply);
        }
        Outcome::Silent => {
            tx.execute("UPDATE butler_chat_group_turn SET state='done', updated_at=?2 WHERE id=?1", params![id, now()])?;
        }
        Outcome::Failed(reason) => {
            tx.execute("UPDATE butler_chat_group_turn SET state='failed', error=?2, updated_at=?3 WHERE id=?1", params![id, reason, now()])?;
            notice(&tx, t.group_id, "error", &format!("{} 没能回复：{reason}", m.name))?;
        }
    }
    finished.relayed.extend(advance_discussion(&tx, &t)?);
    tx.commit()?;
    Ok(Some(finished))
}

/// 成员引用的原话出自哪条群消息：只在它这次回复时能看到的发言（加入之后、本次输入上界之内）里，从近到远找。
fn quoted_seq(tx: &Connection, t: &Turn, m: &Member, fragment: &str) -> Result<Option<i64>> {
    let through = match t.input_through {
        Some(through) => through,
        None => get(tx, t.group_id)?.seq,
    };
    let candidates = collect(tx, &format!("SELECT {MESSAGE_COLUMNS} FROM butler_chat_group_message
        WHERE group_id=?1 AND kind='message' AND seq>?2 AND seq<=?3 ORDER BY seq DESC LIMIT ?4"),
        params![t.group_id, m.joined_after, through, QUOTE_SEARCH_LIMIT], message_row)?;
    Ok(candidates.into_iter().find(|message| reply_quote::matches(fragment, &message.body)).map(|message| message.seq))
}

fn discussion_members(tx: &Connection, t: &Turn, round: Option<i64>) -> Result<Vec<i64>> {
    collect(tx, "SELECT DISTINCT t.member_id FROM butler_chat_group_turn t
        JOIN butler_chat_group_member m ON m.id=t.member_id AND m.active=1
        WHERE t.group_id=?1 AND t.origin_seq=?2 AND t.depth=0 AND (?3 IS NULL OR t.round=?3) ORDER BY t.member_id",
        params![t.group_id, t.origin_seq, round], |r| r.get(0))
}

/// 多轮讨论：一轮里所有人都说完（或跳过）后，参与者带着彼此的新发言进入下一轮。
/// 这一轮没人再发言（都认为没有新内容）、到达计划轮数、用户插话或只剩一人时结束。
fn advance_discussion(tx: &Connection, t: &Turn) -> Result<Vec<i64>> {
    if t.depth != 0 || t.round >= t.rounds {
        return Ok(Vec::new());
    }
    let (open, spoke): (bool, bool) = tx.query_row("SELECT
            EXISTS(SELECT 1 FROM butler_chat_group_turn WHERE group_id=?1 AND origin_seq=?2 AND depth=0 AND round=?3 AND state IN ('queued','running')),
            EXISTS(SELECT 1 FROM butler_chat_group_turn WHERE group_id=?1 AND origin_seq=?2 AND depth=0 AND round=?3 AND reply_seq IS NOT NULL)",
        params![t.group_id, t.origin_seq, t.round], |r| Ok((r.get(0)?, r.get(1)?)))?;
    if open || !spoke {
        return Ok(Vec::new());
    }
    let interrupted: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_chat_group_message
        WHERE group_id=?1 AND seq>?2 AND kind='message' AND sender_member IS NULL)", params![t.group_id, t.origin_seq], |r| r.get(0))?;
    let next = t.round + 1;
    let started: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_chat_group_turn WHERE group_id=?1 AND origin_seq=?2 AND depth=0 AND round=?3)",
        params![t.group_id, t.origin_seq, next], |r| r.get(0))?;
    let members = discussion_members(tx, t, Some(t.round))?;
    if interrupted || started || members.len() < 2 {
        return Ok(Vec::new());
    }
    let trigger = get(tx, t.group_id)?.seq;
    members.into_iter().map(|member| insert_turn(tx, NewTurn {
        group: t.group_id, member, trigger, origin: t.origin_seq, depth: 0, round: next, rounds: t.rounds,
    })).collect()
}

fn relay(tx: &Connection, t: &Turn, from: &Member, reply: &Message) -> Result<Vec<i64>> {
    let active = active_members(tx, t.group_id)?;
    let others = active.iter().filter(|m| m.id != from.id).cloned().collect::<Vec<_>>();
    // 讨论还有下一轮时，被 @ 的参与者会在下一轮回应，不再单独接力，避免同一人连说两次。
    let discussing = if t.depth == 0 && t.round < t.rounds { discussion_members(tx, t, None)? } else { Vec::new() };
    let mut created = Vec::new();
    let review_due = t.depth > 0 && t.depth % RELAY_REVIEW_INTERVAL == 0;
    for target in parse_mentions(&reply.body, &others) {
        let waiting: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_chat_group_turn WHERE member_id=?1 AND state='queued' AND relay_review_pending=0)",
            [target], |r| r.get(0))?;
        if waiting || discussing.contains(&target) {
            continue;
        }
        let id = insert_turn(tx, NewTurn {
            group: t.group_id, member: target, trigger: reply.seq, origin: t.origin_seq, depth: t.depth + 1, round: 1, rounds: 1,
        })?;
        if review_due {
            tx.execute("UPDATE butler_chat_group_turn SET relay_review_pending=1 WHERE id=?1", [id])?;
        }
        created.push(id);
    }
    Ok(created)
}

/// 停止群里所有排队和进行中的回复。返回需要打断执行的回复。
pub fn stop(conn: &mut Connection, group: i64) -> Result<Vec<Turn>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    get(&tx, group)?;
    let running = collect(&tx, &format!("SELECT {TURN_COLUMNS} FROM butler_chat_group_turn WHERE group_id=?1 AND state='running'"), [group], turn_row)?;
    let routes = tx.execute("UPDATE butler_chat_group_message SET route_state='cancelled' WHERE group_id=?1 AND route_state='pending'", [group])?;
    let turns = tx.execute("UPDATE butler_chat_group_turn SET state='cancelled', error='用户停止了回复', updated_at=?2
        WHERE group_id=?1 AND state IN ('queued','running')", params![group, now()])?;
    if routes + turns > 0 {
        notice(&tx, group, "notice", "你停止了当前的回复")?;
    }
    tx.commit()?;
    Ok(running)
}

pub fn member_conversations(conn: &Connection, group: i64) -> Result<Vec<i64>> {
    Ok(members(conn, group)?.into_iter().map(|m| m.conversation_id).collect())
}

/// 应用重启后，上次未结束的回复已经失去执行者：有完整结果的照常公开，否则说明中断原因，不自动重做。
pub fn recover(conn: &mut Connection) -> Result<Vec<i64>> {
    let mut groups = Vec::new();
    for t in running_turns(conn)? {
        let outcome = match evidence(conn, &t)? {
            Evidence::Answer(body) => Outcome::Reply(body),
            Evidence::Silent => Outcome::Silent,
            Evidence::Failed(reason) => Outcome::Failed(reason),
            Evidence::Pending => Outcome::Failed("应用重启时这次回复还没有完成，结果无法确认，没有自动重试".into()),
        };
        if finish_turn(conn, t.id, outcome)?.is_some() && !groups.contains(&t.group_id) {
            groups.push(t.group_id);
        }
    }
    Ok(groups)
}

fn push_unique(result: &mut Vec<i64>, id: i64) {
    if !result.contains(&id) {
        result.push(id);
    }
}

/// 找出正文中 `@名字` 点到的成员（按首次出现顺序）。`@所有人` 表示全部成员。
/// 中文名后常常不加空格，因此按最长的成员名前缀匹配；代码里的 @ 和邮箱地址不算点名。
pub fn parse_mentions(body: &str, members: &[Member]) -> Vec<i64> {
    let mut result = Vec::new();
    let mut in_fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut in_code = false;
        let mut previous: Option<char> = None;
        for (offset, c) in line.char_indices() {
            let before = previous.replace(c);
            if c == '`' {
                in_code = !in_code;
                continue;
            }
            if in_code || (c != '@' && c != '＠') || before.is_some_and(|p| p.is_ascii_alphanumeric()) {
                continue;
            }
            let rest = &line[offset + c.len_utf8()..];
            let best = members.iter()
                .filter(|m| !m.name.is_empty() && rest.starts_with(m.name.as_str()))
                .map(|m| m.name.len()).max();
            if let Some(len) = best {
                for m in members.iter().filter(|m| m.name.len() == len && rest.starts_with(m.name.as_str())) {
                    push_unique(&mut result, m.id);
                }
            } else if EVERYONE_MENTIONS.iter().any(|word| rest.starts_with(word)) {
                for m in members {
                    push_unique(&mut result, m.id);
                }
            }
        }
    }
    result
}

fn speaker(message: &Message) -> String {
    if message.from_user() { "用户".to_string() } else { message.sender_name.clone() }
}

fn clip_body(body: &str) -> String {
    if body.chars().count() <= INPUT_PER_MESSAGE_CHARS {
        return body.to_string();
    }
    format!("{}\n…（原消息过长，后面部分未展开）", body.chars().take(INPUT_PER_MESSAGE_CHARS).collect::<String>())
}

/// 成员看到的是一段普通的聊天记录：谁说了什么、回复了谁。不包含任何调度字段。
fn format_input(conn: &Connection, group: &Group, delta: &[Message], trigger_seq: i64, background_through: Option<i64>) -> Result<String> {
    let mut blocks = Vec::new();
    let mut size = 0usize;
    for message in delta.iter().rev() {
        let block = if message.kind == "message" {
            let quote = match message.reply_to {
                Some(seq) => match message_or_none(conn, group.id, seq)? {
                    Some(quoted) => format!("（回复 {}：「{}」）", speaker(&quoted), quoted.body),
                    None => String::new(),
                },
                None => String::new(),
            };
            format!("{}{quote}：{}", speaker(message), clip_body(&message.body))
        } else {
            // 系统提示是写给用户看的（“你邀请…”）；成员读到的“你”应当指用户，而不是它自己。
            let body = message.body.strip_prefix('你').map(|rest| format!("用户{rest}")).unwrap_or_else(|| message.body.clone());
            format!("〔系统〕{body}")
        };
        let chars = block.chars().count();
        if !blocks.is_empty() && message.seq != trigger_seq && (blocks.len() >= INPUT_MESSAGE_LIMIT || size + chars > INPUT_CHAR_LIMIT) {
            if message.seq < trigger_seq { break; }
            continue;
        }
        size += chars;
        blocks.push((message.seq, block));
    }
    let omitted = delta.len() - blocks.len();
    blocks.reverse();
    let mut text = format!("群聊「{}」的新消息：\n\n", group.name);
    if omitted > 0 {
        text.push_str(&format!("（更早的 {omitted} 条群消息未展开）\n\n"));
    }
    for (i, (seq, block)) in blocks.into_iter().enumerate() {
        if i > 0 { text.push_str("\n\n"); }
        if background_through.is_some_and(|through| seq <= through && seq < trigger_seq) {
            text.push_str("〔此前群聊背景，不重新执行旧请求〕");
        }
        text.push_str(&block);
    }
    Ok(text)
}

fn message_or_none(conn: &Connection, group: i64, seq: i64) -> Result<Option<Message>> {
    Ok(conn.query_row(&format!("{} WHERE group_id=?1 AND seq=?2", public_messages_sql()),
        params![group, seq], public_message_row).optional()?)
}

#[cfg(test)]
#[path = "butler_group_tests.rs"]
mod tests;
