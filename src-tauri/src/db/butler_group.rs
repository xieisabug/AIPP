//! Durable public group log and membership epochs. Private execution records are
//! never read by the public log API. All mutating entry points own an IMMEDIATE transaction.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub fn schema(conn: &Connection) -> rusqlite::Result<()> {
    // A savepoint also works when the database initializer owns a transaction.
    // Never leave half of the execution-identity migration installed.
    conn.execute_batch("SAVEPOINT butler_group_schema")?;
    match migrate_schema(conn) {
        Ok(()) => conn.execute_batch("RELEASE butler_group_schema"),
        Err(error) => {
            conn.execute_batch("ROLLBACK TO butler_group_schema; RELEASE butler_group_schema")?;
            Err(error)
        }
    }
}

fn migrate_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(include_str!("butler_group_schema.sql"))?;
    let has_reference:bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_group_event') WHERE name='reference_only')",[],|r|r.get(0))?;
    if !has_reference { conn.execute("ALTER TABLE butler_group_event ADD COLUMN reference_only INTEGER NOT NULL DEFAULT 0",[])?; }
    let has_snapshot:bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_group_inbox') WHERE name='snapshot_seq')",[],|r|r.get(0))?;
    if !has_snapshot { conn.execute("ALTER TABLE butler_group_inbox ADD COLUMN snapshot_seq INTEGER",[])?; }
    let has_wait:bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_group') WHERE name='seat_wait_seconds')",[],|r|r.get(0))?;
    if !has_wait { conn.execute("ALTER TABLE butler_group ADD COLUMN seat_wait_seconds INTEGER NOT NULL DEFAULT 120",[])?; }
    for table in ["butler_group_packet","butler_group_inbox","butler_group_preparation"] {
        let has:bool=conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name='conversation_id')"),[],|r|r.get(0))?;
        if !has {conn.execute(&format!("ALTER TABLE {table} ADD COLUMN conversation_id INTEGER REFERENCES conversation(id)"),[])?;}
        conn.execute(&format!("UPDATE {table} SET conversation_id=(SELECT conversation_id FROM butler_group_member m WHERE m.id={table}.member_id) WHERE conversation_id IS NULL"),[])?;
    }
    let has:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_group_dependency') WHERE name='target_conversation_id')",[],|r|r.get(0))?;
    if !has {conn.execute("ALTER TABLE butler_group_dependency ADD COLUMN target_conversation_id INTEGER REFERENCES conversation(id)",[])?;}
    conn.execute("UPDATE butler_group_dependency SET target_conversation_id=(SELECT conversation_id FROM butler_group_member WHERE id=target_member) WHERE target_conversation_id IS NULL",[])?;
    let has:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_group_event') WHERE name='sender_conversation')",[],|r|r.get(0))?;
    if !has {conn.execute("ALTER TABLE butler_group_event ADD COLUMN sender_conversation INTEGER REFERENCES conversation(id)",[])?;}
    let has:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_group_event') WHERE name='sender_packet')",[],|r|r.get(0))?;
    if !has {conn.execute("ALTER TABLE butler_group_event ADD COLUMN sender_packet INTEGER REFERENCES butler_group_packet(id)",[])?;}
    let has:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_group_dispatch') WHERE name='return_to_packet')",[],|r|r.get(0))?;
    if !has {conn.execute("ALTER TABLE butler_group_dispatch ADD COLUMN return_to_packet INTEGER REFERENCES butler_group_packet(id)",[])?;}
    // Older public requests can be bound only when persisted dispatch/dependency
    // ownership identifies exactly one packet in the original sender execution.
    // Never infer ownership from the latest message or from an agent-chosen key.
    conn.execute_batch("WITH owners(group_id,event_seq,packet,conversation) AS (
        SELECT m.group_id,d.event_seq,p.id,p.conversation_id FROM butler_group_dispatch d
        JOIN butler_group_packet p ON p.id=d.owner_packet JOIN butler_group_member m ON m.id=p.member_id
        UNION SELECT m.group_id,d.request_seq,p.id,p.conversation_id FROM butler_group_dependency d
        JOIN butler_group_packet p ON p.id=d.owner_packet JOIN butler_group_member m ON m.id=p.member_id
    ) UPDATE butler_group_event SET sender_packet=(SELECT min(o.packet) FROM owners o
        WHERE o.group_id=butler_group_event.group_id AND o.event_seq=butler_group_event.seq
        AND o.conversation=butler_group_event.sender_conversation)
        WHERE sender_packet IS NULL AND 1=(SELECT count(DISTINCT o.packet) FROM owners o
        WHERE o.group_id=butler_group_event.group_id AND o.event_seq=butler_group_event.seq
        AND o.conversation=butler_group_event.sender_conversation);")?;
    let has:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_group_stop_report') WHERE name='conversation_id')",[],|r|r.get(0))?;
    if !has {conn.execute("ALTER TABLE butler_group_stop_report ADD COLUMN conversation_id INTEGER REFERENCES conversation(id)",[])?;}
    conn.execute("INSERT OR IGNORE INTO butler_group_execution(conversation_id,member_id,kind,delivered_through) SELECT conversation_id,id,'main',delivered_through FROM butler_group_member",[])?;
    if has_legacy_inbox_uniqueness(conn)? {
        // No table references inbox IDs; retain all IDs and frozen packet links.
        // SQLite's inline UNIQUE constraint requires a table rebuild.
        conn.execute_batch("CREATE TABLE butler_group_inbox_execution_migration (
            id INTEGER PRIMARY KEY, group_id INTEGER NOT NULL REFERENCES butler_group(id) ON DELETE CASCADE,
            member_id INTEGER NOT NULL REFERENCES butler_group_member(id) ON DELETE CASCADE,
            conversation_id INTEGER NOT NULL REFERENCES conversation(id),
            event_seq INTEGER NOT NULL, reason TEXT NOT NULL, round_id INTEGER REFERENCES butler_group_round(id),
            state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','prepared','delivered','answered','waiting','failed','cancelled','uncertain')),
            packet_id INTEGER, error TEXT, snapshot_seq INTEGER,
            UNIQUE(conversation_id,event_seq,reason),
            FOREIGN KEY(group_id,event_seq) REFERENCES butler_group_event(group_id,seq)
        );
        INSERT INTO butler_group_inbox_execution_migration
            (id,group_id,member_id,conversation_id,event_seq,reason,round_id,state,packet_id,error,snapshot_seq)
            SELECT id,group_id,member_id,conversation_id,event_seq,reason,round_id,state,packet_id,error,snapshot_seq FROM butler_group_inbox;
        DROP TABLE butler_group_inbox;
        ALTER TABLE butler_group_inbox_execution_migration RENAME TO butler_group_inbox;
        CREATE INDEX idx_butler_group_inbox_queue ON butler_group_inbox(member_id,state,id);")?;
    }
    conn.execute_batch("DROP INDEX IF EXISTS idx_butler_group_one_active_packet;
        DROP INDEX IF EXISTS idx_butler_group_preparation_active;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_butler_group_execution_packet ON butler_group_packet(conversation_id) WHERE state IN ('prepared','delivered','running','uncertain');
        CREATE UNIQUE INDEX IF NOT EXISTS idx_butler_group_execution_preparation ON butler_group_preparation(conversation_id) WHERE state='pending';
        CREATE INDEX IF NOT EXISTS idx_butler_group_execution_queue ON butler_group_inbox(conversation_id,state,id);")?;
    Ok(())
}

fn has_legacy_inbox_uniqueness(conn:&Connection)->rusqlite::Result<bool> {
    // Inspect SQLite's parsed schema: SQL case, quoting and column order do not
    // change uniqueness semantics. Source-text matching could leave the old
    // constraint in place and prevent two executions receiving one event.
    let mut stmt=conn.prepare("SELECT name FROM pragma_index_list('butler_group_inbox') WHERE \"unique\"=1")?;
    let names=stmt.query_map([],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for name in names {
        let mut columns=conn.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
        let mut names=columns.query_map([name],|row|row.get::<_,Option<String>>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        names.iter_mut().for_each(|name| {if let Some(name)=name {name.make_ascii_lowercase();}});
        names.sort();
        if names==vec![Some("event_seq".into()),Some("member_id".into()),Some("reason".into())] {return Ok(true);}
    }
    Ok(false)
}

#[cfg(test)]
#[path="butler_group_schema_tests.rs"]
mod schema_tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub goal: String,
    pub chief_member: Option<i64>,
    pub seq: i64,
    pub state: String,
    pub seat_wait_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind { Message, Progress, Result, Question, Notice }

impl MessageKind {
    fn as_str(&self) -> &'static str {
        match self { Self::Message => "message", Self::Progress => "progress", Self::Result => "result",
            Self::Question => "question", Self::Notice => "notice" }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub group_id: i64,
    pub seq: i64,
    pub sender_member: Option<i64>,
    #[serde(default)]
    pub sender_conversation:Option<i64>,
    pub sender_name: String,
    pub kind: String,
    pub body: String,
    pub reply_to: Option<i64>,
    pub reference_only: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupView {
    pub group: Group,
    pub members: Vec<Member>,
}

fn group_row(row: &Row<'_>) -> rusqlite::Result<Group> {
    Ok(Group { id: row.get(0)?, name: row.get(1)?, goal: row.get(2)?, chief_member: row.get(3)?,
        seq: row.get(4)?, state: row.get(5)?,seat_wait_seconds:row.get(6)? })
}
pub(super) fn member_row(row: &Row<'_>) -> rusqlite::Result<Member> {
    Ok(Member { id: row.get(0)?, group_id: row.get(1)?, assistant_id: row.get(2)?, name: row.get(3)?,
        conversation_id: row.get(4)?, joined_after: row.get(5)?, delivered_through: row.get(6)?, active: row.get(7)? })
}
pub(super) const MEMBER_COLUMNS: &str = "id,group_id,assistant_id,name,conversation_id,joined_after,delivered_through,active";
const EVENT_COLUMNS: &str = "group_id,seq,sender_member,sender_name,kind,body,reply_to,created_at,reference_only,sender_conversation";
fn event_row(row: &Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event { group_id: row.get(0)?, seq: row.get(1)?, sender_member: row.get(2)?, sender_name: row.get(3)?,
        kind: row.get(4)?, body: row.get(5)?, reply_to: row.get(6)?, created_at: row.get(7)?,reference_only:row.get(8)?,sender_conversation:row.get(9)? })
}

pub fn get(conn: &Connection, group: i64) -> Result<Group> {
    conn.query_row("SELECT id,name,goal,chief_member,seq,state,seat_wait_seconds FROM butler_group WHERE id=?1",
        [group], group_row).optional()?.with_context(|| format!("协作群 #{group} 不存在"))
}
pub fn list(conn: &Connection) -> Result<Vec<Group>> {
    let mut stmt = conn.prepare("SELECT id,name,goal,chief_member,seq,state,seat_wait_seconds FROM butler_group ORDER BY id DESC")?;
    let rows = stmt.query_map([], group_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
pub fn members(conn: &Connection, group: i64) -> Result<Vec<Member>> {
    let mut stmt = conn.prepare(&format!("SELECT {MEMBER_COLUMNS} FROM butler_group_member WHERE group_id=?1 ORDER BY id"))?;
    let rows = stmt.query_map([group], member_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
pub fn member(conn: &Connection, id: i64) -> Result<Member> {
    conn.query_row(&format!("SELECT {MEMBER_COLUMNS} FROM butler_group_member WHERE id=?1"), [id], member_row)
        .optional()?.with_context(|| format!("群成员 #{id} 不存在"))
}
pub fn view(conn: &Connection, group: i64) -> Result<GroupView> {
    Ok(GroupView { group: get(conn, group)?, members: members(conn, group)? })
}
pub fn event(conn: &Connection, group: i64, seq: i64) -> Result<Event> {
    conn.query_row(&format!("SELECT {EVENT_COLUMNS} FROM butler_group_event WHERE group_id=?1 AND seq=?2"),
        params![group, seq], event_row).optional()?.context("群消息不存在")
}

/// UI is the local user's view. Agent reads MUST use read_as_member instead.
pub fn events(conn: &Connection, group: i64, after: i64, limit: usize) -> Result<Vec<Event>> {
    get(conn, group)?;
    ensure!((1..=200).contains(&limit) && after >= 0, "群消息分页参数无效");
    let mut stmt = conn.prepare(&format!("SELECT {EVENT_COLUMNS} FROM butler_group_event WHERE group_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3"))?;
    let rows = stmt.query_map(params![group, after, limit as i64], event_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn read_as_member(conn: &Connection, member_id: i64, seq: i64) -> Result<Event> {
    let m = member(conn, member_id)?;
    ensure!(m.active, "成员已退出，不能读取群消息");
    // Sharing is checked by exact public event identity, never a conversation-wide grant.
    let shared: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_share WHERE member_id=?1 AND event_seq=?2)",
        params![member_id, seq], |r| r.get(0))?;
    ensure!(seq > m.joined_after || shared, "该历史消息未向此成员分享");
    event(conn, m.group_id, seq)
}

pub(super) fn add_member_in(conn: &Connection, group: &Group, assistant: i64, name: &str) -> Result<i64> {
    ensure!(group.state == "active", "协作群已停止，不能加入成员");
    ensure!(assistant > 0 && !name.trim().is_empty(), "助手身份或名称无效");
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_member WHERE group_id=?1 AND assistant_id=?2 AND active=1)",
        params![group.id, assistant], |r| r.get(0))?;
    ensure!(!exists, "助手已经是本群成员");
    conn.execute("INSERT INTO conversation(name,assistant_id,conversation_kind,parent_butler_conversation_id,is_hidden_from_normal_chat_list)
        VALUES(?1,?2,'butler_group_member',?3,1)", params![format!("{name} · {}", group.name), assistant, group.id])?;
    let conversation = conn.last_insert_rowid();
    conn.execute("INSERT INTO butler_group_member(group_id,assistant_id,name,conversation_id,joined_after,delivered_through)
        VALUES(?1,?2,?3,?4,?5,?5)", params![group.id, assistant, name, conversation, group.seq])?;
    let member=conn.last_insert_rowid();
    conn.execute("INSERT INTO butler_group_execution(conversation_id,member_id,kind,delivered_through) VALUES(?1,?2,'main',?3)",params![conversation,member,group.seq])?;
    Ok(member)
}

pub fn create(conn: &mut Connection, name: &str, goal: &str, chief: (i64, String), invited: &[(i64, String)]) -> Result<GroupView> {
    ensure!(!name.trim().is_empty() && name.chars().count() <= 120, "群名须为 1–120 字符");
    ensure!(!goal.trim().is_empty() && goal.chars().count() <= 24000, "目标须为 1–24000 字符");
    ensure!(invited.len() <= 30, "协作群最多邀请 30 名助手");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute("INSERT INTO conversation(name,conversation_kind,is_hidden_from_normal_chat_list) VALUES(?1,'butler_group',1)", [name])?;
    let id = tx.last_insert_rowid();
    tx.execute("INSERT INTO butler_group(id,name,goal) VALUES(?1,?2,?3)", params![id, name, goal])?;
    let group = get(&tx, id)?;
    let chief_member = add_member_in(&tx, &group, chief.0, &chief.1)?;
    tx.execute("UPDATE butler_group SET chief_member=?1 WHERE id=?2", params![chief_member, id])?;
    for (assistant, label) in invited { add_member_in(&tx, &group, *assistant, label)?; }
    let result = view(&tx, id)?;
    tx.commit()?;
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub request_key: String,
    pub kind: MessageKind,
    pub body: String,
    pub reply_to: Option<i64>,
    #[serde(default)]
    pub reference_only: bool,
    pub recipients: Vec<i64>,
}

/// Sender is derived by the service from an execution identity, not from model arguments.
pub fn post(conn: &mut Connection, group: i64, sender: Option<i64>, post: &Post) -> Result<Event> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let result = post_in(&tx, group, sender, post)?;
    tx.commit()?;
    Ok(result)
}

pub(super) fn post_in(tx: &Connection, group: i64, sender: Option<i64>, post: &Post) -> Result<Event> {
    let room = get(tx, group)?;
    ensure!(room.state == "active", "协作群已停止，不能派发新消息");
    ensure!(!post.request_key.trim().is_empty() && post.request_key.len() <= 128, "需要稳定的消息请求标识（1–128 字节）");
    ensure!(!post.body.trim().is_empty() && post.body.chars().count() <= 100000, "公开消息正文须为 1–100000 字符");
    let sender_name = if let Some(id) = sender {
        let m = member(&tx, id)?;
        ensure!(m.active && m.group_id == group, "发送者不属于本群");
        m.name
    } else { "用户".to_string() };
    let key = format!("{}:{}", sender.unwrap_or(0), post.request_key);
    let canonical = serde_json::to_string(post)?;
    let previous: Option<(i64, String)> = tx.query_row("SELECT seq,request_json FROM butler_group_event WHERE group_id=?1 AND request_key=?2",
        params![group, key], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    if let Some((seq, original)) = previous {
        ensure!(canonical == original, "请求标识已用于不同的群消息，不能重复发送");
        return event(&tx, group, seq);
    }
    if let Some(seq) = post.reply_to {
        if let Some(id) = sender { read_as_member(&tx, id, seq)?; } else { event(&tx, group, seq)?; }
    }
    let current = members(&tx, group)?.into_iter().filter(|m| m.active).map(|m| m.id).collect::<Vec<_>>();
    let activation = if !post.recipients.is_empty() {
        super::butler_group_context::Activation::Directed(post.recipients.clone())
    } else if sender.is_none() && post.kind == MessageKind::Message {
        // User replies default to the original member. A mere citation must be
        // represented separately from reply_to by the caller.
        let reply_member = if post.reference_only { None } else { post.reply_to.map(|seq| event(&tx, group, seq)).transpose()?.and_then(|e|e.sender_member) };
        match reply_member {
            Some(id) => super::butler_group_context::Activation::Directed(vec![id]),
            None => super::butler_group_context::Activation::UserPlain,
        }
    } else { super::butler_group_context::Activation::Background };
    let targets = super::butler_group_context::recipients(&activation, room.chief_member.context("协作群缺少协调者")?, &current)
        .map_err(anyhow::Error::msg)?;
    ensure!(!sender.is_some_and(|id|targets.contains(&id)), "不能通过群消息唤醒自己");
    if let Some(quoted) = post.reply_to {
        for target in &targets {
            read_as_member(tx,*target,quoted).with_context(||format!("回复接收成员 #{target} 无权读取原消息；请先明确分享该消息"))?;
        }
    }
    let seq = room.seq + 1;
    tx.execute("INSERT INTO butler_group_event(group_id,seq,sender_member,sender_name,kind,body,reply_to,request_key,request_json,reference_only)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![group,seq,sender,sender_name,post.kind.as_str(),post.body,post.reply_to,key,canonical,post.reference_only])?;
    tx.execute("UPDATE butler_group SET seq=?1 WHERE id=?2",params![seq,group])?;
    let reply_execution=if !post.reference_only {
        post.reply_to.map(|id|event(tx,group,id)).transpose()?.and_then(|e|e.sender_conversation)
    } else {None};
    for target in targets {
        if let Some(conversation)=reply_execution {
            let e=super::butler_group_execution::get(tx,conversation)?;
            if e.id==target && e.task {
                super::butler_group_execution::enqueue_in(tx,conversation,seq,&format!("task-reply:{conversation}"),None)?;
                continue;
            }
        }
        enqueue_in(&tx, group, target, seq, "direct", None)?;
    }
    let result = event(&tx,group,seq)?;
    Ok(result)
}

pub fn add_member(conn: &mut Connection, group_id: i64, assistant: i64, name: &str) -> Result<Member> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let room = get(&tx,group_id)?;
    let id = add_member_in(&tx,&room,assistant,name)?;
    post_in(&tx,group_id,None,&Post { request_key:format!("member:{id}:joined"),kind:MessageKind::Notice,
        body:format!("{name} 加入协作群。默认只接收加入后的消息和明确分享的资料。"),reply_to:None,reference_only:false,recipients:vec![] })?;
    let result = member(&tx,id)?;
    tx.commit()?;
    Ok(result)
}

pub fn remove_member(conn: &mut Connection, group_id: i64, member_id: i64) -> Result<Member> {
    Ok(remove_member_with_stopped(conn,group_id,member_id)?.0)
}

pub fn remove_member_with_stopped(conn:&mut Connection,group_id:i64,member_id:i64)->Result<(Member,Vec<super::butler_group_execution::Scope>)> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let m = member(&tx,member_id)?;
    ensure!(m.group_id == group_id,"成员不属于该群");
    let room = get(&tx,group_id)?;
    ensure!(room.chief_member != Some(member_id),"不能移除默认协调者");
    if !m.active { return Ok((m,vec![])); }
    post_in(&tx,group_id,None,&Post { request_key:format!("member:{member_id}:removed"),kind:MessageKind::Notice,
        body:format!("{} 已移出协作群，其本群工作正在停止。已经读取的资料无法撤回。",m.name),reply_to:None,reference_only:false,recipients:vec![] })?;
    tx.execute("UPDATE butler_group_member SET active=0 WHERE id=?1",[member_id])?;
    let stopped=fence_member_in(&tx,member_id,"成员已退出，后续投递已取消")?;
    super::butler_group_delivery::release_member_rounds_in(&tx,group_id,member_id)?;
    let result = member(&tx,member_id)?;
    tx.commit()?;
    Ok((result,stopped))
}

pub(super) fn fence_member_in(conn:&Connection,member_id:i64,reason:&str) -> Result<Vec<super::butler_group_execution::Scope>> {
    let mut stmt=conn.prepare("SELECT conversation_id FROM butler_group_execution WHERE member_id=?1")?;
    let seeds=stmt.query_map([member_id],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    super::butler_group_stop::fence(conn,&seeds,reason)
}

pub fn stop(conn:&mut Connection,group_id:i64) -> Result<Vec<Member>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let room = get(&tx,group_id)?;
    let members = members(&tx,group_id)?;
    if room.state == "active" {
        post_in(&tx,group_id,None,&Post { request_key:"group:stopped".into(),kind:MessageKind::Notice,
            body:"用户停止本群工作。已封锁后续派发，正在逐项取消执行；已发生的外部操作不保证撤销。".into(),reply_to:None,reference_only:false,recipients:vec![] })?;
        tx.execute("UPDATE butler_group SET state='stopped' WHERE id=?1",[group_id])?;
        tx.execute("UPDATE butler_group_round SET state='cancelled' WHERE group_id=?1 AND state='active'",[group_id])?;
        for m in &members { fence_member_in(&tx,m.id,"本群工作已停止")?; }
    }
    tx.commit()?;
    Ok(members)
}

/// Explicit user action only. Sharing never extends to neighboring messages.
pub fn stop_member(conn:&mut Connection,group_id:i64,member_id:i64) -> Result<Member> {
    Ok(stop_member_with_stopped(conn,group_id,member_id)?.0)
}

pub fn stop_member_with_stopped(conn:&mut Connection,group_id:i64,member_id:i64)->Result<(Member,Vec<super::butler_group_execution::Scope>)> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let m = member(&tx,member_id)?;
    ensure!(m.active && m.group_id == group_id,"不是本群有效成员");
    let stopped=fence_member_in(&tx,member_id,"用户停止该成员当前群工作")?;
    super::butler_group_delivery::release_member_rounds_in(&tx,group_id,member_id)?;
    let seq = get(&tx,group_id)?.seq;
    post_in(&tx,group_id,None,&Post { request_key:format!("stop-member:{member_id}:{seq}"),kind:MessageKind::Notice,
        body:format!("用户停止了 {} 当前的群内工作。",m.name),reply_to:None,reference_only:false,recipients:vec![] })?;
    tx.commit()?;
    Ok((m,stopped))
}

/// Explicit user replacement. Existing inputs stay byte-for-byte intact; old
/// execution identities are fenced before the new goal is appended.
pub fn replace_goal(conn:&mut Connection,group_id:i64,goal:&str,request_key:&str,expected_members:&[i64]) -> Result<bool> {
    ensure!(!goal.trim().is_empty() && goal.chars().count()<=24000,"目标须为 1–24000 字符");
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let room=get(&tx,group_id)?;
    ensure!(room.state=="active","协作群已停止，不能替换目标");
    let post=Post { request_key:request_key.into(),kind:MessageKind::Message,
        body:format!("用户明确替换群目标。此前未完成工作已停止；新的目标是：\n{goal}"),reply_to:None,reference_only:false,recipients:vec![] };
    let previous:Option<String>=tx.query_row("SELECT request_json FROM butler_group_event WHERE group_id=?1 AND request_key=?2",
        params![group_id,format!("0:{request_key}")],|r|r.get(0)).optional()?;
    if let Some(previous)=previous {
        ensure!(previous==serde_json::to_string(&post)?,"目标替换请求标识已用于不同内容");
        return Ok(false);
    }
    let members=members(&tx,group_id)?.into_iter().filter(|m|m.active).collect::<Vec<_>>();
    let actual=members.iter().map(|m|m.id).collect::<Vec<_>>();
    ensure!(actual==expected_members,"成员列表在替换目标时发生变化，请重试；未更改目标");
    tx.execute("UPDATE butler_group_round SET state='cancelled' WHERE group_id=?1 AND state='active'",[group_id])?;
    // Cancel all owned dependencies before fencing any recipient, so replacing
    // the whole goal cannot enqueue callbacks from the discarded work.
    tx.execute("UPDATE butler_group_dependency SET state='cancelled' WHERE owner_packet IN (
        SELECT p.id FROM butler_group_packet p JOIN butler_group_member m ON m.id=p.member_id WHERE m.group_id=?1)
        AND state IN ('pending','ready')",[group_id])?;
    for member in members { fence_member_in(&tx,member.id,"用户已明确替换群目标，旧工作不得继续")?; }
    tx.execute("UPDATE butler_group SET goal=?1 WHERE id=?2",params![goal,group_id])?;
    post_in(&tx,group_id,None,&post)?;
    tx.commit()?;
    Ok(true)
}

pub fn set_wait_limit(conn:&mut Connection,group_id:i64,seconds:i64,request_key:&str) -> Result<()> {
    ensure!((5..=3600).contains(&seconds),"发言席位等待时间应为 5–3600 秒");
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let already_applied:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_event WHERE group_id=?1 AND request_key=?2)",
        params![group_id,format!("0:{request_key}")],|r|r.get(0))?;
    post_in(&tx,group_id,None,&Post { request_key:request_key.into(),kind:MessageKind::Notice,
        body:format!("用户将每个发言席位的等待上限设为 {seconds} 秒；到期让出席位，原工作继续。"),reply_to:None,reference_only:false,recipients:vec![] })?;
    // Validate retry content through post_in, but never roll back a newer setting.
    if !already_applied {
        tx.execute("UPDATE butler_group SET seat_wait_seconds=?1 WHERE id=?2",params![seconds,group_id])?;
    }
    tx.commit()?; Ok(())
}

/// Explicit user action only. Sharing never extends to neighboring messages.
pub fn share(conn:&mut Connection,group_id:i64,member_id:i64,sequences:&[i64],request_key:&str) -> Result<()> {
    ensure!(!sequences.is_empty() && sequences.len() <= 100,"每次分享 1–100 条消息");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let m = member(&tx,member_id)?;
    ensure!(m.active && m.group_id == group_id,"接收者不是本群有效成员");
    for seq in sequences {
        event(&tx,group_id,*seq)?;
        tx.execute("INSERT INTO butler_group_share(member_id,event_seq) VALUES(?1,?2) ON CONFLICT DO NOTHING",params![member_id,seq])?;
    }
    post_in(&tx,group_id,None,&Post { request_key:request_key.into(),kind:MessageKind::Notice,
        body:format!("用户向 {} 分享了群历史消息 {}。",m.name,serde_json::to_string(sequences)?),reply_to:None,reference_only:false,recipients:vec![] })?;
    tx.commit()?;
    Ok(())
}

pub(super) fn enqueue_in(conn: &Connection, group: i64, member: i64, seq: i64, reason: &str, round: Option<i64>) -> Result<()> {
    let conversation=self::member(conn,member)?.conversation_id;
    super::butler_group_execution::enqueue_in(conn,conversation,seq,reason,round)?;
    ensure!(self::member(conn,member)?.group_id==group,"收件箱群归属不匹配");
    Ok(())
}

/// Member-session resolution is the only agent entry point for this domain.
pub fn scope(conn: &Connection, conversation: i64) -> Result<super::butler_group_execution::Scope> {
    super::butler_group_execution::scope(conn,conversation)
}
