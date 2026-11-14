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
/// 成员之间 @ 接力的最大层数，以及同一条用户消息最多引出的接力次数，防止互相点名形成循环。
pub const MAX_RELAY_DEPTH: i64 = 3;
pub const MAX_RELAYS_PER_ORIGIN: i64 = 12;
/// 用户要求成员讨论时最多进行的轮数。
pub const MAX_DISCUSSION_ROUNDS: i64 = 5;
/// 单次投递给成员的群消息上限；更早的消息只保留数量说明。
const INPUT_MESSAGE_LIMIT: usize = 80;
const INPUT_CHAR_LIMIT: usize = 60_000;
const INPUT_PER_MESSAGE_CHARS: usize = 20_000;
/// 引用别人消息时，在成员输入里保留的原文片段长度。
const QUOTE_EXCERPT_CHARS: usize = 60;
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
    PRIMARY KEY(group_id, seq)
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

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    // 早期版本按顺序串行回复（after_turn），现在同一轮并发回复、多轮讨论按 round/rounds 推进。
    ensure_column(conn, "butler_chat_group_turn", "round", "INTEGER NOT NULL DEFAULT 1")?;
    ensure_column(conn, "butler_chat_group_turn", "rounds", "INTEGER NOT NULL DEFAULT 1")?;
    // 成员发言对应它执行会话里的那条最终回复，界面据此复用私聊的消息信息、代码预览与附件。
    ensure_column(conn, "butler_chat_group_message", "source_conversation_id", "INTEGER")?;
    ensure_column(conn, "butler_chat_group_message", "source_message_id", "INTEGER")?;
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

pub fn active_members(conn: &Connection, group: i64) -> Result<Vec<Member>> {
    Ok(members(conn, group)?.into_iter().filter(|m| m.active).collect())
}

pub fn member(conn: &Connection, id: i64) -> Result<Member> {
    conn.query_row(&format!("SELECT {MEMBER_COLUMNS} FROM butler_chat_group_member WHERE id=?1"), [id], member_row)
        .optional()?.with_context(|| format!("群成员 #{id} 不存在"))
}

pub fn message(conn: &Connection, group: i64, seq: i64) -> Result<Message> {
    conn.query_row(&format!("SELECT {MESSAGE_COLUMNS} FROM butler_chat_group_message WHERE group_id=?1 AND seq=?2"),
        params![group, seq], message_row).optional()?.with_context(|| format!("群聊 #{group} 的消息 #{seq} 不存在"))
}

/// 按时间正序返回最新一页；`before` 用于向上翻页。
pub fn messages(conn: &Connection, group: i64, before: Option<i64>, limit: usize) -> Result<Vec<Message>> {
    ensure!((1..=200).contains(&limit), "群消息分页参数无效：limit={limit}");
    let mut rows = collect(conn, &format!("SELECT {MESSAGE_COLUMNS} FROM butler_chat_group_message
        WHERE group_id=?1 AND seq<?2 ORDER BY seq DESC LIMIT ?3"), params![group, before.unwrap_or(i64::MAX), limit as i64], message_row)?;
    rows.reverse();
    Ok(rows)
}

fn messages_between(conn: &Connection, group: i64, after: i64, through: i64) -> Result<Vec<Message>> {
    collect(conn, &format!("SELECT {MESSAGE_COLUMNS} FROM butler_chat_group_message
        WHERE group_id=?1 AND seq>?2 AND seq<=?3 ORDER BY seq"), params![group, after, through], message_row)
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
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
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
    let seq = get(tx, group)?.seq + 1;
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
    tx.execute("UPDATE butler_chat_group_turn SET state='cancelled', error=?2, updated_at=CURRENT_TIMESTAMP
        WHERE member_id=?1 AND state IN ('queued','running')", params![member_id, reason])?;
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
    Skipped,
}

/// 开始一次回复：固定本次可见的群消息上界，整理成员上次回复后其他人的公开发言。
/// 触发消息已经在更早的回复里送达时跳过，避免同一条消息被同一位成员回复两次。
pub fn start_turn(conn: &mut Connection, id: i64) -> Result<Start> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let t = turn(&tx, id)?;
    if t.state != "queued" {
        return Ok(Start::Skipped);
    }
    let m = member(&tx, t.member_id)?;
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
    let room = get(&tx, t.group_id)?;
    let delta = messages_between(&tx, room.id, m.delivered_through.max(m.joined_after), room.seq)?
        .into_iter().filter(|message| message.sender_member != Some(m.id)).collect::<Vec<_>>();
    if delta.is_empty() {
        skip(&tx, "skipped")?;
        tx.commit()?;
        return Ok(Start::Skipped);
    }
    let input = format_input(&tx, &room, &delta)?;
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

/// 把本次群消息写入成员会话。只有仍在进行的回复可以写入；写入成功后才推进成员的读取位置，
/// 失败或被停止的回复不会让成员“跳过”这些消息。
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
    tx.execute("UPDATE butler_chat_group_member SET delivered_through=max(delivered_through,?2) WHERE id=?1", params![m.id, through])?;
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
    if body.trim() == "[[NO_REPLY]]" {
        return Ok(Evidence::Silent);
    }
    if super::butler_execution::is_completed_content(&kind, &body, finished, calls.as_deref()) {
        return Ok(Evidence::Answer(body.trim().to_string()));
    }
    Ok(Evidence::Pending)
}

pub enum Outcome {
    Reply(String),
    Silent,
    Failed(String),
}

#[derive(Debug, Default)]
pub struct Finished {
    pub reply: Option<Message>,
    pub relayed: Vec<i64>,
}

/// 结束一次回复：公开最终回复，并按回复里的 @ 邀请其他成员接着说（受层数与次数上限约束）；
/// 讨论中的一轮全部说完后进入下一轮。已被停止或移出的回复不会再公开迟到的结果。
pub fn finish_turn(conn: &mut Connection, id: i64, outcome: Outcome) -> Result<Option<Finished>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let t = turn(&tx, id)?;
    if t.state != "running" {
        return Ok(None);
    }
    let m = member(&tx, t.member_id)?;
    let mut finished = Finished::default();
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
            tx.execute("UPDATE butler_chat_group_turn SET state='done', updated_at=CURRENT_TIMESTAMP WHERE id=?1", [id])?;
        }
        Outcome::Failed(reason) => {
            tx.execute("UPDATE butler_chat_group_turn SET state='failed', error=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?1", params![id, reason])?;
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
    let mut blocked = Vec::new();
    for target in parse_mentions(&reply.body, &others) {
        let waiting: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_chat_group_turn WHERE member_id=?1 AND state='queued')",
            [target], |r| r.get(0))?;
        if waiting || discussing.contains(&target) {
            continue;
        }
        let used: i64 = tx.query_row("SELECT count(*) FROM butler_chat_group_turn WHERE group_id=?1 AND origin_seq=?2 AND depth>0",
            params![t.group_id, t.origin_seq], |r| r.get(0))?;
        let name = others.iter().find(|m| m.id == target).map(|m| m.name.clone()).unwrap_or_default();
        if t.depth + 1 > MAX_RELAY_DEPTH || used >= MAX_RELAYS_PER_ORIGIN {
            blocked.push(name);
            continue;
        }
        created.push(insert_turn(tx, NewTurn {
            group: t.group_id, member: target, trigger: reply.seq, origin: t.origin_seq, depth: t.depth + 1, round: 1, rounds: 1,
        })?);
    }
    if !blocked.is_empty() {
        notice(tx, t.group_id, "notice", &format!("成员之间连续接力的次数已达上限，没有再请 {} 回复。需要的话可以直接 @ 对方。", join_names(&blocked)))?;
    }
    Ok(created)
}

/// 停止群里所有排队和进行中的回复。返回需要打断执行的回复。
pub fn stop(conn: &mut Connection, group: i64) -> Result<Vec<Turn>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    get(&tx, group)?;
    let running = collect(&tx, &format!("SELECT {TURN_COLUMNS} FROM butler_chat_group_turn WHERE group_id=?1 AND state='running'"), [group], turn_row)?;
    let routes = tx.execute("UPDATE butler_chat_group_message SET route_state='cancelled' WHERE group_id=?1 AND route_state='pending'", [group])?;
    let turns = tx.execute("UPDATE butler_chat_group_turn SET state='cancelled', error='用户停止了回复', updated_at=CURRENT_TIMESTAMP
        WHERE group_id=?1 AND state IN ('queued','running')", [group])?;
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

fn excerpt(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    format!("{}…", flat.chars().take(limit).collect::<String>())
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
fn format_input(conn: &Connection, group: &Group, delta: &[Message]) -> Result<String> {
    let mut blocks = Vec::new();
    let mut size = 0usize;
    for message in delta.iter().rev() {
        let block = if message.kind == "message" {
            let quote = match message.reply_to {
                Some(seq) => match message_or_none(conn, group.id, seq)? {
                    Some(quoted) => format!("（回复 {}：「{}」）", speaker(&quoted), excerpt(&quoted.body, QUOTE_EXCERPT_CHARS)),
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
        size += block.chars().count();
        if !blocks.is_empty() && (blocks.len() >= INPUT_MESSAGE_LIMIT || size > INPUT_CHAR_LIMIT) {
            break;
        }
        blocks.push(block);
    }
    let omitted = delta.len() - blocks.len();
    blocks.reverse();
    let mut text = format!("群聊「{}」的新消息：\n\n", group.name);
    if omitted > 0 {
        text.push_str(&format!("（更早的 {omitted} 条群消息未展开）\n\n"));
    }
    text.push_str(&blocks.join("\n\n"));
    Ok(text)
}

fn message_or_none(conn: &Connection, group: i64, seq: i64) -> Result<Option<Message>> {
    Ok(conn.query_row(&format!("SELECT {MESSAGE_COLUMNS} FROM butler_chat_group_message WHERE group_id=?1 AND seq=?2"),
        params![group, seq], message_row).optional()?)
}

#[cfg(test)]
#[path = "butler_group_tests.rs"]
mod tests;
