use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{prelude::*, SecondsFormat};
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize, Serializer};
use tracing::{debug, instrument};

use crate::errors::AppError;
use crate::utils::db_utils::{get_datetime_from_row, get_required_datetime_from_row};

use super::get_db_path;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum AttachmentType {
    Image = 1,
    Text = 2,
    PDF = 3,
    Word = 4,
    PowerPoint = 5,
    Excel = 6,
}

impl TryFrom<i64> for AttachmentType {
    type Error = rusqlite::Error;

    fn try_from(value: i64) -> std::result::Result<Self, Self::Error> {
        match value {
            1 => Ok(AttachmentType::Image),
            2 => Ok(AttachmentType::Text),
            3 => Ok(AttachmentType::PDF),
            4 => Ok(AttachmentType::Word),
            5 => Ok(AttachmentType::PowerPoint),
            6 => Ok(AttachmentType::Excel),
            _ => Err(rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Integer,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid attachment type: {}", value),
                )),
            )),
        }
    }
}

fn serialize_datetime_millis<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&dt.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn serialize_option_datetime_millis<S>(
    dt: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match dt {
        Some(value) => {
            serializer.serialize_str(&value.to_rfc3339_opts(SecondsFormat::Millis, true))
        }
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Conversation {
    pub id: i64,
    pub name: String,
    pub assistant_id: Option<i64>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub conversation_id: i64,
    pub message_type: String,
    pub content: String,
    pub llm_model_id: Option<i64>,
    pub llm_model_name: Option<String>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub finish_time: Option<DateTime<Utc>>,
    pub token_count: i32,
    pub input_token_count: i32,
    pub output_token_count: i32,
    pub generation_group_id: Option<String>,
    pub parent_group_id: Option<String>,
    pub tool_calls_json: Option<String>, // 保存原始 tool_calls JSON
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub first_token_time: Option<DateTime<Utc>>, // 首个 token 到达时间
    pub ttft_ms: Option<i64>,            // Time to First Token (毫秒)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageDetail {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub conversation_id: i64,
    pub message_type: String,
    pub content: String,
    pub llm_model_id: Option<i64>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub finish_time: Option<DateTime<Utc>>,
    pub token_count: i32,
    pub input_token_count: i32,
    pub output_token_count: i32,
    pub generation_group_id: Option<String>,
    pub parent_group_id: Option<String>,
    pub tool_calls_json: Option<String>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub first_token_time: Option<DateTime<Utc>>, // 首个 token 到达时间
    pub ttft_ms: Option<i64>, // Time to First Token (毫秒)
    pub attachment_list: Vec<MessageAttachment>,
    pub regenerate: Vec<MessageDetail>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageAttachment {
    pub id: i64,
    pub message_id: i64,
    pub attachment_type: AttachmentType,
    pub attachment_url: Option<String>,
    pub attachment_content: Option<String>,
    pub attachment_hash: Option<String>,
    pub use_vector: bool,
    pub token_count: Option<i32>,
}

pub trait Repository<T> {
    fn create(&self, item: &T) -> Result<T>;
    fn read(&self, id: i64) -> Result<Option<T>>;
    fn update(&self, item: &T) -> Result<()>;
    fn delete(&self, id: i64) -> Result<()>;
}

pub struct ConversationRepository {
    conn: Connection,
}

impl ConversationRepository {
    #[instrument(level = "debug", skip(conn))]
    pub fn new(conn: Connection) -> Self {
        ConversationRepository { conn }
    }

    #[instrument(level = "debug", skip(self), fields(page = page, per_page = per_page))]
    pub fn list(&self, page: u32, per_page: u32) -> Result<Vec<Conversation>> {
        let offset = (page - 1) * per_page;
        let mut stmt = self.conn.prepare(
            "SELECT id, name, assistant_id, created_time
             FROM conversation
             ORDER BY created_time DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(&[&per_page, &offset], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                name: row.get(1)?,
                assistant_id: row.get(2)?,
                created_time: get_required_datetime_from_row(row, 3, "created_time")?,
            })
        })?;
        rows.collect()
    }

    pub fn update_assistant_id(
        &self,
        origin_assistant_id: i64,
        assistant_id: Option<i64>,
    ) -> Result<()> {
        debug!(origin_assistant_id, new_assistant_id = ?assistant_id, "update_assistant_id");
        self.conn.execute(
            "UPDATE conversation SET assistant_id = ?1 WHERE assistant_id = ?2",
            (&assistant_id, &origin_assistant_id),
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id = conversation.id, name = conversation.name))]
    pub fn update_name(&self, conversation: &Conversation) -> Result<()> {
        self.conn.execute(
            "UPDATE conversation SET name = ?1 WHERE id = ?2",
            (&conversation.name, &conversation.id),
        )?;
        Ok(())
    }
}

impl Repository<Conversation> for ConversationRepository {
    #[instrument(level = "debug", skip(self, conversation), fields(name = conversation.name))]
    fn create(&self, conversation: &Conversation) -> Result<Conversation> {
        self.conn.execute(
            "INSERT INTO conversation (name, assistant_id, created_time) VALUES (?1, ?2, ?3)",
            (&conversation.name, &conversation.assistant_id, &conversation.created_time),
        )?;
        let id = self.conn.last_insert_rowid();
        debug!(conversation_id = id, "conversation inserted");
        Ok(Conversation {
            id,
            name: conversation.name.clone(),
            assistant_id: conversation.assistant_id,
            created_time: conversation.created_time,
        })
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn read(&self, id: i64) -> Result<Option<Conversation>> {
        self.conn
            .query_row(
                "SELECT id, name, assistant_id, created_time FROM conversation WHERE id = ?",
                &[&id],
                |row| {
                    Ok(Conversation {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        assistant_id: row.get(2)?,
                        created_time: get_required_datetime_from_row(row, 3, "created_time")?,
                    })
                },
            )
            .optional()
    }

    #[instrument(level = "debug", skip(self, conversation), fields(id = conversation.id))]
    fn update(&self, conversation: &Conversation) -> Result<()> {
        self.conn.execute(
            "UPDATE conversation SET name = ?1, assistant_id = ?2 WHERE id = ?3",
            (&conversation.name, &conversation.assistant_id, &conversation.id),
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn delete(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM conversation WHERE id = ?", &[&id])?;
        Ok(())
    }
}

pub struct MessageRepository {
    conn: Connection,
}

impl MessageRepository {
    #[instrument(level = "debug", skip(conn))]
    pub fn new(conn: Connection) -> Self {
        MessageRepository { conn }
    }

    #[instrument(level = "debug", skip(self), fields(conversation_id = conversation_id))]
    pub fn list_by_conversation_id(
        &self,
        conversation_id: i64,
    ) -> Result<Vec<(Message, Option<MessageAttachment>)>> {
        let mut stmt = self.conn.prepare("SELECT message.id, message.parent_id, message.conversation_id, message.message_type, message.content, message.llm_model_id, message.llm_model_name, message.created_time, message.start_time, message.finish_time, message.token_count, message.input_token_count, message.output_token_count, message.generation_group_id, message.parent_group_id, message.tool_calls_json, message.first_token_time, message.ttft_ms, ma.attachment_type, ma.attachment_url, ma.attachment_content, ma.use_vector as attachment_use_vector, ma.token_count as attachment_token_count
                                          FROM message
                                          LEFT JOIN message_attachment ma ON message.id = ma.message_id
                                          WHERE message.conversation_id = ?1
                                          ORDER BY message.created_time ASC")?;
        let rows = stmt.query_map(&[&conversation_id], |row| {
            let attachment_type_int: Option<i64> = row.get(18).ok();
            let attachment_type = attachment_type_int.map(AttachmentType::try_from).transpose()?;
            let message = Message {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                conversation_id: row.get(2)?,
                message_type: row.get(3)?,
                content: row.get(4)?,
                llm_model_id: row.get(5)?,
                llm_model_name: row.get(6)?,
                created_time: get_required_datetime_from_row(row, 7, "created_time")?,
                start_time: get_datetime_from_row(row, 8)?,
                finish_time: get_datetime_from_row(row, 9)?,
                token_count: row.get(10)?,
                input_token_count: row.get(11)?,
                output_token_count: row.get(12)?,
                generation_group_id: row.get(13)?,
                parent_group_id: row.get(14)?,
                tool_calls_json: row.get(15)?,
                first_token_time: get_datetime_from_row(row, 16)?,
                ttft_ms: row.get(17).ok(),
            };
            let attachment = if attachment_type.is_some() {
                Some(MessageAttachment {
                    id: 0,
                    message_id: row.get(0)?,
                    attachment_type: attachment_type.unwrap(),
                    attachment_url: row.get(19)?,
                    attachment_content: row.get(20)?,
                    attachment_hash: None,
                    use_vector: row.get(21)?,
                    token_count: row.get(22)?,
                })
            } else {
                None
            };
            Ok((message, attachment))
        })?;
        rows.collect()
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    pub fn update_finish_time(&self, id: i64) -> Result<()> {
        // Avoid SQLite CURRENT_TIMESTAMP (second precision) which can be earlier than millisecond
        // timestamps (e.g., first_token_time) and breaks duration-based TPS calculations.
        let now = chrono::Utc::now();
        self.conn.execute(
            "UPDATE message SET finish_time = ?1 WHERE id = ?2",
            rusqlite::params![now, id],
        )?;
        Ok(())
    }

    /// 更新消息内容
    #[instrument(level = "debug", skip(self, content), fields(id = id, content_len = content.len()))]
    pub fn update_content(&self, id: i64, content: &str) -> Result<()> {
        self.conn.execute("UPDATE message SET content = ?1 WHERE id = ?2", (content, id))?;
        Ok(())
    }
}

impl Repository<Message> for MessageRepository {
    #[instrument(level = "debug", skip(self, message), fields(conversation_id = message.conversation_id, message_type = message.message_type))]
    fn create(&self, message: &Message) -> Result<Message> {
        // rusqlite Params trait only supports up to 16 parameters, use named params for 17+ fields
        self.conn.execute(
            "INSERT INTO message (parent_id, conversation_id, message_type, content, llm_model_id, llm_model_name, created_time, start_time, finish_time, token_count, input_token_count, output_token_count, generation_group_id, parent_group_id, tool_calls_json, first_token_time, ttft_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            rusqlite::params![
                &message.parent_id,
                &message.conversation_id,
                &message.message_type,
                &message.content,
                &message.llm_model_id,
                &message.llm_model_name,
                &message.created_time,
                &message.start_time,
                &message.finish_time,
                &message.token_count,
                &message.input_token_count,
                &message.output_token_count,
                &message.generation_group_id,
                &message.parent_group_id,
                &message.tool_calls_json,
                &message.first_token_time,
                &message.ttft_ms,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Message {
            id,
            parent_id: message.parent_id,
            conversation_id: message.conversation_id,
            message_type: message.message_type.clone(),
            content: message.content.clone(),
            llm_model_id: message.llm_model_id,
            llm_model_name: message.llm_model_name.clone(),
            created_time: message.created_time,
            start_time: message.start_time,
            finish_time: message.finish_time,
            token_count: message.token_count,
            input_token_count: message.input_token_count,
            output_token_count: message.output_token_count,
            generation_group_id: message.generation_group_id.clone(),
            parent_group_id: message.parent_group_id.clone(),
            tool_calls_json: message.tool_calls_json.clone(),
            first_token_time: message.first_token_time,
            ttft_ms: message.ttft_ms,
        })
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn read(&self, id: i64) -> Result<Option<Message>> {
        self.conn
            .query_row("SELECT id, parent_id, conversation_id, message_type, content, llm_model_id, llm_model_name, created_time, start_time, finish_time, token_count, input_token_count, output_token_count, generation_group_id, parent_group_id, tool_calls_json, first_token_time, ttft_ms FROM message WHERE id = ?", &[&id], |row| {
                Ok(Message {
                    id: row.get(0)?,
                    parent_id: row.get(1)?,
                    conversation_id: row.get(2)?,
                    message_type: row.get(3)?,
                    content: row.get(4)?,
                    llm_model_id: row.get(5)?,
                    llm_model_name: row.get(6)?,
                    created_time: get_required_datetime_from_row(row, 7, "created_time")?,
                    start_time: get_datetime_from_row(row, 8)?,
                    finish_time: get_datetime_from_row(row, 9)?,
                    token_count: row.get(10)?,
                    input_token_count: row.get(11)?,
                    output_token_count: row.get(12)?,
                    generation_group_id: row.get(13)?,
                    parent_group_id: row.get(14)?,
                    tool_calls_json: row.get(15)?,
                    first_token_time: get_datetime_from_row(row, 16)?,
                    ttft_ms: row.get(17).ok(),
                })
            })
            .optional()
    }

    #[instrument(level = "debug", skip(self, message), fields(id = message.id))]
    fn update(&self, message: &Message) -> Result<()> {
        self.conn.execute(
            "UPDATE message SET conversation_id = ?1, message_type = ?2, content = ?3, llm_model_id = ?4, llm_model_name = ?5, token_count = ?6, input_token_count = ?7, output_token_count = ?8, tool_calls_json = ?9, first_token_time = ?10, ttft_ms = ?11, start_time = ?12, finish_time = ?13 WHERE id = ?14",
            rusqlite::params![
                &message.conversation_id,
                &message.message_type,
                &message.content,
                &message.llm_model_id,
                &message.llm_model_name,
                &message.token_count,
                &message.input_token_count,
                &message.output_token_count,
                &message.tool_calls_json,
                &message.first_token_time,
                &message.ttft_ms,
                &message.start_time,
                &message.finish_time,
                &message.id,
            ],
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn delete(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM message WHERE id = ?", &[&id])?;
        Ok(())
    }
}

pub struct MessageAttachmentRepository {
    conn: Connection,
}

impl MessageAttachmentRepository {
    #[instrument(level = "debug", skip(conn))]
    pub fn new(conn: Connection) -> Self {
        MessageAttachmentRepository { conn }
    }

    #[instrument(level = "debug", skip(self, id_list), fields(id_count = id_list.len()))]
    pub fn list_by_id(&self, id_list: &Vec<i64>) -> Result<Vec<MessageAttachment>> {
        let id_list_str: Vec<String> = id_list.iter().map(|id| id.to_string()).collect();
        let id_list_str = id_list_str.join(",");
        let query = format!("SELECT id, message_id, attachment_type, attachment_url, attachment_content, attachment_hash, use_vector, token_count FROM message_attachment WHERE id IN ({})", id_list_str);
        let mut stmt = self.conn.prepare(&query)?;
        let rows = stmt.query_map([], |row| {
            let attachment_type_int: i64 = row.get(2)?;
            let attachment_type = AttachmentType::try_from(attachment_type_int)?;
            Ok(MessageAttachment {
                id: row.get(0)?,
                message_id: row.get(1)?,
                attachment_type,
                attachment_url: row.get(3)?,
                attachment_content: row.get(4)?,
                attachment_hash: row.get(5)?,
                use_vector: row.get(6)?,
                token_count: row.get(7)?,
            })
        })?;
        rows.collect()
    }

    pub fn read_by_attachment_hash(
        &self,
        attachment_hash: &str,
    ) -> Result<Option<MessageAttachment>> {
        self.conn
            .query_row("SELECT id, message_id, attachment_type, attachment_url, attachment_content, attachment_hash, use_vector, token_count FROM message_attachment WHERE attachment_hash = ?", &[&attachment_hash], |row| {
                let attachment_type_int: i64 = row.get(2)?;
                let attachment_type = AttachmentType::try_from(attachment_type_int)?;
                Ok(MessageAttachment {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    attachment_type,
                    attachment_url: row.get(3)?,
                    attachment_content: row.get(4)?,
                    attachment_hash: row.get(5)?,
                    use_vector: row.get(6)?,
                    token_count: row.get(7)?,
                })
            })
            .optional()
    }
}

impl Repository<MessageAttachment> for MessageAttachmentRepository {
    #[instrument(level = "debug", skip(self, attachment), fields(message_id = attachment.message_id, attachment_type = ?(attachment.attachment_type as i64)))]
    fn create(&self, attachment: &MessageAttachment) -> Result<MessageAttachment> {
        self.conn.execute(
            "INSERT INTO message_attachment (message_id, attachment_type, attachment_url, attachment_content, attachment_hash, use_vector, token_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (&attachment.message_id, &(attachment.attachment_type as i64), &attachment.attachment_url, &attachment.attachment_content, &attachment.attachment_hash, &attachment.use_vector, &attachment.token_count),
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(MessageAttachment {
            id,
            message_id: attachment.message_id,
            attachment_type: attachment.attachment_type,
            attachment_url: attachment.attachment_url.clone(),
            attachment_content: attachment.attachment_content.clone(),
            attachment_hash: None,
            use_vector: attachment.use_vector,
            token_count: attachment.token_count,
        })
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn read(&self, id: i64) -> Result<Option<MessageAttachment>> {
        self.conn
            .query_row("SELECT id, message_id, attachment_type, attachment_url, attachment_content, attachment_hash, use_vector, token_count FROM message_attachment WHERE id = ?", &[&id], |row| {
                let attachment_type_int: i64 = row.get(2)?;
                let attachment_type = AttachmentType::try_from(attachment_type_int)?;
                Ok(MessageAttachment {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    attachment_type,
                    attachment_url: row.get(3)?,
                    attachment_content: row.get(4)?,
                    attachment_hash: row.get(5)?,
                    use_vector: row.get(6)?,
                    token_count: row.get(7)?,
                })
            })
            .optional()
    }

    #[instrument(level = "debug", skip(self, attachment), fields(id = attachment.id))]
    fn update(&self, attachment: &MessageAttachment) -> Result<()> {
        self.conn.execute(
            "UPDATE message_attachment SET message_id = ?1 WHERE id = ?2",
            (&attachment.message_id, &attachment.id),
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn delete(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM message_attachment WHERE id = ?", &[&id])?;
        Ok(())
    }
}

pub struct ConversationDatabase {
    db_path: PathBuf,
}

impl ConversationDatabase {
    pub fn new(app_handle: &tauri::AppHandle) -> rusqlite::Result<Self> {
        let db_path = get_db_path(app_handle, "conversation.db");

        Ok(ConversationDatabase { db_path: db_path.unwrap() })
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_connection(&self) -> rusqlite::Result<Connection> {
        let conn = Connection::open(&self.db_path)?;
        // 性能优化：为所有连接设置更合适的 PRAGMA
        // - WAL 能改善读写并发性能
        // - synchronous=NORMAL 在保证安全的同时提升速度
        // - busy_timeout 防止短暂锁竞争导致的失败
        // - temp_store=MEMORY、适度增大 cache_size 提升查询性能
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;\nPRAGMA synchronous=NORMAL;\nPRAGMA foreign_keys=ON;\nPRAGMA busy_timeout=5000;\nPRAGMA temp_store=MEMORY;\nPRAGMA cache_size=-20000;",
        )?;
        Ok(conn)
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn conversation_repo(&self) -> Result<ConversationRepository, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        Ok(ConversationRepository::new(conn))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn message_repo(&self) -> Result<MessageRepository, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        Ok(MessageRepository::new(conn))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn attachment_repo(&self) -> Result<MessageAttachmentRepository, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        Ok(MessageAttachmentRepository::new(conn))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn conversation_summary_repo(&self) -> Result<ConversationSummaryRepository, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        Ok(ConversationSummaryRepository::new(conn))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn create_tables(&self) -> rusqlite::Result<()> {
        let conn = self.get_connection().unwrap();

        conn.execute(
            "CREATE TABLE IF NOT EXISTS conversation (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                assistant_id INTEGER,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS message (
                id              INTEGER
                primary key autoincrement,
                conversation_id INTEGER not null,
                message_type    TEXT    not null,
                content         TEXT    not null,
                llm_model_id    INTEGER,
                created_time    DATETIME default CURRENT_TIMESTAMP,
                token_count     INTEGER,
                input_token_count INTEGER DEFAULT 0,
                output_token_count INTEGER DEFAULT 0,
                parent_id       integer,
                start_time      DATETIME,
                finish_time     DATETIME,
                llm_model_name  TEXT,
                generation_group_id TEXT,
                parent_group_id TEXT,
                tool_calls_json TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS acp_session (
                conversation_id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                updated_time DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        // 添加迁移逻辑：如果parent_group_id、tool_calls_json、input_token_count或output_token_count列不存在，则添加它们
        let mut stmt = conn.prepare("PRAGMA table_info(message)")?;
        let column_info: Vec<String> = stmt
            .query_map([], |row| {
                let column_name: String = row.get(1)?;
                Ok(column_name)
            })?
            .collect::<Result<Vec<String>, _>>()?;

        if !column_info.contains(&"parent_group_id".to_string()) {
            conn.execute("ALTER TABLE message ADD COLUMN parent_group_id TEXT", [])?;
        }
        if !column_info.contains(&"tool_calls_json".to_string()) {
            conn.execute("ALTER TABLE message ADD COLUMN tool_calls_json TEXT", [])?;
        }
        if !column_info.contains(&"input_token_count".to_string()) {
            conn.execute("ALTER TABLE message ADD COLUMN input_token_count INTEGER DEFAULT 0", [])?;
        }
        if !column_info.contains(&"output_token_count".to_string()) {
            conn.execute(
                "ALTER TABLE message ADD COLUMN output_token_count INTEGER DEFAULT 0",
                [],
            )?;
        }
        // 添加性能指标相关列
        if !column_info.contains(&"first_token_time".to_string()) {
            conn.execute("ALTER TABLE message ADD COLUMN first_token_time DATETIME", [])?;
        }
        if !column_info.contains(&"ttft_ms".to_string()) {
            conn.execute("ALTER TABLE message ADD COLUMN ttft_ms INTEGER", [])?;
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS message_attachment (
                id                 INTEGER
                primary key autoincrement,
                message_id         INTEGER,
                attachment_type    INTEGER           not null,
                attachment_url     TEXT,
                attachment_hash    TEXT,
                attachment_content TEXT,
                use_vector         BOOLEAN default 0 not null,
                token_count        INTEGER
            )",
            [],
        )?;

        // 关键索引：显著提升查询性能
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_message_conversation_id ON message(conversation_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_message_conversation_created ON message(conversation_id, created_time)",
            [],
        )?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_message_parent_id ON message(parent_id)", [])?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_message_attachment_message_id ON message_attachment(message_id)",
            [],
        )?;

        // 创建对话总结表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS conversation_summary (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL,
                summary TEXT NOT NULL,
                user_intent TEXT,
                key_outcomes TEXT,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversation_summary_conversation_id ON conversation_summary(conversation_id)",
            [],
        )?;

        Ok(())
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn get_acp_session_id(&self, conversation_id: i64) -> Result<Option<String>, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        let session_id = conn
            .query_row(
                "SELECT session_id FROM acp_session WHERE conversation_id = ?1",
                params![conversation_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::from)?;
        Ok(session_id)
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn upsert_acp_session_id(
        &self,
        conversation_id: i64,
        session_id: &str,
    ) -> Result<(), AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        conn.execute(
            "INSERT INTO acp_session (conversation_id, session_id, updated_time)
             VALUES (?1, ?2, CURRENT_TIMESTAMP)
             ON CONFLICT(conversation_id)
             DO UPDATE SET session_id = excluded.session_id, updated_time = CURRENT_TIMESTAMP",
            params![conversation_id, session_id],
        )
        .map_err(AppError::from)?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn delete_acp_session_id(&self, conversation_id: i64) -> Result<(), AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        conn.execute(
            "DELETE FROM acp_session WHERE conversation_id = ?1",
            params![conversation_id],
        )
        .map_err(AppError::from)?;
        Ok(())
    }

    /// 获取对话的token统计信息
    pub fn get_conversation_token_stats(
        &self,
        conversation_id: i64,
    ) -> rusqlite::Result<ConversationTokenStats> {
        let conn = Connection::open(&self.db_path)?;

        // 获取总token统计和按类型统计的消息数量
        let (
            total_tokens,
            input_tokens,
            output_tokens,
            message_count,
            system_count,
            user_count,
            response_count,
            reasoning_count,
            tool_result_count,
        ): (i64, i64, i64, i64, i64, i64, i64, i64, i64) = conn.query_row(
            "SELECT
                COALESCE(SUM(token_count), 0) as total,
                COALESCE(SUM(input_token_count), 0) as input,
                COALESCE(SUM(output_token_count), 0) as output,
                COUNT(*) as msg_count,
                COUNT(CASE WHEN message_type = 'system' THEN 1 END) as system_count,
                COUNT(CASE WHEN message_type = 'user' THEN 1 END) as user_count,
                COUNT(CASE WHEN message_type = 'response' THEN 1 END) as response_count,
                COUNT(CASE WHEN message_type = 'reasoning' THEN 1 END) as reasoning_count,
                COUNT(CASE WHEN message_type = 'tool_result' THEN 1 END) as tool_result_count
            FROM message
            WHERE conversation_id = ?1",
            &[&conversation_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )?;

        // 按模型分组统计
        let mut stmt = conn.prepare(
            "SELECT
                llm_model_id,
                COALESCE(llm_model_name, 'Unknown') as llm_model_name,
                SUM(token_count) as total,
                SUM(input_token_count) as input,
                SUM(output_token_count) as output,
                COUNT(*) as msg_count,
                AVG(
                    CASE
                        WHEN ttft_ms IS NOT NULL THEN ttft_ms
                        WHEN start_time IS NOT NULL AND first_token_time IS NOT NULL THEN
                            MAX((julianday(first_token_time) - julianday(start_time)) * 86400000, 0)
                        ELSE NULL
                    END
                ) as avg_ttft,
                AVG(CASE
                    WHEN output_token_count > 0
                        AND finish_time IS NOT NULL
                        AND COALESCE(first_token_time, start_time) IS NOT NULL
                        AND ((julianday(finish_time) - julianday(COALESCE(first_token_time, start_time))) * 86400000) > 0
                    THEN
                        (output_token_count * 1000.0) / CAST(
                            (julianday(finish_time) - julianday(COALESCE(first_token_time, start_time))) * 86400000 AS REAL
                        )
                    ELSE NULL
                END) as avg_tps
            FROM message
            WHERE conversation_id = ?1 AND llm_model_id IS NOT NULL AND message_type IN ('response', 'reasoning')
            GROUP BY llm_model_id
            ORDER BY total DESC",
        )?;

        let mut by_model = stmt
            .query_map(&[&conversation_id], |row| {
                Ok(ModelTokenBreakdown {
                    model_id: row.get(0)?,
                    model_name: row.get(1).unwrap_or_else(|_| "Unknown".to_string()),
                    total_tokens: row.get(2)?,
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    message_count: row.get(5)?,
                    avg_ttft_ms: row.get(6).ok(),
                    avg_tps: row.get(7).ok(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // åŸºäºŽ response å’Œ reasoning æ¶ˆæ¯çš„æ€» token åŠæ—¶é—´é•¿åº¦è®¡ç®— TPS
        let mut perf_stmt = conn.prepare(
            "SELECT
                llm_model_id,
                token_count,
                input_token_count,
                output_token_count,
                created_time,
                start_time,
                first_token_time,
                finish_time,
                ttft_ms
            FROM message
            WHERE conversation_id = ?1 AND message_type IN ('response', 'reasoning')",
        )?;

        let mut total_tokens_for_speed: i64 = 0;
        let mut total_duration_ms_for_speed: i64 = 0;
        let mut model_speed_map: HashMap<Option<i64>, (i64, i64)> = HashMap::new();

        let perf_rows = perf_stmt.query_map(&[&conversation_id], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                get_required_datetime_from_row(row, 4, "created_time")?,
                get_datetime_from_row(row, 5)?,
                get_datetime_from_row(row, 6)?,
                get_datetime_from_row(row, 7)?,
                row.get::<_, Option<i64>>(8).ok().flatten(),
            ))
        })?;

        for row in perf_rows {
            let (
                model_id,
                token_count,
                input_token_count,
                output_token_count,
                created_time,
                start_time,
                first_token_time,
                finish_time,
                ttft_ms,
            ) = row?;

            let tokens_for_speed = if output_token_count > 0 {
                output_token_count
            } else if token_count > 0 {
                token_count
            } else if input_token_count + output_token_count > 0 {
                input_token_count + output_token_count
            } else {
                0
            };

            if tokens_for_speed <= 0 {
                continue;
            }

            let mut end_point = finish_time.unwrap_or_else(chrono::Utc::now);
            // Backward-compat: older code stored finish_time via SQLite CURRENT_TIMESTAMP (second precision).
            // If finish_time has 0ms but start timestamps have ms within the same second, bump end to end-of-second
            // to avoid negative/zero durations which lead to N/A or extreme TPS.
            if finish_time.is_some()
                && end_point.timestamp_subsec_millis() == 0
                && [first_token_time, start_time].into_iter().flatten().any(|t| {
                    t.timestamp() == end_point.timestamp() && t.timestamp_subsec_millis() > 0
                })
            {
                end_point = end_point + chrono::Duration::milliseconds(999);
            }
            let start_point = {
                let candidates = [first_token_time, start_time, Some(created_time)];
                let mut selected: Option<DateTime<Utc>> = None;
                for candidate in candidates {
                    if let Some(candidate_dt) = candidate {
                        if end_point.timestamp_millis() > candidate_dt.timestamp_millis() {
                            selected = Some(candidate_dt);
                            break;
                        }
                    }
                }
                selected.unwrap_or_else(|| end_point - chrono::Duration::milliseconds(1))
            };
            let mut duration_ms =
                (end_point.timestamp_millis() - start_point.timestamp_millis()).max(1);
            // Backward-compat: older non-stream code stored start_time/first_token_time too late (near finish),
            // but did store the total request duration in ttft_ms. Prefer that when it's clearly larger.
            if let Some(ttft) = ttft_ms {
                if ttft > 0 && ttft > duration_ms {
                    duration_ms = ttft.max(1);
                }
            }

            total_tokens_for_speed += tokens_for_speed;
            total_duration_ms_for_speed += duration_ms;

            model_speed_map
                .entry(model_id)
                .and_modify(|(t, d)| {
                    *t += tokens_for_speed;
                    *d += duration_ms;
                })
                .or_insert((tokens_for_speed, duration_ms));
        }

        for model_entry in by_model.iter_mut() {
            if let Some((tokens, duration_ms)) = model_speed_map.get(&model_entry.model_id) {
                if *tokens > 0 && *duration_ms > 0 {
                    model_entry.avg_tps = Some((*tokens as f64 * 1000.0) / (*duration_ms as f64));
                }
            }
        }

        // 计算平均 TTFT 和 TPS (仅针对 response 消息)
        let (avg_ttft, avg_tps): (Option<f64>, Option<f64>) = conn.query_row(
            "SELECT
                AVG(
                    CASE
                        WHEN ttft_ms IS NOT NULL THEN ttft_ms
                        WHEN start_time IS NOT NULL AND first_token_time IS NOT NULL THEN
                            MAX((julianday(first_token_time) - julianday(start_time)) * 86400000, 0)
                        ELSE NULL
                    END
                ) as avg_ttft,
                AVG(CASE
                    WHEN output_token_count > 0
                        AND finish_time IS NOT NULL
                        AND COALESCE(first_token_time, start_time) IS NOT NULL
                        AND ((julianday(finish_time) - julianday(COALESCE(first_token_time, start_time))) * 86400000) > 0
                    THEN
                        (output_token_count * 1000.0) / CAST(
                            (julianday(finish_time) - julianday(COALESCE(first_token_time, start_time))) * 86400000 AS REAL
                        )
                    ELSE NULL
                END) as avg_tps
            FROM message
            WHERE conversation_id = ?1 AND message_type IN ('response', 'reasoning')",
            &[&conversation_id],
            |row| {
                Ok((row.get(0)?, row.get(1)?))
            },
        )?;

        let aggregated_avg_tps = if total_tokens_for_speed > 0 && total_duration_ms_for_speed > 0 {
            Some((total_tokens_for_speed as f64 * 1000.0) / (total_duration_ms_for_speed as f64))
        } else {
            // Ensure frontend never needs to show N/A for TPS.
            Some(0.0)
        };

        Ok(ConversationTokenStats {
            total_tokens: total_tokens as i32,
            input_tokens: input_tokens as i32,
            output_tokens: output_tokens as i32,
            by_model,
            message_count: message_count as i32,
            system_message_count: system_count as i32,
            user_message_count: user_count as i32,
            response_message_count: response_count as i32,
            reasoning_message_count: reasoning_count as i32,
            tool_result_message_count: tool_result_count as i32,
            avg_ttft_ms: avg_ttft,
            avg_tps: aggregated_avg_tps.or(avg_tps),
        })
    }

    /// 获取单个消息的token统计信息
    pub fn get_message_token_stats(&self, message_id: i64) -> rusqlite::Result<MessageTokenStats> {
        let conn = Connection::open(&self.db_path)?;

        conn.query_row(
            "SELECT
                id,
                token_count,
                input_token_count,
                output_token_count,
                llm_model_name,
                ttft_ms,
                first_token_time,
                finish_time,
                start_time,
                created_time
            FROM message
            WHERE id = ?1",
            &[&message_id],
            |row| {
                let total_tokens: i32 = row.get(1)?;
                let input_tokens: i32 = row.get(2)?;
                let output_tokens: i32 = row.get(3)?;
                let first_token_time = get_datetime_from_row(row, 6)?;
                let finish_time = get_datetime_from_row(row, 7)?;
                let start_time = get_datetime_from_row(row, 8)?;
                let created_time = get_required_datetime_from_row(row, 9, "created_time")?;
                let ttft_ms: Option<i64> =
                    row.get(5).ok().or_else(|| match (start_time, first_token_time) {
                        (Some(start), Some(first_token)) => {
                            Some((first_token.timestamp_millis() - start.timestamp_millis()).max(0))
                        }
                        _ => None,
                    });

                // 计算 TPS (Tokens Per Second)，优先使用输出 token，缺失时回退到总 token
                let tokens_for_speed: i64 = if output_tokens > 0 {
                    output_tokens as i64
                } else if total_tokens > 0 {
                    total_tokens as i64
                } else if input_tokens + output_tokens > 0 {
                    (input_tokens + output_tokens) as i64
                } else {
                    0
                };

                let tps = if tokens_for_speed > 0 {
                    let mut end_point = finish_time.unwrap_or_else(chrono::Utc::now);
                    if finish_time.is_some()
                        && end_point.timestamp_subsec_millis() == 0
                        && [first_token_time, start_time].into_iter().flatten().any(|t| {
                            t.timestamp() == end_point.timestamp()
                                && t.timestamp_subsec_millis() > 0
                        })
                    {
                        end_point = end_point + chrono::Duration::milliseconds(999);
                    }
                    let start_point = {
                        let candidates = [first_token_time, start_time, Some(created_time)];
                        let mut selected: Option<DateTime<Utc>> = None;
                        for candidate in candidates {
                            if let Some(candidate_dt) = candidate {
                                if end_point.timestamp_millis() > candidate_dt.timestamp_millis() {
                                    selected = Some(candidate_dt);
                                    break;
                                }
                            }
                        }
                        selected.unwrap_or_else(|| end_point - chrono::Duration::milliseconds(1))
                    };
                    let mut duration_ms =
                        (end_point.timestamp_millis() - start_point.timestamp_millis()).max(1);
                    if let Some(ttft) = ttft_ms {
                        if ttft > 0 && ttft > duration_ms {
                            duration_ms = ttft.max(1);
                        }
                    }
                    Some((tokens_for_speed as f64) * 1000.0 / duration_ms as f64)
                } else {
                    None
                };

                Ok(MessageTokenStats {
                    message_id: row.get(0)?,
                    total_tokens,
                    input_tokens,
                    output_tokens,
                    model_name: row.get(4).ok(),
                    ttft_ms,
                    tps,
                })
            },
        )
    }
}

/// 对话token统计信息
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationTokenStats {
    pub total_tokens: i32,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub by_model: Vec<ModelTokenBreakdown>,
    pub message_count: i32,
    // 按消息类型统计
    pub system_message_count: i32,
    pub user_message_count: i32,
    pub response_message_count: i32,
    pub reasoning_message_count: i32,
    pub tool_result_message_count: i32,
    // 性能指标统计
    pub avg_ttft_ms: Option<f64>, // 平均首字延迟 (毫秒)
    pub avg_tps: Option<f64>,     // 平均生成速度
}

/// 模型token分解信息
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelTokenBreakdown {
    pub model_id: Option<i64>,
    pub model_name: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub message_count: i64,
    // 性能指标统计
    pub avg_ttft_ms: Option<f64>,
    pub avg_tps: Option<f64>,
}

/// 消息token统计信息
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageTokenStats {
    pub message_id: i64,
    pub total_tokens: i32,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub model_name: Option<String>,
    pub ttft_ms: Option<i64>, // Time to First Token (毫秒)
    pub tps: Option<f64>,     // Tokens Per Second
}

/// 对话总结结构体
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationSummary {
    pub id: i64,
    pub conversation_id: i64,
    pub summary: String,       // 对话整体总结
    pub user_intent: String,   // 用户目的
    pub key_outcomes: String,  // 关键成果
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
}

pub struct ConversationSummaryRepository {
    conn: Connection,
}

impl ConversationSummaryRepository {
    #[instrument(level = "debug", skip(conn))]
    pub fn new(conn: Connection) -> Self {
        ConversationSummaryRepository { conn }
    }

    #[instrument(level = "debug", skip(self))]
    pub fn create(&self, summary: &ConversationSummary) -> Result<ConversationSummary> {
        self.conn.execute(
            "INSERT INTO conversation_summary (conversation_id, summary, user_intent, key_outcomes, created_time) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                &summary.conversation_id,
                &summary.summary,
                &summary.user_intent,
                &summary.key_outcomes,
                &summary.created_time,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(ConversationSummary {
            id,
            conversation_id: summary.conversation_id,
            summary: summary.summary.clone(),
            user_intent: summary.user_intent.clone(),
            key_outcomes: summary.key_outcomes.clone(),
            created_time: summary.created_time,
        })
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_by_conversation_id(&self, conversation_id: i64) -> Result<Option<ConversationSummary>> {
        self.conn
            .query_row(
                "SELECT id, conversation_id, summary, user_intent, key_outcomes, created_time FROM conversation_summary WHERE conversation_id = ?",
                &[&conversation_id],
                |row| {
                    Ok(ConversationSummary {
                        id: row.get(0)?,
                        conversation_id: row.get(1)?,
                        summary: row.get(2)?,
                        user_intent: row.get(3)?,
                        key_outcomes: row.get(4)?,
                        created_time: get_required_datetime_from_row(row, 5, "created_time")?,
                    })
                },
            )
            .optional()
    }

    #[instrument(level = "debug", skip(self))]
    pub fn exists(&self, conversation_id: i64) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM conversation_summary WHERE conversation_id = ?",
            &[&conversation_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    #[instrument(level = "debug", skip(self))]
    pub fn delete_by_conversation_id(&self, conversation_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM conversation_summary WHERE conversation_id = ?",
            rusqlite::params![&conversation_id],
        )?;
        Ok(())
    }
}
