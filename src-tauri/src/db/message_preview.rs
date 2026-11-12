//! `preview_message`：把引用解析成一条具体的助手回复，并按管家可见范围读取原文。
//!
//! 模型通常不知道消息 ID，所以除了 `message_id` 还接受它已经掌握的工作句柄：
//! `exchange:<id>`（协作答复）、`task:<任务会话 id>`（独立任务最终答复）、
//! `scheduled_run:<run_id>`（管家定时任务的运行产出）。
use rusqlite::{params, Connection, OptionalExtension};

/// 可引用的会话：管家会话（与 `butler_history` 范围一致，群聊不在内）与管家定时任务的运行会话。
pub const PREVIEWABLE_BUTLER_KINDS: [&str; 4] =
    ["butler_main", "butler_contact", "butler_contact_archive", "butler_task"];

/// 定时任务的运行会话：隐藏保存每次运行的过程（按任务保留最近若干个），可从投递消息或任务详情打开。
pub const SCHEDULED_RUN_KIND: &str = "scheduled_run";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRef {
    Message(i64),
    Exchange(i64),
    Task(i64),
    ScheduledRun(String),
}

impl MessageRef {
    /// `message_id` 与 `work_ref` 二选一。
    pub fn parse(message_id: Option<i64>, work_ref: Option<&str>) -> Result<Self, String> {
        let work_ref = work_ref.map(str::trim).filter(|value| !value.is_empty());
        match (message_id, work_ref) {
            (Some(_), Some(_)) => Err("message_id 与 work_ref 只能填一个".into()),
            (None, None) => Err("需要 message_id，或 work_ref（exchange:<id> / task:<id> / scheduled_run:<run_id>）".into()),
            (Some(id), None) if id > 0 => Ok(Self::Message(id)),
            (Some(id), None) => Err(format!("message_id 应为正整数，收到 {id}")),
            (None, Some(value)) => Self::parse_work_ref(value),
        }
    }

    fn parse_work_ref(value: &str) -> Result<Self, String> {
        let invalid = || {
            format!("无法识别的 work_ref「{value}」，应为 exchange:<id>、task:<任务会话 id> 或 scheduled_run:<run_id>")
        };
        let (kind, id) = value.split_once(':').ok_or_else(invalid)?;
        let id = id.trim();
        let number = || id.parse::<i64>().ok().filter(|n| *n > 0).ok_or_else(invalid);
        match kind.trim() {
            "message" | "msg" => Ok(Self::Message(number()?)),
            "exchange" => Ok(Self::Exchange(number()?)),
            "task" => Ok(Self::Task(number()?)),
            "scheduled_run" if !id.is_empty() => Ok(Self::ScheduledRun(id.to_string())),
            _ => Err(invalid()),
        }
    }
}

/// 把引用解析成消息 ID。工作句柄指向那项工作的最终答复。
pub fn resolve(conn: &Connection, reference: &MessageRef) -> Result<i64, String> {
    let db = |e: rusqlite::Error| format!("读取引用失败: {e}");
    match reference {
        MessageRef::Message(id) => Ok(*id),
        MessageRef::Exchange(id) => {
            let row: Option<(String, Option<i64>)> = conn
                .query_row(
                    "SELECT e.state, (SELECT p.anchor FROM butler_peer_post p
                        WHERE p.exchange_id=e.id AND p.leg='reply' AND p.outgoing=1)
                       FROM butler_peer_exchange e WHERE e.id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(db)?;
            let (state, anchor) = row.ok_or_else(|| format!("协作 #{id} 不存在或已删除"))?;
            anchor.ok_or_else(|| format!("协作 #{id} 还没有可展示的答复（状态 {state}）"))
        }
        MessageRef::Task(id) => {
            let row: Option<Option<i64>> = conn
                .query_row(
                    "SELECT final_message_id FROM butler_task_result WHERE task_conversation_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db)?;
            row.flatten().ok_or_else(|| format!("任务 #{id} 还没有最终答复，或任务不存在"))
        }
        MessageRef::ScheduledRun(run_id) => {
            let row: Option<Option<i64>> = conn
                .query_row(
                    "SELECT final_message_id FROM scheduled_task_run WHERE run_id=?1",
                    [run_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db)?;
            match row {
                None => Err(format!("定时任务运行 {run_id} 不存在或已删除")),
                Some(None) => Err(format!(
                    "定时任务运行 {run_id} 没有保留可展示的产出（失败、被停止，或是旧版本的运行）"
                )),
                Some(Some(message)) => Ok(message),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreviewRow {
    pub message_id: i64,
    pub conversation_id: i64,
    pub conversation_kind: String,
    pub conversation_name: String,
    pub assistant_id: Option<i64>,
    pub message_type: String,
    pub content: String,
    pub origin: Option<String>,
    pub created_time: String,
    pub archived: bool,
    /// 消息属于管家定时任务的运行会话时，是该定时任务的名称。
    pub scheduled_task_name: Option<String>,
}

pub fn load(conn: &Connection, message_id: i64) -> rusqlite::Result<Option<PreviewRow>> {
    conn.query_row(
        "SELECT m.id, c.id, c.conversation_kind, coalesce(c.name,''), c.assistant_id, m.message_type, m.content,
                CASE WHEN json_valid(m.metadata_json) THEN json_extract(m.metadata_json,'$.origin') END,
                m.created_time,
                c.conversation_kind='butler_contact_archive' OR (c.conversation_kind='butler_main'
                    AND NOT EXISTS(SELECT 1 FROM butler_main_state s WHERE s.butler_conversation_id=c.id)),
                (SELECT t.name FROM scheduled_task_run r JOIN scheduled_task t ON t.id=r.task_id
                  WHERE r.conversation_id=c.id AND COALESCE(t.butler_mode, 0)=1 LIMIT 1)
           FROM message m JOIN conversation c ON c.id=m.conversation_id
          WHERE m.id=?1",
        [message_id],
        |r| {
            Ok(PreviewRow {
                message_id: r.get(0)?,
                conversation_id: r.get(1)?,
                conversation_kind: r.get(2)?,
                conversation_name: r.get(3)?,
                assistant_id: r.get(4)?,
                message_type: r.get(5)?,
                content: r.get(6)?,
                origin: r.get(7)?,
                created_time: r.get(8)?,
                archived: r.get(9)?,
                scheduled_task_name: r.get(10)?,
            })
        },
    )
    .optional()
}

/// 只允许展示管家范围内的助手回复；普通聊天、群聊、系统/工具/思考消息与内部回合都不行。
pub fn ensure_previewable(row: &PreviewRow) -> Result<(), String> {
    let id = row.message_id;
    let in_scope = PREVIEWABLE_BUTLER_KINDS.contains(&row.conversation_kind.as_str())
        || row.conversation_kind == SCHEDULED_RUN_KIND
        || row.scheduled_task_name.is_some();
    if !in_scope {
        return Err(format!("消息 #{id} 不在管家会话或管家定时任务里，不能展示"));
    }
    if !matches!(row.message_type.as_str(), "response" | "assistant") {
        return Err(format!("只能展示助手的回复，消息 #{id} 的类型是 {}", row.message_type));
    }
    if row.origin.as_deref() == Some("internal") {
        return Err(format!("消息 #{id} 属于内部回合，不能展示"));
    }
    Ok(())
}

/// 某个定时任务保留的隐藏运行会话中，除最新 `keep` 个以外的会话（用于按任务限量保留）。
pub fn scheduled_run_conversation_ids_beyond(
    conn: &Connection,
    task_id: i64,
    keep: u32,
) -> rusqlite::Result<Vec<i64>> {
    conn.prepare(
        "SELECT r.conversation_id FROM scheduled_task_run r JOIN conversation c ON c.id=r.conversation_id
          WHERE r.task_id=?1 AND c.conversation_kind=?2
          ORDER BY r.started_time DESC, r.id DESC LIMIT -1 OFFSET ?3",
    )?
    .query_map(params![task_id, SCHEDULED_RUN_KIND, keep], |r| r.get(0))?
    .collect()
}

#[cfg(test)]
#[path = "message_preview_tests.rs"]
mod tests;
