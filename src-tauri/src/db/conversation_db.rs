use std::path::PathBuf;

use chrono::{prelude::*, SecondsFormat};
use rusqlite::{Connection, OptionalExtension, Result};
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
        let mut stmt = self.conn.prepare("SELECT message.id, message.parent_id, message.conversation_id, message.message_type, message.content, message.llm_model_id, message.llm_model_name, message.created_time, message.start_time, message.finish_time, message.token_count, message.input_token_count, message.output_token_count, message.generation_group_id, message.parent_group_id, message.tool_calls_json, ma.attachment_type, ma.attachment_url, ma.attachment_content, ma.use_vector as attachment_use_vector, ma.token_count as attachment_token_count
                                          FROM message
                                          LEFT JOIN message_attachment ma ON message.id = ma.message_id
                                          WHERE message.conversation_id = ?1
                                          ORDER BY message.created_time ASC")?;
        let rows = stmt.query_map(&[&conversation_id], |row| {
            let attachment_type_int: Option<i64> = row.get(16).ok();
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
            };
            let attachment = if attachment_type.is_some() {
                Some(MessageAttachment {
                    id: 0,
                    message_id: row.get(0)?,
                    attachment_type: attachment_type.unwrap(),
                    attachment_url: row.get(17)?,
                    attachment_content: row.get(18)?,
                    attachment_hash: None,
                    use_vector: row.get(19)?,
                    token_count: row.get(20)?,
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
        self.conn
            .execute("UPDATE message SET finish_time = CURRENT_TIMESTAMP WHERE id = ?1", [&id])?;
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
        self.conn.execute(
            "INSERT INTO message (parent_id, conversation_id, message_type, content, llm_model_id, llm_model_name, created_time, start_time, finish_time, token_count, input_token_count, output_token_count, generation_group_id, parent_group_id, tool_calls_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            (
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
            ),
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
        })
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn read(&self, id: i64) -> Result<Option<Message>> {
        self.conn
            .query_row("SELECT id, parent_id, conversation_id, message_type, content, llm_model_id, llm_model_name, created_time, start_time, finish_time, token_count, input_token_count, output_token_count, generation_group_id, parent_group_id, tool_calls_json FROM message WHERE id = ?", &[&id], |row| {
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
                })
            })
            .optional()
    }

    #[instrument(level = "debug", skip(self, message), fields(id = message.id))]
    fn update(&self, message: &Message) -> Result<()> {
        self.conn.execute(
            "UPDATE message SET conversation_id = ?1, message_type = ?2, content = ?3, llm_model_id = ?4, llm_model_name = ?5, token_count = ?6, input_token_count = ?7, output_token_count = ?8, tool_calls_json = ?9 WHERE id = ?10",
            (
                &message.conversation_id,
                &message.message_type,
                &message.content,
                &message.llm_model_id,
                &message.llm_model_name,
                &message.token_count,
                &message.input_token_count,
                &message.output_token_count,
                &message.tool_calls_json,
                &message.id,
            ),
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

        Ok(())
    }

    /// 获取对话的token统计信息
    pub fn get_conversation_token_stats(
        &self,
        conversation_id: i64,
    ) -> rusqlite::Result<ConversationTokenStats> {
        let conn = Connection::open(&self.db_path)?;

        // 获取总token统计
        let (total_tokens, input_tokens, output_tokens, message_count): (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT
                    COALESCE(SUM(token_count), 0) as total,
                    COALESCE(SUM(input_token_count), 0) as input,
                    COALESCE(SUM(output_token_count), 0) as output,
                    COUNT(*) as msg_count
                FROM message
                WHERE conversation_id = ?1",
                &[&conversation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;

        // 按模型分组统计
        let mut stmt = conn.prepare(
            "SELECT
                llm_model_id,
                COALESCE(llm_model_name, 'Unknown') as llm_model_name,
                SUM(token_count) as total,
                SUM(input_token_count) as input,
                SUM(output_token_count) as output,
                COUNT(*) as msg_count
            FROM message
            WHERE conversation_id = ?1 AND llm_model_id IS NOT NULL AND message_type = 'response'
            GROUP BY llm_model_id
            ORDER BY total DESC",
        )?;

        let by_model = stmt
            .query_map(&[&conversation_id], |row| {
                Ok(ModelTokenBreakdown {
                    model_id: row.get(0)?,
                    model_name: row.get(1).unwrap_or_else(|_| "Unknown".to_string()),
                    total_tokens: row.get(2)?,
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    message_count: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(ConversationTokenStats {
            total_tokens: total_tokens as i32,
            input_tokens: input_tokens as i32,
            output_tokens: output_tokens as i32,
            by_model,
            message_count: message_count as i32,
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
                llm_model_name
            FROM message
            WHERE id = ?1",
            &[&message_id],
            |row| {
                Ok(MessageTokenStats {
                    message_id: row.get(0)?,
                    total_tokens: row.get(1)?,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                    model_name: row.get(4).ok(),
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
}

/// 消息token统计信息
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageTokenStats {
    pub message_id: i64,
    pub total_tokens: i32,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub model_name: Option<String>,
}
