use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{prelude::*, SecondsFormat};
use rusqlite::{params, types::Type, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value as JsonValue;
use tracing::{debug, instrument};

use crate::errors::AppError;
use crate::utils::db_utils::{get_datetime_from_row, get_required_datetime_from_row};

use super::{get_db_path, get_db_write_lock};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum AttachmentType {
    Image = 1,
    Text = 2,
    PDF = 3,
    Word = 4,
    PowerPoint = 5,
    Excel = 6,
    Skill = 7,
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
            7 => Ok(AttachmentType::Skill),
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

fn serialize_string_array(value: &[String]) -> Result<String> {
    serde_json::to_string(value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn deserialize_string_array(index: usize, raw: String, column_name: &str) -> Result<Vec<String>> {
    serde_json::from_str::<Vec<String>>(&raw).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid {column_name} JSON: {error}"),
            )),
        )
    })
}

fn ensure_column_exists(
    conn: &Connection,
    table_name: &str,
    column_name: &str,
    column_definition: &str,
) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table_name})"))?;
    let existing_columns =
        stmt.query_map([], |row| row.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if existing_columns.iter().any(|existing| existing == column_name) {
        return Ok(());
    }

    conn.execute(
        &format!("ALTER TABLE {table_name} ADD COLUMN {column_name} {column_definition}"),
        [],
    )?;
    Ok(())
}

fn sqlite_write_lock_poisoned_error(db_name: &str) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("{db_name} write lock poisoned"),
    )))
}

#[derive(Debug, Default, Clone)]
struct PersistedUsageMetadata {
    usage_source: Option<String>,
    thought_tokens: i64,
    cached_input_tokens: i64,
    cached_read_tokens: i64,
    cached_write_tokens: i64,
}

fn parse_persisted_usage_metadata(raw: Option<&str>) -> PersistedUsageMetadata {
    let Some(raw) = raw else {
        return PersistedUsageMetadata::default();
    };
    let Ok(JsonValue::Object(map)) = serde_json::from_str::<JsonValue>(raw) else {
        return PersistedUsageMetadata::default();
    };

    let cached_input_tokens = map
        .get("cached_input_tokens")
        .or_else(|| map.get("cached_read_tokens"))
        .and_then(|value| value.as_i64())
        .unwrap_or(0);

    PersistedUsageMetadata {
        usage_source: map
            .get("usage_source")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        thought_tokens: map.get("thought_tokens").and_then(|value| value.as_i64()).unwrap_or(0),
        cached_input_tokens,
        cached_read_tokens: cached_input_tokens,
        cached_write_tokens: map
            .get("cached_write_tokens")
            .or_else(|| map.get("cache_creation_tokens"))
            .and_then(|value| value.as_i64())
            .unwrap_or(0),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Conversation {
    pub id: i64,
    pub name: String,
    pub assistant_id: Option<i64>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub updated_time: DateTime<Utc>,
    pub conversation_kind: String,
    pub parent_butler_conversation_id: Option<i64>,
    pub source_task_title: Option<String>,
    pub is_hidden_from_normal_chat_list: bool,
    pub channel_source: Option<String>,
    pub butler_task_status: Option<String>,
    pub butler_task_summary: Option<String>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub butler_task_finalized_at: Option<DateTime<Utc>>,
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
    pub metadata_json: Option<String>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub first_token_time: Option<DateTime<Utc>>, // 首个 token 到达时间
    pub ttft_ms: Option<i64>,            // Time to First Token (毫秒)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LargeMessagePreviewMetadata {
    pub line_count: usize,
    pub payload_char_count: usize,
    pub content_hash: String,
    pub reason: String,
    pub should_preview: bool,
    pub summary: String,
    pub preview_text: String,
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
    pub metadata_json: Option<String>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub first_token_time: Option<DateTime<Utc>>, // 首个 token 到达时间
    pub ttft_ms: Option<i64>, // Time to First Token (毫秒)
    pub attachment_list: Vec<MessageAttachment>,
    pub regenerate: Vec<MessageDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_message_preview: Option<LargeMessagePreviewMetadata>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QueuedConversationMessage {
    pub id: i64,
    pub conversation_id: i64,
    pub queue_kind: String,
    pub status: String,
    pub request_json: String,
    pub prompt: String,
    pub assistant_id: i64,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub updated_time: DateTime<Utc>,
}

pub trait Repository<T> {
    fn create(&self, item: &T) -> Result<T>;
    fn read(&self, id: i64) -> Result<Option<T>>;
    fn update(&self, item: &T) -> Result<()>;
    fn delete(&self, id: i64) -> Result<()>;
}

pub struct ConversationRepository {
    conn: Connection,
    write_lock: Arc<Mutex<()>>,
}

impl ConversationRepository {
    #[instrument(level = "debug", skip(conn))]
    #[allow(dead_code)]
    pub fn new(conn: Connection) -> Self {
        Self::new_with_write_lock(conn, Arc::new(Mutex::new(())))
    }

    #[instrument(level = "debug", skip(conn, write_lock))]
    pub fn new_with_write_lock(conn: Connection, write_lock: Arc<Mutex<()>>) -> Self {
        ConversationRepository { conn, write_lock }
    }

    fn with_serialized_write<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| sqlite_write_lock_poisoned_error("conversation.db"))?;
        f(&self.conn)
    }

    #[instrument(level = "debug", skip(self), fields(page = page, per_page = per_page))]
    pub fn list(&self, page: u32, per_page: u32) -> Result<Vec<Conversation>> {
        let offset = (page - 1) * per_page;
        let mut stmt = self.conn.prepare(
            "SELECT id, name, assistant_id, created_time, updated_time, conversation_kind,
                    parent_butler_conversation_id, source_task_title,
                    is_hidden_from_normal_chat_list, channel_source, butler_task_status,
                    butler_task_summary, butler_task_finalized_at
              FROM conversation
             WHERE COALESCE(is_hidden_from_normal_chat_list, 0) = 0
             ORDER BY created_time DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(&[&per_page, &offset], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                name: row.get(1)?,
                assistant_id: row.get(2)?,
                created_time: get_required_datetime_from_row(row, 3, "created_time")?,
                updated_time: get_required_datetime_from_row(row, 4, "updated_time")?,
                conversation_kind: row.get(5)?,
                parent_butler_conversation_id: row.get(6)?,
                source_task_title: row.get(7)?,
                is_hidden_from_normal_chat_list: row.get(8)?,
                channel_source: row.get(9)?,
                butler_task_status: row.get(10)?,
                butler_task_summary: row.get(11)?,
                butler_task_finalized_at: get_datetime_from_row(row, 12)?,
            })
        })?;
        rows.collect()
    }

    /// List conversations with optional filters for conversation_kind and fuzzy search.
    /// When `search` is provided, matches against conversation name and message content.
    #[instrument(level = "debug", skip(self), fields(page = page, per_page = per_page, conversation_kind = ?conversation_kind, search = ?search))]
    pub fn list_with_filters(
        &self,
        page: u32,
        per_page: u32,
        conversation_kind: Option<&str>,
        search: Option<&str>,
    ) -> Result<Vec<Conversation>> {
        let offset = (page - 1) * per_page;
        let kind = conversation_kind.unwrap_or("normal");
        let search_pattern =
            search.filter(|s| !s.trim().is_empty()).map(|s| format!("%{}%", s.trim()));

        let sql = if search_pattern.is_some() {
            "SELECT DISTINCT c.id, c.name, c.assistant_id, c.created_time, c.updated_time,
                    c.conversation_kind, c.parent_butler_conversation_id, c.source_task_title,
                    c.is_hidden_from_normal_chat_list, c.channel_source, c.butler_task_status,
                    c.butler_task_summary, c.butler_task_finalized_at
               FROM conversation c
               LEFT JOIN message m ON m.conversation_id = c.id
              WHERE c.conversation_kind = ?1
                AND (c.name LIKE ?2 COLLATE NOCASE OR m.content LIKE ?2 COLLATE NOCASE)
              ORDER BY c.updated_time DESC
              LIMIT ?3 OFFSET ?4"
        } else {
            "SELECT id, name, assistant_id, created_time, updated_time, conversation_kind,
                    parent_butler_conversation_id, source_task_title,
                    is_hidden_from_normal_chat_list, channel_source, butler_task_status,
                    butler_task_summary, butler_task_finalized_at
               FROM conversation
              WHERE conversation_kind = ?1
              ORDER BY updated_time DESC
              LIMIT ?3 OFFSET ?4"
        };

        let mut stmt = self.conn.prepare(sql)?;
        let like_val = search_pattern.unwrap_or_default();
        let rows = stmt.query_map(rusqlite::params![kind, like_val, per_page, offset], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                name: row.get(1)?,
                assistant_id: row.get(2)?,
                created_time: get_required_datetime_from_row(row, 3, "created_time")?,
                updated_time: get_required_datetime_from_row(row, 4, "updated_time")?,
                conversation_kind: row.get(5)?,
                parent_butler_conversation_id: row.get(6)?,
                source_task_title: row.get(7)?,
                is_hidden_from_normal_chat_list: row.get(8)?,
                channel_source: row.get(9)?,
                butler_task_status: row.get(10)?,
                butler_task_summary: row.get(11)?,
                butler_task_finalized_at: get_datetime_from_row(row, 12)?,
            })
        })?;
        rows.collect()
    }

    #[instrument(level = "debug", skip(self), fields(parent_butler_conversation_id = parent_butler_conversation_id))]
    pub fn list_by_parent_butler_conversation_id(
        &self,
        parent_butler_conversation_id: i64,
    ) -> Result<Vec<Conversation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, assistant_id, created_time, updated_time, conversation_kind,
                    parent_butler_conversation_id, source_task_title,
                    is_hidden_from_normal_chat_list, channel_source, butler_task_status,
                    butler_task_summary, butler_task_finalized_at
             FROM conversation
             WHERE parent_butler_conversation_id = ?1
             ORDER BY updated_time DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![parent_butler_conversation_id], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                name: row.get(1)?,
                assistant_id: row.get(2)?,
                created_time: get_required_datetime_from_row(row, 3, "created_time")?,
                updated_time: get_required_datetime_from_row(row, 4, "updated_time")?,
                conversation_kind: row.get(5)?,
                parent_butler_conversation_id: row.get(6)?,
                source_task_title: row.get(7)?,
                is_hidden_from_normal_chat_list: row.get(8)?,
                channel_source: row.get(9)?,
                butler_task_status: row.get(10)?,
                butler_task_summary: row.get(11)?,
                butler_task_finalized_at: get_datetime_from_row(row, 12)?,
            })
        })?;
        rows.collect()
    }

    pub fn count_butler_task_conversations(
        &self,
        parent_butler_conversation_id: i64,
    ) -> Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(*)
             FROM conversation
             WHERE parent_butler_conversation_id = ?1
               AND conversation_kind = 'butler_task'",
            params![parent_butler_conversation_id],
            |row| row.get(0),
        )
    }

    pub fn list_butler_task_conversations_paginated(
        &self,
        parent_butler_conversation_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Conversation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, assistant_id, created_time, updated_time, conversation_kind,
                    parent_butler_conversation_id, source_task_title,
                    is_hidden_from_normal_chat_list, channel_source, butler_task_status,
                    butler_task_summary, butler_task_finalized_at
             FROM conversation
             WHERE parent_butler_conversation_id = ?1
               AND conversation_kind = 'butler_task'
             ORDER BY updated_time DESC, id DESC
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows =
            stmt.query_map(params![parent_butler_conversation_id, limit, offset], |row| {
                Ok(Conversation {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    assistant_id: row.get(2)?,
                    created_time: get_required_datetime_from_row(row, 3, "created_time")?,
                    updated_time: get_required_datetime_from_row(row, 4, "updated_time")?,
                    conversation_kind: row.get(5)?,
                    parent_butler_conversation_id: row.get(6)?,
                    source_task_title: row.get(7)?,
                    is_hidden_from_normal_chat_list: row.get(8)?,
                    channel_source: row.get(9)?,
                    butler_task_status: row.get(10)?,
                    butler_task_summary: row.get(11)?,
                    butler_task_finalized_at: get_datetime_from_row(row, 12)?,
                })
            })?;
        rows.collect()
    }

    #[instrument(level = "debug", skip(self))]
    pub fn list_reconcilable_butler_task_conversation_ids(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT conversation.id
             FROM conversation
             WHERE conversation_kind = 'butler_task'
               AND COALESCE(butler_task_status, '') IN ('running', 'cancelled')
               AND (
                    butler_task_finalized_at IS NULL
                    OR NOT EXISTS (
                        SELECT 1
                        FROM butler_task_result
                        WHERE butler_task_result.task_conversation_id = conversation.id
                    )
               )
             ORDER BY updated_time ASC, id ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect()
    }

    #[instrument(level = "debug", skip(self))]
    pub fn list_butler_task_conversation_ids_pending_followup(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT task_conversation_id
             FROM butler_task_result
             WHERE COALESCE(followup_status, 'enqueued') IN ('pending', 'handoff_injected')
             ORDER BY updated_time ASC, task_conversation_id ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect()
    }

    pub fn update_assistant_id(
        &self,
        origin_assistant_id: i64,
        assistant_id: Option<i64>,
    ) -> Result<()> {
        debug!(origin_assistant_id, new_assistant_id = ?assistant_id, "update_assistant_id");
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE conversation SET assistant_id = ?1 WHERE assistant_id = ?2",
                (&assistant_id, &origin_assistant_id),
            )?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(id = conversation.id, name = conversation.name))]
    pub fn update_name(&self, conversation: &Conversation) -> Result<()> {
        let now = chrono::Utc::now();
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE conversation SET name = ?1, updated_time = ?2 WHERE id = ?3",
                (&conversation.name, &now, &conversation.id),
            )?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(origin_parent_id = origin_parent_id, new_parent_id = new_parent_id))]
    pub fn reassign_parent_butler_conversation(
        &self,
        origin_parent_id: i64,
        new_parent_id: i64,
    ) -> Result<()> {
        let now = chrono::Utc::now();
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE conversation
                 SET parent_butler_conversation_id = ?1,
                     updated_time = ?2
                 WHERE parent_butler_conversation_id = ?3",
                params![new_parent_id, now, origin_parent_id],
            )?;
            Ok(())
        })
    }
}

impl Repository<Conversation> for ConversationRepository {
    #[instrument(level = "debug", skip(self, conversation), fields(name = conversation.name))]
    fn create(&self, conversation: &Conversation) -> Result<Conversation> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "INSERT INTO conversation (
                    name,
                    assistant_id,
                    created_time,
                    updated_time,
                    conversation_kind,
                    parent_butler_conversation_id,
                    source_task_title,
                    is_hidden_from_normal_chat_list,
                    channel_source,
                    butler_task_status,
                    butler_task_summary,
                    butler_task_finalized_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                (
                    &conversation.name,
                    &conversation.assistant_id,
                    &conversation.created_time,
                    &conversation.updated_time,
                    &conversation.conversation_kind,
                    &conversation.parent_butler_conversation_id,
                    &conversation.source_task_title,
                    &conversation.is_hidden_from_normal_chat_list,
                    &conversation.channel_source,
                    &conversation.butler_task_status,
                    &conversation.butler_task_summary,
                    &conversation.butler_task_finalized_at,
                ),
            )?;
            let id = conn.last_insert_rowid();
            debug!(conversation_id = id, "conversation inserted");
            Ok(Conversation {
                id,
                name: conversation.name.clone(),
                assistant_id: conversation.assistant_id,
                created_time: conversation.created_time,
                updated_time: conversation.updated_time,
                conversation_kind: conversation.conversation_kind.clone(),
                parent_butler_conversation_id: conversation.parent_butler_conversation_id,
                source_task_title: conversation.source_task_title.clone(),
                is_hidden_from_normal_chat_list: conversation.is_hidden_from_normal_chat_list,
                channel_source: conversation.channel_source.clone(),
                butler_task_status: conversation.butler_task_status.clone(),
                butler_task_summary: conversation.butler_task_summary.clone(),
                butler_task_finalized_at: conversation.butler_task_finalized_at,
            })
        })
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn read(&self, id: i64) -> Result<Option<Conversation>> {
        self.conn
            .query_row(
                "SELECT id, name, assistant_id, created_time, updated_time, conversation_kind,
                        parent_butler_conversation_id, source_task_title,
                        is_hidden_from_normal_chat_list, channel_source, butler_task_status,
                        butler_task_summary, butler_task_finalized_at
                 FROM conversation
                 WHERE id = ?",
                &[&id],
                |row| {
                    Ok(Conversation {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        assistant_id: row.get(2)?,
                        created_time: get_required_datetime_from_row(row, 3, "created_time")?,
                        updated_time: get_required_datetime_from_row(row, 4, "updated_time")?,
                        conversation_kind: row.get(5)?,
                        parent_butler_conversation_id: row.get(6)?,
                        source_task_title: row.get(7)?,
                        is_hidden_from_normal_chat_list: row.get(8)?,
                        channel_source: row.get(9)?,
                        butler_task_status: row.get(10)?,
                        butler_task_summary: row.get(11)?,
                        butler_task_finalized_at: get_datetime_from_row(row, 12)?,
                    })
                },
            )
            .optional()
    }

    #[instrument(level = "debug", skip(self, conversation), fields(id = conversation.id))]
    fn update(&self, conversation: &Conversation) -> Result<()> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE conversation
                 SET name = ?1,
                     assistant_id = ?2,
                     updated_time = ?3,
                     conversation_kind = ?4,
                     parent_butler_conversation_id = ?5,
                     source_task_title = ?6,
                     is_hidden_from_normal_chat_list = ?7,
                     channel_source = ?8,
                     butler_task_status = ?9,
                     butler_task_summary = ?10,
                     butler_task_finalized_at = ?11
                 WHERE id = ?12",
                (
                    &conversation.name,
                    &conversation.assistant_id,
                    &conversation.updated_time,
                    &conversation.conversation_kind,
                    &conversation.parent_butler_conversation_id,
                    &conversation.source_task_title,
                    &conversation.is_hidden_from_normal_chat_list,
                    &conversation.channel_source,
                    &conversation.butler_task_status,
                    &conversation.butler_task_summary,
                    &conversation.butler_task_finalized_at,
                    &conversation.id,
                ),
            )?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn delete(&self, id: i64) -> Result<()> {
        self.with_serialized_write(|conn| {
            conn.execute("DELETE FROM conversation WHERE id = ?", &[&id])?;
            Ok(())
        })
    }
}

pub struct MessageRepository {
    conn: Connection,
    write_lock: Arc<Mutex<()>>,
}

impl MessageRepository {
    #[instrument(level = "debug", skip(conn))]
    #[allow(dead_code)]
    pub fn new(conn: Connection) -> Self {
        Self::new_with_write_lock(conn, Arc::new(Mutex::new(())))
    }

    #[instrument(level = "debug", skip(conn, write_lock))]
    pub fn new_with_write_lock(conn: Connection, write_lock: Arc<Mutex<()>>) -> Self {
        MessageRepository { conn, write_lock }
    }

    fn with_serialized_write<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| sqlite_write_lock_poisoned_error("conversation.db"))?;
        f(&self.conn)
    }

    #[instrument(level = "debug", skip(self), fields(conversation_id = conversation_id))]
    pub fn list_by_conversation_id(
        &self,
        conversation_id: i64,
    ) -> Result<Vec<(Message, Option<MessageAttachment>)>> {
        let mut stmt = self.conn.prepare("SELECT message.id, message.parent_id, message.conversation_id, message.message_type, message.content, message.llm_model_id, message.llm_model_name, message.created_time, message.start_time, message.finish_time, message.token_count, message.input_token_count, message.output_token_count, message.generation_group_id, message.parent_group_id, message.tool_calls_json, message.metadata_json, message.first_token_time, message.ttft_ms, ma.attachment_type, ma.attachment_url, ma.attachment_content, ma.use_vector as attachment_use_vector, ma.token_count as attachment_token_count
                                          FROM message
                                          LEFT JOIN message_attachment ma ON message.id = ma.message_id
                                          WHERE message.conversation_id = ?1
                                          ORDER BY message.created_time ASC")?;
        let rows = stmt.query_map(&[&conversation_id], |row| {
            let attachment_type_int: Option<i64> = row.get(19).ok();
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
                metadata_json: row.get(16)?,
                first_token_time: get_datetime_from_row(row, 17)?,
                ttft_ms: row.get(18).ok(),
            };
            let attachment = if attachment_type.is_some() {
                Some(MessageAttachment {
                    id: 0,
                    message_id: row.get(0)?,
                    attachment_type: attachment_type.unwrap(),
                    attachment_url: row.get(20)?,
                    attachment_content: row.get(21)?,
                    attachment_hash: None,
                    use_vector: row.get(22)?,
                    token_count: row.get(23)?,
                })
            } else {
                None
            };
            Ok((message, attachment))
        })?;
        rows.collect()
    }

    fn insert_message(&self, message: &Message, touch_conversation: bool) -> Result<Message> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "INSERT INTO message (parent_id, conversation_id, message_type, content, llm_model_id, llm_model_name, created_time, start_time, finish_time, token_count, input_token_count, output_token_count, generation_group_id, parent_group_id, tool_calls_json, metadata_json, first_token_time, ttft_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
                    &message.metadata_json,
                    &message.first_token_time,
                    &message.ttft_ms,
                ],
            )?;
            if touch_conversation {
                conn.execute(
                    "UPDATE conversation SET updated_time = ?1 WHERE id = ?2",
                    rusqlite::params![&message.created_time, &message.conversation_id],
                )?;
            }
            let id = conn.last_insert_rowid();
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
                metadata_json: message.metadata_json.clone(),
                first_token_time: message.first_token_time,
                ttft_ms: message.ttft_ms,
            })
        })
    }

    #[instrument(level = "debug", skip(self, message), fields(conversation_id = message.conversation_id, message_type = message.message_type))]
    pub fn create_without_touch_conversation(&self, message: &Message) -> Result<Message> {
        self.insert_message(message, false)
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    pub fn update_finish_time(&self, id: i64) -> Result<()> {
        // Avoid SQLite CURRENT_TIMESTAMP (second precision) which can be earlier than millisecond
        // timestamps (e.g., first_token_time) and breaks duration-based TPS calculations.
        let now = chrono::Utc::now();
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE message SET finish_time = ?1 WHERE id = ?2",
                rusqlite::params![now, id],
            )?;
            Ok(())
        })
    }

    /// 更新消息内容
    #[instrument(level = "debug", skip(self, content), fields(id = id, content_len = content.len()))]
    pub fn update_content(&self, id: i64, content: &str) -> Result<()> {
        self.with_serialized_write(|conn| {
            conn.execute("UPDATE message SET content = ?1 WHERE id = ?2", (content, id))?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self, metadata_json), fields(id = id))]
    pub fn update_metadata(&self, id: i64, metadata_json: Option<&str>) -> Result<()> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE message SET metadata_json = ?1 WHERE id = ?2",
                rusqlite::params![metadata_json, id],
            )?;
            Ok(())
        })
    }

    /// 更新对话中所有正在进行的消息的 finish_time（用于取消操作）
    /// 只更新 start_time IS NOT NULL 且 finish_time IS NULL 的消息
    #[instrument(level = "debug", skip(self), fields(conversation_id = conversation_id))]
    pub fn finish_pending_messages(&self, conversation_id: i64) -> Result<usize> {
        let now = chrono::Utc::now();
        self.with_serialized_write(|conn| {
            let updated = conn.execute(
                "UPDATE message SET finish_time = ?1 WHERE conversation_id = ?2 AND start_time IS NOT NULL AND finish_time IS NULL",
                rusqlite::params![now, conversation_id],
            )?;
            Ok(updated)
        })
    }
}

impl Repository<Message> for MessageRepository {
    #[instrument(level = "debug", skip(self, message), fields(conversation_id = message.conversation_id, message_type = message.message_type))]
    fn create(&self, message: &Message) -> Result<Message> {
        self.insert_message(message, true)
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn read(&self, id: i64) -> Result<Option<Message>> {
        self.conn
            .query_row("SELECT id, parent_id, conversation_id, message_type, content, llm_model_id, llm_model_name, created_time, start_time, finish_time, token_count, input_token_count, output_token_count, generation_group_id, parent_group_id, tool_calls_json, metadata_json, first_token_time, ttft_ms FROM message WHERE id = ?", &[&id], |row| {
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
                    metadata_json: row.get(16)?,
                    first_token_time: get_datetime_from_row(row, 17)?,
                    ttft_ms: row.get(18).ok(),
                })
            })
            .optional()
    }

    #[instrument(level = "debug", skip(self, message), fields(id = message.id))]
    fn update(&self, message: &Message) -> Result<()> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE message SET conversation_id = ?1, message_type = ?2, content = ?3, llm_model_id = ?4, llm_model_name = ?5, token_count = ?6, input_token_count = ?7, output_token_count = ?8, tool_calls_json = ?9, metadata_json = ?10, first_token_time = ?11, ttft_ms = ?12, start_time = ?13, finish_time = ?14 WHERE id = ?15",
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
                    &message.metadata_json,
                    &message.first_token_time,
                    &message.ttft_ms,
                    &message.start_time,
                    &message.finish_time,
                    &message.id,
                ],
            )?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn delete(&self, id: i64) -> Result<()> {
        self.with_serialized_write(|conn| {
            conn.execute("DELETE FROM message WHERE id = ?", &[&id])?;
            Ok(())
        })
    }
}

pub struct MessageAttachmentRepository {
    conn: Connection,
    write_lock: Arc<Mutex<()>>,
}

impl MessageAttachmentRepository {
    #[instrument(level = "debug", skip(conn))]
    #[allow(dead_code)]
    pub fn new(conn: Connection) -> Self {
        Self::new_with_write_lock(conn, Arc::new(Mutex::new(())))
    }

    #[instrument(level = "debug", skip(conn, write_lock))]
    pub fn new_with_write_lock(conn: Connection, write_lock: Arc<Mutex<()>>) -> Self {
        MessageAttachmentRepository { conn, write_lock }
    }

    fn with_serialized_write<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| sqlite_write_lock_poisoned_error("conversation.db"))?;
        f(&self.conn)
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

    pub fn update_image_content(&self, id: i64, attachment_url: &str, attachment_content: &str) -> Result<()> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE message_attachment SET attachment_url = ?1, attachment_content = ?2 WHERE id = ?3",
                (attachment_url, attachment_content, &id),
            )?;
            Ok(())
        })
    }
}

impl Repository<MessageAttachment> for MessageAttachmentRepository {
    #[instrument(level = "debug", skip(self, attachment), fields(message_id = attachment.message_id, attachment_type = ?(attachment.attachment_type as i64)))]
    fn create(&self, attachment: &MessageAttachment) -> Result<MessageAttachment> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "INSERT INTO message_attachment (message_id, attachment_type, attachment_url, attachment_content, attachment_hash, use_vector, token_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (&attachment.message_id, &(attachment.attachment_type as i64), &attachment.attachment_url, &attachment.attachment_content, &attachment.attachment_hash, &attachment.use_vector, &attachment.token_count),
            )?;
            let id = conn.last_insert_rowid();
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
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE message_attachment SET message_id = ?1 WHERE id = ?2",
                (&attachment.message_id, &attachment.id),
            )?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn delete(&self, id: i64) -> Result<()> {
        self.with_serialized_write(|conn| {
            conn.execute("DELETE FROM message_attachment WHERE id = ?", &[&id])?;
            Ok(())
        })
    }
}

pub struct QueuedConversationMessageRepository {
    conn: Connection,
    write_lock: Arc<Mutex<()>>,
}

impl QueuedConversationMessageRepository {
    #[instrument(level = "debug", skip(conn, write_lock))]
    pub fn new_with_write_lock(conn: Connection, write_lock: Arc<Mutex<()>>) -> Self {
        QueuedConversationMessageRepository { conn, write_lock }
    }

    fn with_serialized_write<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| sqlite_write_lock_poisoned_error("conversation.db"))?;
        f(&self.conn)
    }

    fn queued_message_from_row(row: &rusqlite::Row<'_>) -> Result<QueuedConversationMessage> {
        Ok(QueuedConversationMessage {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            queue_kind: row.get(2)?,
            status: row.get(3)?,
            request_json: row.get(4)?,
            prompt: row.get(5)?,
            assistant_id: row.get(6)?,
            created_time: get_required_datetime_from_row(row, 7, "created_time")?,
            updated_time: get_required_datetime_from_row(row, 8, "updated_time")?,
        })
    }

    #[instrument(level = "debug", skip(self, request_json, prompt), fields(conversation_id, queue_kind, assistant_id))]
    pub fn enqueue(
        &self,
        conversation_id: i64,
        queue_kind: &str,
        request_json: &str,
        prompt: &str,
        assistant_id: i64,
    ) -> Result<QueuedConversationMessage> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "INSERT INTO queued_conversation_message
                    (conversation_id, queue_kind, status, request_json, prompt, assistant_id, created_time, updated_time)
                 VALUES (?1, ?2, 'queued', ?3, ?4, ?5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
                params![conversation_id, queue_kind, request_json, prompt, assistant_id],
            )?;
            let id = conn.last_insert_rowid();
            conn.query_row(
                "SELECT id, conversation_id, queue_kind, status, request_json, prompt, assistant_id, created_time, updated_time
                 FROM queued_conversation_message
                 WHERE id = ?1",
                params![id],
                Self::queued_message_from_row,
            )
        })
    }

    #[instrument(level = "debug", skip(self), fields(conversation_id))]
    pub fn list_queued_by_conversation(
        &self,
        conversation_id: i64,
    ) -> Result<Vec<QueuedConversationMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, queue_kind, status, request_json, prompt, assistant_id, created_time, updated_time
             FROM queued_conversation_message
             WHERE conversation_id = ?1 AND status = 'queued'
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![conversation_id], Self::queued_message_from_row)?;
        rows.collect()
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn promote_to_interrupt(&self, id: i64) -> Result<Option<QueuedConversationMessage>> {
        self.with_serialized_write(|conn| {
            let changed = conn.execute(
                "UPDATE queued_conversation_message
                 SET queue_kind = 'interrupt', updated_time = CURRENT_TIMESTAMP
                 WHERE id = ?1 AND status = 'queued'",
                params![id],
            )?;
            if changed == 0 {
                return Ok(None);
            }
            conn.query_row(
                "SELECT id, conversation_id, queue_kind, status, request_json, prompt, assistant_id, created_time, updated_time
                 FROM queued_conversation_message
                 WHERE id = ?1",
                params![id],
                Self::queued_message_from_row,
            )
            .optional()
        })
    }

    #[instrument(level = "debug", skip(self), fields(conversation_id, interrupt_only))]
    pub fn take_next_for_dispatch(
        &self,
        conversation_id: i64,
        interrupt_only: bool,
    ) -> Result<Option<QueuedConversationMessage>> {
        self.with_serialized_write(|conn| {
            let sql = if interrupt_only {
                "SELECT id, conversation_id, queue_kind, status, request_json, prompt, assistant_id, created_time, updated_time
                 FROM queued_conversation_message
                 WHERE conversation_id = ?1 AND status = 'queued' AND queue_kind = 'interrupt'
                 ORDER BY id ASC
                 LIMIT 1"
            } else {
                "SELECT id, conversation_id, queue_kind, status, request_json, prompt, assistant_id, created_time, updated_time
                 FROM queued_conversation_message
                 WHERE conversation_id = ?1 AND status = 'queued'
                 ORDER BY CASE queue_kind WHEN 'interrupt' THEN 0 ELSE 1 END, id ASC
                 LIMIT 1"
            };
            let queued = conn
                .query_row(sql, params![conversation_id], Self::queued_message_from_row)
                .optional()?;
            let Some(mut queued) = queued else {
                return Ok(None);
            };
            let changed = conn.execute(
                "UPDATE queued_conversation_message
                 SET status = 'dispatching', updated_time = CURRENT_TIMESTAMP
                 WHERE id = ?1 AND status = 'queued'",
                params![queued.id],
            )?;
            if changed == 0 {
                return Ok(None);
            }
            queued.status = "dispatching".to_string();
            Ok(Some(queued))
        })
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn finish_dispatch(&self, id: i64) -> Result<()> {
        self.with_serialized_write(|conn| {
            conn.execute("DELETE FROM queued_conversation_message WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn reset_dispatch(&self, id: i64) -> Result<Option<QueuedConversationMessage>> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE queued_conversation_message
                 SET status = 'queued', updated_time = CURRENT_TIMESTAMP
                 WHERE id = ?1",
                params![id],
            )?;
            conn.query_row(
                "SELECT id, conversation_id, queue_kind, status, request_json, prompt, assistant_id, created_time, updated_time
                 FROM queued_conversation_message
                 WHERE id = ?1",
                params![id],
                Self::queued_message_from_row,
            )
            .optional()
        })
    }
}

pub struct ConversationDatabase {
    db_path: PathBuf,
    write_lock: Arc<Mutex<()>>,
}

pub(crate) fn ensure_conversation_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS conversation (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            assistant_id INTEGER,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            conversation_kind TEXT NOT NULL DEFAULT 'normal',
            parent_butler_conversation_id INTEGER,
            source_task_title TEXT,
            is_hidden_from_normal_chat_list INTEGER NOT NULL DEFAULT 0,
            channel_source TEXT,
            butler_task_status TEXT,
            butler_task_summary TEXT,
            butler_task_finalized_at DATETIME
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_conversation_name ON conversation(name)", [])?;

    let mut conversation_stmt = conn.prepare("PRAGMA table_info(conversation)")?;
    let conversation_columns: Vec<String> = conversation_stmt
        .query_map([], |row| {
            let column_name: String = row.get(1)?;
            Ok(column_name)
        })?
        .collect::<Result<Vec<String>, _>>()?;

    if !conversation_columns.contains(&"updated_time".to_string()) {
        conn.execute("ALTER TABLE conversation ADD COLUMN updated_time DATETIME", [])?;
    }
    conn.execute(
        "UPDATE conversation
         SET updated_time = COALESCE(updated_time, created_time, CURRENT_TIMESTAMP)
         WHERE updated_time IS NULL",
        [],
    )?;

    if !conversation_columns.contains(&"conversation_kind".to_string()) {
        conn.execute(
            "ALTER TABLE conversation ADD COLUMN conversation_kind TEXT NOT NULL DEFAULT 'normal'",
            [],
        )?;
    }
    conn.execute(
        "UPDATE conversation
         SET conversation_kind = 'butler_main'
         WHERE conversation_kind = 'butler_main_archive'",
        [],
    )?;
    if !conversation_columns.contains(&"parent_butler_conversation_id".to_string()) {
        conn.execute(
            "ALTER TABLE conversation ADD COLUMN parent_butler_conversation_id INTEGER",
            [],
        )?;
    }
    if !conversation_columns.contains(&"source_task_title".to_string()) {
        conn.execute("ALTER TABLE conversation ADD COLUMN source_task_title TEXT", [])?;
    }
    if !conversation_columns.contains(&"is_hidden_from_normal_chat_list".to_string()) {
        conn.execute(
            "ALTER TABLE conversation ADD COLUMN is_hidden_from_normal_chat_list INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !conversation_columns.contains(&"channel_source".to_string()) {
        conn.execute("ALTER TABLE conversation ADD COLUMN channel_source TEXT", [])?;
    }
    if !conversation_columns.contains(&"butler_task_status".to_string()) {
        conn.execute("ALTER TABLE conversation ADD COLUMN butler_task_status TEXT", [])?;
    }
    if !conversation_columns.contains(&"butler_task_summary".to_string()) {
        conn.execute("ALTER TABLE conversation ADD COLUMN butler_task_summary TEXT", [])?;
    }
    if !conversation_columns.contains(&"butler_task_finalized_at".to_string()) {
        conn.execute("ALTER TABLE conversation ADD COLUMN butler_task_finalized_at DATETIME", [])?;
    }

    Ok(())
}

pub(crate) fn ensure_agent_session_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS acp_session (
            conversation_id INTEGER NOT NULL,
            session_id TEXT NOT NULL,
            agent_kind TEXT NOT NULL DEFAULT 'acp',
            updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (conversation_id, agent_kind)
        )",
        [],
    )?;
    let columns: Vec<(String, i64)> = conn
        .prepare("PRAGMA table_info(acp_session)")?
        .query_map([], |row| Ok((row.get(1)?, row.get(5)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let has_agent_kind = columns.iter().any(|(name, _)| name == "agent_kind");
    let has_composite_primary_key = columns.iter().filter(|(_, pk)| *pk > 0).count() >= 2;
    if !has_agent_kind || !has_composite_primary_key {
        let legacy_agent_kind = if has_agent_kind { "agent_kind" } else { "'acp'" };
        conn.execute_batch(&format!(
            "ALTER TABLE acp_session RENAME TO acp_session_legacy;
             CREATE TABLE acp_session (
                 conversation_id INTEGER NOT NULL,
                 session_id TEXT NOT NULL,
                 agent_kind TEXT NOT NULL DEFAULT 'acp',
                 updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                 PRIMARY KEY (conversation_id, agent_kind)
             );
             INSERT INTO acp_session (conversation_id, session_id, agent_kind, updated_time)
             SELECT conversation_id, session_id, {legacy_agent_kind}, updated_time
             FROM acp_session_legacy;
             DROP TABLE acp_session_legacy;"
        ))?;
    }
    Ok(())
}

impl ConversationDatabase {
    pub fn new(app_handle: &tauri::AppHandle) -> rusqlite::Result<Self> {
        let db_path = get_db_path(app_handle, "conversation.db");
        let db_path = db_path.unwrap();
        let write_lock = get_db_write_lock(&db_path);

        Ok(ConversationDatabase { db_path, write_lock })
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

    pub fn write_lock(&self) -> Arc<Mutex<()>> {
        self.write_lock.clone()
    }

    pub fn with_write_connection<T, F>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&Connection) -> Result<T, AppError>,
    {
        let _guard = self.write_lock.lock().map_err(|_| {
            AppError::UnknownError("conversation db write lock poisoned".to_string())
        })?;
        let conn = self.get_connection().map_err(AppError::from)?;
        f(&conn)
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn conversation_repo(&self) -> Result<ConversationRepository, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        Ok(ConversationRepository::new_with_write_lock(conn, self.write_lock()))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn message_repo(&self) -> Result<MessageRepository, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        Ok(MessageRepository::new_with_write_lock(conn, self.write_lock()))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn attachment_repo(&self) -> Result<MessageAttachmentRepository, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        Ok(MessageAttachmentRepository::new_with_write_lock(conn, self.write_lock()))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn queued_message_repo(&self) -> Result<QueuedConversationMessageRepository, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        Ok(QueuedConversationMessageRepository::new_with_write_lock(conn, self.write_lock()))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn conversation_summary_repo(&self) -> Result<ConversationSummaryRepository, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        Ok(ConversationSummaryRepository::new_with_write_lock(conn, self.write_lock()))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn butler_repo(&self) -> Result<ButlerRepository, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        Ok(ButlerRepository::new_with_write_lock(conn, self.write_lock()))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn create_tables(&self) -> rusqlite::Result<()> {
        let conn = self.get_connection().unwrap();

        ensure_conversation_table(&conn)?;
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
                tool_calls_json TEXT,
                metadata_json TEXT
            )",
            [],
        )?;

        // Older databases used conversation_id as the sole primary key and had
        // no agent_kind column. Rebuild that small table so ACP and native
        // Codex threads can coexist for one conversation without overwriting
        // each other. Existing rows are ACP sessions for backward compatibility.
        ensure_agent_session_table(&conn)?;

        // 添加迁移逻辑：如果新增列不存在，则按需补齐
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
        if !column_info.contains(&"metadata_json".to_string()) {
            conn.execute("ALTER TABLE message ADD COLUMN metadata_json TEXT", [])?;
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
        conn.execute("CREATE INDEX IF NOT EXISTS idx_message_content ON message(content)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_message_parent_id ON message(parent_id)", [])?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_message_attachment_message_id ON message_attachment(message_id)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS queued_conversation_message (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL,
                queue_kind TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'queued',
                request_json TEXT NOT NULL,
                prompt TEXT NOT NULL,
                assistant_id INTEGER NOT NULL,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_queued_conversation_message_pending
             ON queued_conversation_message(conversation_id, status, queue_kind, id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversation_kind_hidden_created ON conversation(conversation_kind, is_hidden_from_normal_chat_list, created_time DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversation_parent_butler_updated ON conversation(parent_butler_conversation_id, updated_time DESC)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS external_channel_message_link (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel TEXT NOT NULL,
                external_message_id TEXT NOT NULL,
                external_chat_id TEXT,
                external_user_id TEXT,
                conversation_id INTEGER NOT NULL,
                local_message_id INTEGER,
                direction TEXT NOT NULL,
                payload_type TEXT NOT NULL DEFAULT 'text',
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(channel, external_message_id)
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_external_channel_message_conversation ON external_channel_message_link(conversation_id, created_time DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_external_channel_message_local ON external_channel_message_link(local_message_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_external_channel_message_chat_direction ON external_channel_message_link(channel, external_chat_id, direction)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS external_channel_relay_scope (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel TEXT NOT NULL,
                conversation_id INTEGER NOT NULL,
                origin TEXT NOT NULL,
                external_chat_id TEXT,
                external_user_id TEXT,
                anchor_external_message_id TEXT NOT NULL,
                start_after_local_message_id INTEGER NOT NULL DEFAULT 0,
                last_delivered_local_message_id INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending',
                last_error TEXT,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_time DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_external_channel_relay_scope_conversation
             ON external_channel_relay_scope(channel, conversation_id, created_time DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_external_channel_relay_scope_status
             ON external_channel_relay_scope(channel, status, updated_time DESC)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS external_channel_message_delivery (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scope_id INTEGER NOT NULL,
                channel TEXT NOT NULL,
                conversation_id INTEGER NOT NULL,
                local_message_id INTEGER NOT NULL,
                external_message_id TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                rendered_text TEXT,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(scope_id, local_message_id)
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_external_channel_message_delivery_scope
             ON external_channel_message_delivery(scope_id, local_message_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_external_channel_message_delivery_conversation
             ON external_channel_message_delivery(channel, conversation_id, local_message_id)",
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
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversation_summary_summary ON conversation_summary(summary)",
            [],
        )?;

        // 创建对话Todo表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS conversation_todo (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL,
                content TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                active_form TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversation_todo_conversation_id ON conversation_todo(conversation_id)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS butler_main_state (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                butler_conversation_id INTEGER NOT NULL,
                slot TEXT NOT NULL UNIQUE DEFAULT 'default',
                last_active_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (butler_conversation_id) REFERENCES conversation(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS butler_task_definition (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                butler_conversation_id INTEGER NOT NULL,
                task_conversation_id INTEGER NOT NULL UNIQUE,
                title TEXT NOT NULL,
                goal TEXT NOT NULL,
                executor_assistant_id INTEGER NOT NULL,
                executor_assistant_source TEXT NOT NULL,
                permission_template_source TEXT,
                handoff_contract_json TEXT,
                result_handling_mode TEXT,
                notification_policy TEXT,
                temporary_trusted_paths_json TEXT NOT NULL DEFAULT '[]',
                temporary_skill_identifiers_json TEXT NOT NULL DEFAULT '[]',
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (butler_conversation_id) REFERENCES conversation(id) ON DELETE CASCADE,
                FOREIGN KEY (task_conversation_id) REFERENCES conversation(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_butler_task_definition_parent ON butler_task_definition(butler_conversation_id, created_time DESC)",
            [],
        )?;
        ensure_column_exists(
            &conn,
            "butler_task_definition",
            "temporary_trusted_paths_json",
            "TEXT NOT NULL DEFAULT '[]'",
        )?;
        ensure_column_exists(
            &conn,
            "butler_task_definition",
            "temporary_skill_identifiers_json",
            "TEXT NOT NULL DEFAULT '[]'",
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS butler_task_result (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_conversation_id INTEGER NOT NULL UNIQUE,
                handoff_mode TEXT,
                payload_json TEXT,
                summary TEXT,
                structured_output_json TEXT,
                evidence_json TEXT,
                artifact_refs_json TEXT,
                followup_suggestions_json TEXT,
                followup_status TEXT DEFAULT 'enqueued',
                handoff_message_id INTEGER,
                final_message_id INTEGER,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (task_conversation_id) REFERENCES conversation(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_butler_task_result_task ON butler_task_result(task_conversation_id)",
            [],
        )?;
        ensure_column_exists(
            &conn,
            "butler_task_result",
            "followup_status",
            "TEXT DEFAULT 'enqueued'",
        )?;
        ensure_column_exists(&conn, "butler_task_result", "handoff_message_id", "INTEGER")?;

        Ok(())
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn get_acp_session_id(&self, conversation_id: i64) -> Result<Option<String>, AppError> {
        self.get_agent_session_id(conversation_id, "acp")
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn get_agent_session_id(
        &self,
        conversation_id: i64,
        agent_kind: &str,
    ) -> Result<Option<String>, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        let session_id = conn
            .query_row(
                "SELECT session_id FROM acp_session WHERE conversation_id = ?1 AND agent_kind = ?2",
                params![conversation_id, agent_kind],
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
        self.upsert_agent_session_id(conversation_id, "acp", session_id)
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn upsert_agent_session_id(
        &self,
        conversation_id: i64,
        agent_kind: &str,
        session_id: &str,
    ) -> Result<(), AppError> {
        self.with_write_connection(|conn| {
            conn.execute(
                "INSERT INTO acp_session (conversation_id, agent_kind, session_id, updated_time)
                 VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
                 ON CONFLICT(conversation_id, agent_kind)
                 DO UPDATE SET session_id = excluded.session_id, updated_time = CURRENT_TIMESTAMP",
                params![conversation_id, agent_kind, session_id],
            )
            .map_err(AppError::from)?;
            Ok(())
        })
    }


    #[instrument(level = "debug", skip(self), err)]
    pub fn delete_agent_session_id(
        &self,
        conversation_id: i64,
        agent_kind: &str,
    ) -> Result<(), AppError> {
        self.with_write_connection(|conn| {
            conn.execute(
                "DELETE FROM acp_session WHERE conversation_id = ?1 AND agent_kind = ?2",
                params![conversation_id, agent_kind],
            )
            .map_err(AppError::from)?;
            Ok(())
        })
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

        let mut thought_tokens_total: i64 = 0;
        let mut cached_input_tokens_total: i64 = 0;
        let mut cached_read_tokens_total: i64 = 0;
        let mut cached_write_tokens_total: i64 = 0;
        let mut estimated_message_count: i64 = 0;
        let mut metadata_by_model: HashMap<Option<i64>, PersistedUsageMetadata> = HashMap::new();

        let mut metadata_stmt = conn.prepare(
            "SELECT llm_model_id, metadata_json
             FROM message
             WHERE conversation_id = ?1 AND message_type IN ('response', 'reasoning')",
        )?;

        let metadata_rows = metadata_stmt.query_map(&[&conversation_id], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })?;

        for row in metadata_rows {
            let (model_id, metadata_json) = row?;
            let metadata = parse_persisted_usage_metadata(metadata_json.as_deref());
            thought_tokens_total += metadata.thought_tokens;
            cached_input_tokens_total += metadata.cached_input_tokens;
            cached_read_tokens_total += metadata.cached_read_tokens;
            cached_write_tokens_total += metadata.cached_write_tokens;
            if metadata.usage_source.as_deref() == Some("estimated") {
                estimated_message_count += 1;
            }

            let entry = metadata_by_model.entry(model_id).or_default();
            entry.thought_tokens += metadata.thought_tokens;
            entry.cached_input_tokens += metadata.cached_input_tokens;
            entry.cached_read_tokens += metadata.cached_read_tokens;
            entry.cached_write_tokens += metadata.cached_write_tokens;
        }

        // 获取对话的开始时间和结束时间
        let (start_time, finish_time): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT
                    MIN(start_time) as earliest_start,
                    MAX(finish_time) as latest_finish
                FROM message
                WHERE conversation_id = ?1",
                &[&conversation_id],
                |row| Ok((row.get(0).ok(), row.get(1).ok())),
            )
            .unwrap_or((None, None));

        let start_time_parsed = start_time
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)));

        let finish_time_parsed = finish_time
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)));

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
                let model_id: Option<i64> = row.get(0)?;
                let extra_usage = metadata_by_model.get(&model_id).cloned().unwrap_or_default();
                Ok(ModelTokenBreakdown {
                    model_id,
                    model_name: row.get(1).unwrap_or_else(|_| "Unknown".to_string()),
                    total_tokens: row.get(2)?,
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    thought_tokens: extra_usage.thought_tokens,
                    cached_input_tokens: extra_usage.cached_input_tokens,
                    cached_read_tokens: extra_usage.cached_read_tokens,
                    cached_write_tokens: extra_usage.cached_write_tokens,
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
            thought_tokens: thought_tokens_total as i32,
            cached_input_tokens: cached_input_tokens_total as i32,
            cached_read_tokens: cached_read_tokens_total as i32,
            cached_write_tokens: cached_write_tokens_total as i32,
            by_model,
            estimated_message_count: estimated_message_count as i32,
            message_count: message_count as i32,
            system_message_count: system_count as i32,
            user_message_count: user_count as i32,
            response_message_count: response_count as i32,
            reasoning_message_count: reasoning_count as i32,
            tool_result_message_count: tool_result_count as i32,
            avg_ttft_ms: avg_ttft,
            avg_tps: aggregated_avg_tps.or(avg_tps),
            start_time: start_time_parsed,
            finish_time: finish_time_parsed,
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
                metadata_json,
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
                let metadata = parse_persisted_usage_metadata(row.get::<_, Option<String>>(4)?.as_deref());
                let first_token_time = get_datetime_from_row(row, 7)?;
                let finish_time = get_datetime_from_row(row, 8)?;
                let start_time = get_datetime_from_row(row, 9)?;
                let created_time = get_required_datetime_from_row(row, 10, "created_time")?;
                let ttft_ms: Option<i64> =
                    row.get(6).ok().or_else(|| match (start_time, first_token_time) {
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
                    thought_tokens: metadata.thought_tokens as i32,
                    cached_input_tokens: metadata.cached_input_tokens as i32,
                    cached_read_tokens: metadata.cached_read_tokens as i32,
                    cached_write_tokens: metadata.cached_write_tokens as i32,
                    usage_source: metadata.usage_source,
                    model_name: row.get(5).ok(),
                    ttft_ms,
                    tps,
                    start_time,
                    finish_time,
                })
            },
        )
    }

    // ============= Todo CRUD Methods =============

    /// Get all todos for a conversation
    #[instrument(level = "debug", skip(self), err)]
    pub fn get_todos(&self, conversation_id: i64) -> Result<Vec<ConversationTodo>, AppError> {
        let conn = self.get_connection().map_err(AppError::from)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, content, status, active_form, sort_order, created_time, updated_time
                 FROM conversation_todo
                 WHERE conversation_id = ?1
                 ORDER BY sort_order ASC, id ASC",
            )
            .map_err(AppError::from)?;

        let todos = stmt
            .query_map(params![conversation_id], |row| {
                Ok(ConversationTodo {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    content: row.get(2)?,
                    status: row.get(3)?,
                    active_form: row.get(4)?,
                    sort_order: row.get(5)?,
                    created_time: get_required_datetime_from_row(row, 6, "created_time")?,
                    updated_time: get_required_datetime_from_row(row, 7, "updated_time")?,
                })
            })
            .map_err(AppError::from)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(AppError::from)?;

        Ok(todos)
    }

    /// Replace all todos for a conversation (delete existing and insert new)
    #[instrument(level = "debug", skip(self, todos), err)]
    pub fn replace_todos(
        &self,
        conversation_id: i64,
        todos: Vec<ConversationTodoInput>,
    ) -> Result<(), AppError> {
        self.with_write_connection(|conn| {
            conn.execute("BEGIN TRANSACTION", []).map_err(AppError::from)?;

            conn.execute(
                "DELETE FROM conversation_todo WHERE conversation_id = ?1",
                params![conversation_id],
            )
            .map_err(|e| {
                let _ = conn.execute("ROLLBACK", []);
                AppError::from(e)
            })?;

            for (index, todo) in todos.iter().enumerate() {
                conn.execute(
                    "INSERT INTO conversation_todo (conversation_id, content, status, active_form, sort_order)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        conversation_id,
                        &todo.content,
                        &todo.status,
                        &todo.active_form,
                        index as i32
                    ],
                )
                .map_err(|e| {
                    let _ = conn.execute("ROLLBACK", []);
                    AppError::from(e)
                })?;
            }

            conn.execute(
                "COMMIT",
                [],
            )
            .map_err(AppError::from)?;

            debug!(
                conversation_id = conversation_id,
                todo_count = todos.len(),
                "Replaced todos for conversation"
            );

            Ok(())
        })
    }

    /// Delete all todos for a conversation
    #[instrument(level = "debug", skip(self), err)]
    pub fn delete_todos(&self, conversation_id: i64) -> Result<(), AppError> {
        self.with_write_connection(|conn| {
            conn.execute(
                "DELETE FROM conversation_todo WHERE conversation_id = ?1",
                params![conversation_id],
            )
            .map_err(AppError::from)?;
            Ok(())
        })
    }
}

/// Todo item stored in database
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationTodo {
    pub id: i64,
    pub conversation_id: i64,
    pub content: String,
    pub status: String,
    pub active_form: String,
    pub sort_order: i32,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub updated_time: DateTime<Utc>,
}

/// Input for creating/updating a todo item
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationTodoInput {
    pub content: String,
    pub status: String,
    pub active_form: String,
}

/// 对话token统计信息
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationTokenStats {
    pub total_tokens: i32,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub thought_tokens: i32,
    pub cached_input_tokens: i32,
    pub cached_read_tokens: i32,
    pub cached_write_tokens: i32,
    pub by_model: Vec<ModelTokenBreakdown>,
    pub estimated_message_count: i32,
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
    // 时间戳信息
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub finish_time: Option<DateTime<Utc>>,
}

/// 模型token分解信息
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelTokenBreakdown {
    pub model_id: Option<i64>,
    pub model_name: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub thought_tokens: i64,
    pub cached_input_tokens: i64,
    pub cached_read_tokens: i64,
    pub cached_write_tokens: i64,
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
    pub thought_tokens: i32,
    pub cached_input_tokens: i32,
    pub cached_read_tokens: i32,
    pub cached_write_tokens: i32,
    pub usage_source: Option<String>,
    pub model_name: Option<String>,
    pub ttft_ms: Option<i64>, // Time to First Token (毫秒)
    pub tps: Option<f64>,     // Tokens Per Second
    // 时间戳信息
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub finish_time: Option<DateTime<Utc>>,
}

/// 对话总结结构体
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationSummary {
    pub id: i64,
    pub conversation_id: i64,
    pub summary: String,      // 对话整体总结
    pub user_intent: String,  // 用户目的
    pub key_outcomes: String, // 关键成果
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
}

pub struct ConversationSummaryRepository {
    conn: Connection,
    write_lock: Arc<Mutex<()>>,
}

impl ConversationSummaryRepository {
    #[instrument(level = "debug", skip(conn, write_lock))]
    pub fn new_with_write_lock(conn: Connection, write_lock: Arc<Mutex<()>>) -> Self {
        ConversationSummaryRepository { conn, write_lock }
    }

    fn with_serialized_write<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| sqlite_write_lock_poisoned_error("conversation.db"))?;
        f(&self.conn)
    }

    #[instrument(level = "debug", skip(self))]
    pub fn create(&self, summary: &ConversationSummary) -> Result<ConversationSummary> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "INSERT INTO conversation_summary (conversation_id, summary, user_intent, key_outcomes, created_time) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    &summary.conversation_id,
                    &summary.summary,
                    &summary.user_intent,
                    &summary.key_outcomes,
                    &summary.created_time,
                ],
            )?;
            let id = conn.last_insert_rowid();
            Ok(ConversationSummary {
                id,
                conversation_id: summary.conversation_id,
                summary: summary.summary.clone(),
                user_intent: summary.user_intent.clone(),
                key_outcomes: summary.key_outcomes.clone(),
                created_time: summary.created_time,
            })
        })
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
        self.with_serialized_write(|conn| {
            conn.execute(
                "DELETE FROM conversation_summary WHERE conversation_id = ?",
                rusqlite::params![&conversation_id],
            )?;
            Ok(())
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ButlerMainState {
    pub id: i64,
    pub butler_conversation_id: i64,
    pub slot: String,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub last_active_at: DateTime<Utc>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub updated_time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ButlerTaskDefinition {
    pub id: i64,
    pub butler_conversation_id: i64,
    pub task_conversation_id: i64,
    pub title: String,
    pub goal: String,
    pub executor_assistant_id: i64,
    pub executor_assistant_source: String,
    pub permission_template_source: Option<String>,
    pub handoff_contract_json: Option<String>,
    pub result_handling_mode: Option<String>,
    pub notification_policy: Option<String>,
    #[serde(default)]
    pub temporary_trusted_paths: Vec<String>,
    #[serde(default)]
    pub temporary_skill_identifiers: Vec<String>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ButlerTaskResult {
    pub id: i64,
    pub task_conversation_id: i64,
    pub handoff_mode: Option<String>,
    pub payload_json: Option<String>,
    pub summary: Option<String>,
    pub structured_output_json: Option<String>,
    pub evidence_json: Option<String>,
    pub artifact_refs_json: Option<String>,
    pub followup_suggestions_json: Option<String>,
    pub followup_status: Option<String>,
    pub handoff_message_id: Option<i64>,
    pub final_message_id: Option<i64>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub created_time: DateTime<Utc>,
    #[serde(serialize_with = "serialize_datetime_millis")]
    pub updated_time: DateTime<Utc>,
}

pub struct ButlerRepository {
    conn: Connection,
    write_lock: Arc<Mutex<()>>,
}

impl ButlerRepository {
    #[instrument(level = "debug", skip(conn))]
    #[allow(dead_code)]
    pub fn new(conn: Connection) -> Self {
        Self::new_with_write_lock(conn, Arc::new(Mutex::new(())))
    }

    #[instrument(level = "debug", skip(conn, write_lock))]
    pub fn new_with_write_lock(conn: Connection, write_lock: Arc<Mutex<()>>) -> Self {
        Self { conn, write_lock }
    }

    fn with_serialized_write<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| sqlite_write_lock_poisoned_error("conversation.db"))?;
        f(&self.conn)
    }

    #[instrument(level = "debug", skip(self), fields(slot = slot))]
    pub fn get_main_state(&self, slot: &str) -> Result<Option<ButlerMainState>> {
        self.conn
            .query_row(
                "SELECT id, butler_conversation_id, slot, last_active_at, created_time, updated_time
                 FROM butler_main_state
                 WHERE slot = ?1",
                params![slot],
                |row| {
                    Ok(ButlerMainState {
                        id: row.get(0)?,
                        butler_conversation_id: row.get(1)?,
                        slot: row.get(2)?,
                        last_active_at: get_required_datetime_from_row(row, 3, "last_active_at")?,
                        created_time: get_required_datetime_from_row(row, 4, "created_time")?,
                        updated_time: get_required_datetime_from_row(row, 5, "updated_time")?,
                    })
                },
            )
            .optional()
    }

    #[instrument(level = "debug", skip(self), fields(slot = slot, butler_conversation_id = butler_conversation_id))]
    pub fn upsert_main_state(
        &self,
        butler_conversation_id: i64,
        slot: &str,
    ) -> Result<ButlerMainState> {
        let now = Utc::now();
        self.with_serialized_write(|conn| {
            conn.execute(
                "INSERT INTO butler_main_state (
                    butler_conversation_id, slot, last_active_at, created_time, updated_time
                ) VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(slot) DO UPDATE SET
                    butler_conversation_id = excluded.butler_conversation_id,
                    last_active_at = excluded.last_active_at,
                    updated_time = excluded.updated_time",
                params![butler_conversation_id, slot, now, now, now],
            )?;
            conn.query_row(
                "SELECT id, butler_conversation_id, slot, last_active_at, created_time, updated_time
                 FROM butler_main_state
                 WHERE slot = ?1",
                params![slot],
                |row| {
                    Ok(ButlerMainState {
                        id: row.get(0)?,
                        butler_conversation_id: row.get(1)?,
                        slot: row.get(2)?,
                        last_active_at: get_required_datetime_from_row(row, 3, "last_active_at")?,
                        created_time: get_required_datetime_from_row(row, 4, "created_time")?,
                        updated_time: get_required_datetime_from_row(row, 5, "updated_time")?,
                    })
                },
            )
        })
    }

    #[instrument(level = "debug", skip(self), fields(slot = slot))]
    pub fn touch_main_state(&self, slot: &str) -> Result<()> {
        let now = Utc::now();
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE butler_main_state
                 SET last_active_at = ?1, updated_time = ?1
                 WHERE slot = ?2",
                params![now, slot],
            )?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self, definition), fields(task_conversation_id = definition.task_conversation_id))]
    pub fn create_task_definition(
        &self,
        definition: &ButlerTaskDefinition,
    ) -> Result<ButlerTaskDefinition> {
        self.with_serialized_write(|conn| {
            let temporary_trusted_paths_json =
                serialize_string_array(&definition.temporary_trusted_paths)?;
            let temporary_skill_identifiers_json =
                serialize_string_array(&definition.temporary_skill_identifiers)?;
            conn.execute(
                "INSERT INTO butler_task_definition (
                    butler_conversation_id,
                    task_conversation_id,
                    title,
                    goal,
                    executor_assistant_id,
                    executor_assistant_source,
                    permission_template_source,
                    handoff_contract_json,
                    result_handling_mode,
                    notification_policy,
                    temporary_trusted_paths_json,
                    temporary_skill_identifiers_json,
                    created_time
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    definition.butler_conversation_id,
                    definition.task_conversation_id,
                    &definition.title,
                    &definition.goal,
                    definition.executor_assistant_id,
                    &definition.executor_assistant_source,
                    &definition.permission_template_source,
                    &definition.handoff_contract_json,
                    &definition.result_handling_mode,
                    &definition.notification_policy,
                    temporary_trusted_paths_json,
                    temporary_skill_identifiers_json,
                    definition.created_time,
                ],
            )?;
            Ok(ButlerTaskDefinition {
                id: conn.last_insert_rowid(),
                butler_conversation_id: definition.butler_conversation_id,
                task_conversation_id: definition.task_conversation_id,
                title: definition.title.clone(),
                goal: definition.goal.clone(),
                executor_assistant_id: definition.executor_assistant_id,
                executor_assistant_source: definition.executor_assistant_source.clone(),
                permission_template_source: definition.permission_template_source.clone(),
                handoff_contract_json: definition.handoff_contract_json.clone(),
                result_handling_mode: definition.result_handling_mode.clone(),
                notification_policy: definition.notification_policy.clone(),
                temporary_trusted_paths: definition.temporary_trusted_paths.clone(),
                temporary_skill_identifiers: definition.temporary_skill_identifiers.clone(),
                created_time: definition.created_time,
            })
        })
    }

    #[instrument(level = "debug", skip(self), fields(butler_conversation_id = butler_conversation_id))]
    pub fn list_task_definitions(
        &self,
        butler_conversation_id: i64,
    ) -> Result<Vec<ButlerTaskDefinition>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, butler_conversation_id, task_conversation_id, title, goal,
                    executor_assistant_id, executor_assistant_source,
                    permission_template_source, handoff_contract_json,
                    result_handling_mode, notification_policy,
                    temporary_trusted_paths_json, temporary_skill_identifiers_json, created_time
             FROM butler_task_definition
             WHERE butler_conversation_id = ?1
             ORDER BY created_time DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![butler_conversation_id], |row| {
            let temporary_trusted_paths =
                deserialize_string_array(11, row.get(11)?, "temporary_trusted_paths_json")?;
            let temporary_skill_identifiers =
                deserialize_string_array(12, row.get(12)?, "temporary_skill_identifiers_json")?;
            Ok(ButlerTaskDefinition {
                id: row.get(0)?,
                butler_conversation_id: row.get(1)?,
                task_conversation_id: row.get(2)?,
                title: row.get(3)?,
                goal: row.get(4)?,
                executor_assistant_id: row.get(5)?,
                executor_assistant_source: row.get(6)?,
                permission_template_source: row.get(7)?,
                handoff_contract_json: row.get(8)?,
                result_handling_mode: row.get(9)?,
                notification_policy: row.get(10)?,
                temporary_trusted_paths,
                temporary_skill_identifiers,
                created_time: get_required_datetime_from_row(row, 13, "created_time")?,
            })
        })?;
        rows.collect()
    }

    #[instrument(level = "debug", skip(self), fields(task_conversation_id = task_conversation_id))]
    pub fn get_task_definition_by_task_conversation_id(
        &self,
        task_conversation_id: i64,
    ) -> Result<Option<ButlerTaskDefinition>> {
        self.conn
            .query_row(
                "SELECT id, butler_conversation_id, task_conversation_id, title, goal,
                        executor_assistant_id, executor_assistant_source,
                        permission_template_source, handoff_contract_json,
                        result_handling_mode, notification_policy,
                        temporary_trusted_paths_json, temporary_skill_identifiers_json, created_time
                 FROM butler_task_definition
                 WHERE task_conversation_id = ?1",
                params![task_conversation_id],
                |row| {
                    let temporary_trusted_paths = deserialize_string_array(
                        11,
                        row.get(11)?,
                        "temporary_trusted_paths_json",
                    )?;
                    let temporary_skill_identifiers = deserialize_string_array(
                        12,
                        row.get(12)?,
                        "temporary_skill_identifiers_json",
                    )?;
                    Ok(ButlerTaskDefinition {
                        id: row.get(0)?,
                        butler_conversation_id: row.get(1)?,
                        task_conversation_id: row.get(2)?,
                        title: row.get(3)?,
                        goal: row.get(4)?,
                        executor_assistant_id: row.get(5)?,
                        executor_assistant_source: row.get(6)?,
                        permission_template_source: row.get(7)?,
                        handoff_contract_json: row.get(8)?,
                        result_handling_mode: row.get(9)?,
                        notification_policy: row.get(10)?,
                        temporary_trusted_paths,
                        temporary_skill_identifiers,
                        created_time: get_required_datetime_from_row(row, 13, "created_time")?,
                    })
                },
            )
            .optional()
    }

    #[instrument(level = "debug", skip(self), fields(origin_butler_conversation_id = origin_butler_conversation_id, new_butler_conversation_id = new_butler_conversation_id))]
    pub fn reassign_task_definitions(
        &self,
        origin_butler_conversation_id: i64,
        new_butler_conversation_id: i64,
    ) -> Result<()> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE butler_task_definition
                 SET butler_conversation_id = ?1
                 WHERE butler_conversation_id = ?2",
                params![new_butler_conversation_id, origin_butler_conversation_id],
            )?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self, result), fields(task_conversation_id = result.task_conversation_id))]
    pub fn upsert_task_result(&self, result: &ButlerTaskResult) -> Result<ButlerTaskResult> {
        self.with_serialized_write(|conn| {
            conn.execute(
                "INSERT INTO butler_task_result (
                    task_conversation_id,
                    handoff_mode,
                    payload_json,
                    summary,
                    structured_output_json,
                    evidence_json,
                    artifact_refs_json,
                    followup_suggestions_json,
                    followup_status,
                    handoff_message_id,
                    final_message_id,
                    created_time,
                    updated_time
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                ON CONFLICT(task_conversation_id) DO UPDATE SET
                    handoff_mode = excluded.handoff_mode,
                    payload_json = excluded.payload_json,
                    summary = excluded.summary,
                    structured_output_json = excluded.structured_output_json,
                    evidence_json = excluded.evidence_json,
                    artifact_refs_json = excluded.artifact_refs_json,
                    followup_suggestions_json = excluded.followup_suggestions_json,
                    followup_status = excluded.followup_status,
                    handoff_message_id = excluded.handoff_message_id,
                    final_message_id = excluded.final_message_id,
                    updated_time = excluded.updated_time",
                params![
                    result.task_conversation_id,
                    &result.handoff_mode,
                    &result.payload_json,
                    &result.summary,
                    &result.structured_output_json,
                    &result.evidence_json,
                    &result.artifact_refs_json,
                    &result.followup_suggestions_json,
                    &result.followup_status,
                    &result.handoff_message_id,
                    &result.final_message_id,
                    result.created_time,
                    result.updated_time,
                ],
            )?;
            conn.query_row(
                "SELECT id, task_conversation_id, handoff_mode, payload_json, summary,
                        structured_output_json, evidence_json, artifact_refs_json,
                        followup_suggestions_json, followup_status, handoff_message_id,
                        final_message_id, created_time, updated_time
                 FROM butler_task_result
                 WHERE task_conversation_id = ?1",
                params![result.task_conversation_id],
                |row| {
                    Ok(ButlerTaskResult {
                        id: row.get(0)?,
                        task_conversation_id: row.get(1)?,
                        handoff_mode: row.get(2)?,
                        payload_json: row.get(3)?,
                        summary: row.get(4)?,
                        structured_output_json: row.get(5)?,
                        evidence_json: row.get(6)?,
                        artifact_refs_json: row.get(7)?,
                        followup_suggestions_json: row.get(8)?,
                        followup_status: row.get(9)?,
                        handoff_message_id: row.get(10)?,
                        final_message_id: row.get(11)?,
                        created_time: get_required_datetime_from_row(row, 12, "created_time")?,
                        updated_time: get_required_datetime_from_row(row, 13, "updated_time")?,
                    })
                },
            )
        })
    }

    #[instrument(level = "debug", skip(self), fields(task_conversation_id = task_conversation_id))]
    pub fn get_task_result(&self, task_conversation_id: i64) -> Result<Option<ButlerTaskResult>> {
        self.conn
            .query_row(
                "SELECT id, task_conversation_id, handoff_mode, payload_json, summary,
                        structured_output_json, evidence_json, artifact_refs_json,
                        followup_suggestions_json, followup_status, handoff_message_id,
                        final_message_id, created_time, updated_time
                 FROM butler_task_result
                 WHERE task_conversation_id = ?1",
                params![task_conversation_id],
                |row| {
                    Ok(ButlerTaskResult {
                        id: row.get(0)?,
                        task_conversation_id: row.get(1)?,
                        handoff_mode: row.get(2)?,
                        payload_json: row.get(3)?,
                        summary: row.get(4)?,
                        structured_output_json: row.get(5)?,
                        evidence_json: row.get(6)?,
                        artifact_refs_json: row.get(7)?,
                        followup_suggestions_json: row.get(8)?,
                        followup_status: row.get(9)?,
                        handoff_message_id: row.get(10)?,
                        final_message_id: row.get(11)?,
                        created_time: get_required_datetime_from_row(row, 12, "created_time")?,
                        updated_time: get_required_datetime_from_row(row, 13, "updated_time")?,
                    })
                },
            )
            .optional()
    }

    #[instrument(level = "debug", skip(self), fields(task_conversation_id = task_conversation_id, followup_status = followup_status))]
    pub fn update_task_result_followup_state(
        &self,
        task_conversation_id: i64,
        followup_status: &str,
        handoff_message_id: Option<i64>,
    ) -> Result<()> {
        let now = Utc::now();
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE butler_task_result
                 SET followup_status = ?1,
                     handoff_message_id = ?2,
                     updated_time = ?3
                 WHERE task_conversation_id = ?4",
                params![followup_status, handoff_message_id, now, task_conversation_id],
            )?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(task_conversation_id = task_conversation_id))]
    pub fn try_mark_task_result_followup_dispatching(
        &self,
        task_conversation_id: i64,
        handoff_message_id: Option<i64>,
    ) -> Result<bool> {
        let now = Utc::now();
        self.with_serialized_write(|conn| {
            let affected = conn.execute(
                "UPDATE butler_task_result
                 SET followup_status = 'dispatching',
                     handoff_message_id = COALESCE(?1, handoff_message_id),
                     updated_time = ?2
                 WHERE task_conversation_id = ?3
                   AND COALESCE(followup_status, 'pending') IN ('pending', 'handoff_injected')",
                params![handoff_message_id, now, task_conversation_id],
            )?;
            Ok(affected > 0)
        })
    }

    #[instrument(level = "debug", skip(self), fields(task_conversation_id = task_conversation_id, status = status))]
    pub fn update_task_conversation_state(
        &self,
        task_conversation_id: i64,
        status: &str,
        summary: Option<&str>,
        finalized_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let now = Utc::now();
        self.with_serialized_write(|conn| {
            conn.execute(
                "UPDATE conversation
                 SET butler_task_status = ?1,
                     butler_task_summary = COALESCE(?2, butler_task_summary),
                     butler_task_finalized_at = COALESCE(?3, butler_task_finalized_at),
                     updated_time = ?4
                 WHERE id = ?5",
                params![status, summary, finalized_at, now, task_conversation_id],
            )?;
            Ok(())
        })
    }
}
