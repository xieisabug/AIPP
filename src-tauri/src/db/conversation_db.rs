use std::path::PathBuf;
use tauri::Manager; // for try_state
use crate::utils::db_utils::build_remote_dsn;

use chrono::{DateTime, Utc};
use sea_orm::Schema;
use sea_orm::{
    entity::prelude::*, ActiveValue, Database, DatabaseBackend, DatabaseConnection, DbErr,
    QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::errors::AppError;

use super::get_db_path;

// ============ AttachmentType Enum ============
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
    type Error = String;

    fn try_from(value: i64) -> std::result::Result<Self, Self::Error> {
        match value {
            1 => Ok(AttachmentType::Image),
            2 => Ok(AttachmentType::Text),
            3 => Ok(AttachmentType::PDF),
            4 => Ok(AttachmentType::Word),
            5 => Ok(AttachmentType::PowerPoint),
            6 => Ok(AttachmentType::Excel),
            _ => Err(format!("Invalid attachment type: {}", value)),
        }
    }
}

// ============ Conversation Entity ============
pub mod conversation {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "conversation")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub assistant_id: Option<i64>,
        pub created_time: Option<ChronoDateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ Message Entity ============
pub mod message {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "message")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub parent_id: Option<i64>,
        pub conversation_id: i64,
        pub message_type: String,
        pub content: String,
        pub llm_model_id: Option<i64>,
        pub llm_model_name: Option<String>,
        pub created_time: Option<ChronoDateTimeUtc>,
        pub start_time: Option<ChronoDateTimeUtc>,
        pub finish_time: Option<ChronoDateTimeUtc>,
        pub token_count: i32,
        pub generation_group_id: Option<String>,
        pub parent_group_id: Option<String>,
        pub tool_calls_json: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ MessageAttachment Entity ============
pub mod message_attachment {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "message_attachment")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub message_id: i64,
        pub attachment_type: i64,
        pub attachment_url: Option<String>,
        pub attachment_content: Option<String>,
        pub attachment_hash: Option<String>,
        pub use_vector: bool,
        pub token_count: Option<i32>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ Legacy structs for backward compatibility ============
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Conversation {
    pub id: i64,
    pub name: String,
    pub assistant_id: Option<i64>,
    pub created_time: DateTime<Utc>,
}

impl From<conversation::Model> for Conversation {
    fn from(model: conversation::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            assistant_id: model.assistant_id,
            created_time: model
                .created_time
                .map(|dt| dt.naive_utc().and_utc())
                .unwrap_or_else(Utc::now),
        }
    }
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
    pub created_time: DateTime<Utc>,
    pub start_time: Option<DateTime<Utc>>,
    pub finish_time: Option<DateTime<Utc>>,
    pub token_count: i32,
    pub generation_group_id: Option<String>,
    pub parent_group_id: Option<String>,
    pub tool_calls_json: Option<String>,
}

impl From<message::Model> for Message {
    fn from(model: message::Model) -> Self {
        Self {
            id: model.id,
            parent_id: model.parent_id,
            conversation_id: model.conversation_id,
            message_type: model.message_type,
            content: model.content,
            llm_model_id: model.llm_model_id,
            llm_model_name: model.llm_model_name,
            created_time: model
                .created_time
                .map(|dt| dt.naive_utc().and_utc())
                .unwrap_or_else(Utc::now),
            start_time: model.start_time.map(|dt| dt.naive_utc().and_utc()),
            finish_time: model.finish_time.map(|dt| dt.naive_utc().and_utc()),
            token_count: model.token_count,
            generation_group_id: model.generation_group_id,
            parent_group_id: model.parent_group_id,
            tool_calls_json: model.tool_calls_json,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageDetail {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub conversation_id: i64,
    pub message_type: String,
    pub content: String,
    pub llm_model_id: Option<i64>,
    pub created_time: DateTime<Utc>,
    pub start_time: Option<DateTime<Utc>>,
    pub finish_time: Option<DateTime<Utc>>,
    pub token_count: i32,
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

impl From<message_attachment::Model> for MessageAttachment {
    fn from(model: message_attachment::Model) -> Self {
        Self {
            id: model.id,
            message_id: model.message_id,
            attachment_type: AttachmentType::try_from(model.attachment_type)
                .unwrap_or(AttachmentType::Text),
            attachment_url: model.attachment_url,
            attachment_content: model.attachment_content,
            attachment_hash: model.attachment_hash,
            use_vector: model.use_vector,
            token_count: model.token_count,
        }
    }
}

// ============ Repository Trait ============
pub trait Repository<T> {
    fn create(&self, item: &T) -> Result<T, AppError>;
    fn read(&self, id: i64) -> Result<Option<T>, AppError>;
    fn update(&self, item: &T) -> Result<(), AppError>;
    fn delete(&self, id: i64) -> Result<(), AppError>;
}

// ============ ConversationRepository ============
pub struct ConversationRepository {
    conn: DatabaseConnection,
}

impl ConversationRepository {
    #[instrument(level = "debug", skip(conn))]
    pub fn new(conn: DatabaseConnection) -> Self {
        ConversationRepository { conn }
    }

    // Helper method to run async code in correct runtime context
    fn with_runtime<F, Fut, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(DatabaseConnection) -> Fut,
        Fut: std::future::Future<Output = Result<T, DbErr>>,
    {
        let conn = self.conn.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                tokio::task::block_in_place(|| handle.block_on(f(conn))).map_err(AppError::from)
            }
            Err(_) => {
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    AppError::from(format!("Failed to create Tokio runtime: {}", e))
                })?;
                rt.block_on(f(conn)).map_err(AppError::from)
            }
        }
    }

    #[instrument(level = "debug", skip(self), fields(page = page, per_page = per_page))]
    pub fn list(&self, page: u32, per_page: u32) -> Result<Vec<Conversation>, AppError> {
        let offset = (page - 1) * per_page;
        let per_page = per_page as u64;
        let offset = offset as u64;

        let models = self.with_runtime(|conn| async move {
            conversation::Entity::find()
                .order_by_desc(conversation::Column::CreatedTime)
                .limit(per_page)
                .offset(offset)
                .all(&conn)
                .await
        })?;

        let conversations: Vec<Conversation> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = conversations.len(), "Listed conversations");
        Ok(conversations)
    }

    #[instrument(level = "debug", skip(self), fields(origin_assistant_id, new_assistant_id = ?assistant_id))]
    pub fn update_assistant_id(
        &self,
        origin_assistant_id: i64,
        assistant_id: Option<i64>,
    ) -> Result<(), AppError> {
        self.with_runtime(|conn| async move {
            conversation::Entity::update_many()
                .col_expr(conversation::Column::AssistantId, Expr::value(assistant_id))
                .filter(conversation::Column::AssistantId.eq(origin_assistant_id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated assistant_id");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, conversation), fields(id = conversation.id, name = %conversation.name))]
    pub fn update_name(&self, conversation: &Conversation) -> Result<(), AppError> {
        let id = conversation.id;
        let name = conversation.name.clone();

        self.with_runtime(|conn| async move {
            conversation::Entity::update_many()
                .col_expr(conversation::Column::Name, Expr::value(name))
                .filter(conversation::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated conversation name");
        Ok(())
    }
}

impl Repository<Conversation> for ConversationRepository {
    #[instrument(level = "debug", skip(self, conversation), fields(name = %conversation.name))]
    fn create(&self, conversation: &Conversation) -> Result<Conversation, AppError> {
        let name = conversation.name.clone();
        let assistant_id = conversation.assistant_id;
        let created_time = conversation.created_time;

        let model = self.with_runtime(|conn| async move {
            let active_model = conversation::ActiveModel {
                id: ActiveValue::NotSet,
                name: Set(name),
                assistant_id: Set(assistant_id),
                created_time: Set(Some(created_time.into())),
            };
            active_model.insert(&conn).await
        })?;

        debug!(conversation_id = model.id, "Conversation inserted");
        Ok(model.into())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn read(&self, id: i64) -> Result<Option<Conversation>, AppError> {
        let result = self.with_runtime(|conn| async move {
            conversation::Entity::find_by_id(id).one(&conn).await
        })?;

        let conversation = result.map(|m| m.into());
        debug!(found = conversation.is_some(), "Fetched conversation");
        Ok(conversation)
    }

    #[instrument(level = "debug", skip(self, conversation), fields(id = conversation.id))]
    fn update(&self, conversation: &Conversation) -> Result<(), AppError> {
        let id = conversation.id;
        let name = conversation.name.clone();
        let assistant_id = conversation.assistant_id;

        self.with_runtime(|conn| async move {
            conversation::Entity::update_many()
                .col_expr(conversation::Column::Name, Expr::value(name))
                .col_expr(conversation::Column::AssistantId, Expr::value(assistant_id))
                .filter(conversation::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated conversation");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn delete(&self, id: i64) -> Result<(), AppError> {
        self.with_runtime(|conn| async move {
            conversation::Entity::delete_by_id(id).exec(&conn).await?;
            Ok(())
        })?;

        debug!("Deleted conversation");
        Ok(())
    }
}

// ============ MessageRepository ============
pub struct MessageRepository {
    conn: DatabaseConnection,
}

impl MessageRepository {
    #[instrument(level = "debug", skip(conn))]
    pub fn new(conn: DatabaseConnection) -> Self {
        MessageRepository { conn }
    }

    // Helper method to run async code in correct runtime context
    fn with_runtime<F, Fut, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(DatabaseConnection) -> Fut,
        Fut: std::future::Future<Output = Result<T, DbErr>>,
    {
        let conn = self.conn.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                tokio::task::block_in_place(|| handle.block_on(f(conn))).map_err(AppError::from)
            }
            Err(_) => {
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    AppError::from(format!("Failed to create Tokio runtime: {}", e))
                })?;
                rt.block_on(f(conn)).map_err(AppError::from)
            }
        }
    }

    #[instrument(level = "debug", skip(self), fields(conversation_id = conversation_id))]
    pub fn list_by_conversation_id(
        &self,
        conversation_id: i64,
    ) -> Result<Vec<(Message, Option<MessageAttachment>)>, AppError> {
        // This requires a custom SQL query with LEFT JOIN, similar to the original implementation
        // For now, we'll fetch messages and attachments separately
        let messages = self.with_runtime(|conn| async move {
            message::Entity::find()
                .filter(message::Column::ConversationId.eq(conversation_id))
                .order_by_asc(message::Column::CreatedTime)
                .all(&conn)
                .await
        })?;

        let message_ids: Vec<i64> = messages.iter().map(|m| m.id).collect();

        let attachments = if !message_ids.is_empty() {
            self.with_runtime(|conn| async move {
                message_attachment::Entity::find()
                    .filter(message_attachment::Column::MessageId.is_in(message_ids))
                    .all(&conn)
                    .await
            })?
        } else {
            vec![]
        };

        // Create a map of message_id -> attachments
        let mut attachment_map: std::collections::HashMap<i64, Vec<MessageAttachment>> =
            std::collections::HashMap::new();
        for attachment_model in attachments {
            let attachment: MessageAttachment = attachment_model.into();
            attachment_map.entry(attachment.message_id).or_insert_with(Vec::new).push(attachment);
        }

        // Combine messages with their first attachment (if any)
        let result: Vec<(Message, Option<MessageAttachment>)> = messages
            .into_iter()
            .map(|msg_model| {
                let message: Message = msg_model.into();
                let first_attachment = attachment_map
                    .get(&message.id)
                    .and_then(|attachments| attachments.first().cloned());
                (message, first_attachment)
            })
            .collect();

        debug!(count = result.len(), "Listed messages by conversation_id");
        Ok(result)
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    pub fn update_finish_time(&self, id: i64) -> Result<(), AppError> {
        let now = Utc::now();

        self.with_runtime(|conn| async move {
            message::Entity::update_many()
                .col_expr(
                    message::Column::FinishTime,
                    Expr::value(Some::<ChronoDateTimeUtc>(now.into())),
                )
                .filter(message::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated message finish_time");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, content), fields(id = id, content_len = content.len()))]
    pub fn update_content(&self, id: i64, content: &str) -> Result<(), AppError> {
        let content = content.to_string();

        self.with_runtime(|conn| async move {
            message::Entity::update_many()
                .col_expr(message::Column::Content, Expr::value(content))
                .filter(message::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated message content");
        Ok(())
    }
}

impl Repository<Message> for MessageRepository {
    #[instrument(level = "debug", skip(self, message), fields(conversation_id = message.conversation_id, message_type = %message.message_type))]
    fn create(&self, message: &Message) -> Result<Message, AppError> {
        let parent_id = message.parent_id;
        let conversation_id = message.conversation_id;
        let message_type = message.message_type.clone();
        let content = message.content.clone();
        let llm_model_id = message.llm_model_id;
        let llm_model_name = message.llm_model_name.clone();
        let created_time = message.created_time;
        let start_time = message.start_time;
        let finish_time = message.finish_time;
        let token_count = message.token_count;
        let generation_group_id = message.generation_group_id.clone();
        let parent_group_id = message.parent_group_id.clone();
        let tool_calls_json = message.tool_calls_json.clone();

        let model = self.with_runtime(|conn| async move {
            let active_model = message::ActiveModel {
                id: ActiveValue::NotSet,
                parent_id: Set(parent_id),
                conversation_id: Set(conversation_id),
                message_type: Set(message_type),
                content: Set(content),
                llm_model_id: Set(llm_model_id),
                llm_model_name: Set(llm_model_name),
                created_time: Set(Some(created_time.into())),
                start_time: Set(start_time.map(|dt| dt.into())),
                finish_time: Set(finish_time.map(|dt| dt.into())),
                token_count: Set(token_count),
                generation_group_id: Set(generation_group_id),
                parent_group_id: Set(parent_group_id),
                tool_calls_json: Set(tool_calls_json),
            };
            active_model.insert(&conn).await
        })?;

        debug!(message_id = model.id, "Message inserted");
        Ok(model.into())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn read(&self, id: i64) -> Result<Option<Message>, AppError> {
        let result = self
            .with_runtime(|conn| async move { message::Entity::find_by_id(id).one(&conn).await })?;

        let message = result.map(|m| m.into());
        debug!(found = message.is_some(), "Fetched message");
        Ok(message)
    }

    #[instrument(level = "debug", skip(self, message), fields(id = message.id))]
    fn update(&self, message: &Message) -> Result<(), AppError> {
        let id = message.id;
        let conversation_id = message.conversation_id;
        let message_type = message.message_type.clone();
        let content = message.content.clone();
        let llm_model_id = message.llm_model_id;
        let llm_model_name = message.llm_model_name.clone();
        let token_count = message.token_count;
        let tool_calls_json = message.tool_calls_json.clone();

        self.with_runtime(|conn| async move {
            message::Entity::update_many()
                .col_expr(message::Column::ConversationId, Expr::value(conversation_id))
                .col_expr(message::Column::MessageType, Expr::value(message_type))
                .col_expr(message::Column::Content, Expr::value(content))
                .col_expr(message::Column::LlmModelId, Expr::value(llm_model_id))
                .col_expr(message::Column::LlmModelName, Expr::value(llm_model_name))
                .col_expr(message::Column::TokenCount, Expr::value(token_count))
                .col_expr(message::Column::ToolCallsJson, Expr::value(tool_calls_json))
                .filter(message::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated message");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn delete(&self, id: i64) -> Result<(), AppError> {
        self.with_runtime(|conn| async move {
            message::Entity::delete_by_id(id).exec(&conn).await?;
            Ok(())
        })?;

        debug!("Deleted message");
        Ok(())
    }
}

// ============ MessageAttachmentRepository ============
pub struct MessageAttachmentRepository {
    conn: DatabaseConnection,
}

impl MessageAttachmentRepository {
    #[instrument(level = "debug", skip(conn))]
    pub fn new(conn: DatabaseConnection) -> Self {
        MessageAttachmentRepository { conn }
    }

    // Helper method to run async code in correct runtime context
    fn with_runtime<F, Fut, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(DatabaseConnection) -> Fut,
        Fut: std::future::Future<Output = Result<T, DbErr>>,
    {
        let conn = self.conn.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                tokio::task::block_in_place(|| handle.block_on(f(conn))).map_err(AppError::from)
            }
            Err(_) => {
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    AppError::from(format!("Failed to create Tokio runtime: {}", e))
                })?;
                rt.block_on(f(conn)).map_err(AppError::from)
            }
        }
    }

    #[instrument(level = "debug", skip(self, id_list), fields(id_count = id_list.len()))]
    pub fn list_by_id(&self, id_list: &Vec<i64>) -> Result<Vec<MessageAttachment>, AppError> {
        if id_list.is_empty() {
            return Ok(vec![]);
        }

        let id_list = id_list.clone();

        let models = self.with_runtime(|conn| async move {
            message_attachment::Entity::find()
                .filter(message_attachment::Column::Id.is_in(id_list))
                .all(&conn)
                .await
        })?;

        let attachments: Vec<MessageAttachment> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = attachments.len(), "Listed attachments by id");
        Ok(attachments)
    }

    #[instrument(level = "debug", skip(self, attachment_hash), fields(attachment_hash = %attachment_hash))]
    pub fn read_by_attachment_hash(
        &self,
        attachment_hash: &str,
    ) -> Result<Option<MessageAttachment>, AppError> {
        let attachment_hash = attachment_hash.to_string();

        let result = self.with_runtime(|conn| async move {
            message_attachment::Entity::find()
                .filter(message_attachment::Column::AttachmentHash.eq(attachment_hash))
                .one(&conn)
                .await
        })?;

        let attachment = result.map(|m| m.into());
        debug!(found = attachment.is_some(), "Fetched attachment by hash");
        Ok(attachment)
    }
}

impl Repository<MessageAttachment> for MessageAttachmentRepository {
    #[instrument(level = "debug", skip(self, attachment), fields(message_id = attachment.message_id, attachment_type = ?(attachment.attachment_type as i64)))]
    fn create(&self, attachment: &MessageAttachment) -> Result<MessageAttachment, AppError> {
        let message_id = attachment.message_id;
        let attachment_type = attachment.attachment_type as i64;
        let attachment_url = attachment.attachment_url.clone();
        let attachment_content = attachment.attachment_content.clone();
        let attachment_hash = attachment.attachment_hash.clone();
        let use_vector = attachment.use_vector;
        let token_count = attachment.token_count;

        let model = self.with_runtime(|conn| async move {
            let active_model = message_attachment::ActiveModel {
                id: ActiveValue::NotSet,
                message_id: Set(message_id),
                attachment_type: Set(attachment_type),
                attachment_url: Set(attachment_url),
                attachment_content: Set(attachment_content),
                attachment_hash: Set(attachment_hash),
                use_vector: Set(use_vector),
                token_count: Set(token_count),
            };
            active_model.insert(&conn).await
        })?;

        debug!(attachment_id = model.id, "Message attachment inserted");
        Ok(model.into())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn read(&self, id: i64) -> Result<Option<MessageAttachment>, AppError> {
        let result = self.with_runtime(|conn| async move {
            message_attachment::Entity::find_by_id(id).one(&conn).await
        })?;

        let attachment = result.map(|m| m.into());
        debug!(found = attachment.is_some(), "Fetched message attachment");
        Ok(attachment)
    }

    #[instrument(level = "debug", skip(self, attachment), fields(id = attachment.id))]
    fn update(&self, attachment: &MessageAttachment) -> Result<(), AppError> {
        let id = attachment.id;
        let message_id = attachment.message_id;

        self.with_runtime(|conn| async move {
            message_attachment::Entity::update_many()
                .col_expr(message_attachment::Column::MessageId, Expr::value(message_id))
                .filter(message_attachment::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated message attachment");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    fn delete(&self, id: i64) -> Result<(), AppError> {
        self.with_runtime(|conn| async move {
            message_attachment::Entity::delete_by_id(id).exec(&conn).await?;
            Ok(())
        })?;

        debug!("Deleted message attachment");
        Ok(())
    }
}

// ============ ConversationDatabase ============
pub struct ConversationDatabase {
    db_path: PathBuf,
    conn: DatabaseConnection,
}

impl ConversationDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "conversation.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, AppError> {
        let db_path = get_db_path(app_handle, "conversation.db")
            .map_err(|e| AppError::from(format!("Failed to get db path: {}", e)))?;
        let mut url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());

        // 尝试远程存储
        if let Some(ds_state) = app_handle.try_state::<crate::DataStorageState>() {
            let flat = ds_state.flat.blocking_lock();
            if let Some((dsn, _)) = build_remote_dsn(&flat) { url = dsn; }
        }

        // Create a new Tokio runtime if we're not in one. If already in a runtime,
        // use block_in_place to safely block without panicking.
        let conn = match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| {
                handle
                    .block_on(async { Database::connect(&url).await })
                    .map_err(|e| AppError::from(format!("Failed to connect to database: {}", e)))
            })?,
            Err(_) => {
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    AppError::from(format!("Failed to create Tokio runtime: {}", e))
                })?;
                rt.block_on(async { Database::connect(&url).await })
                    .map_err(|e| AppError::from(format!("Failed to connect to database: {}", e)))?
            }
        };

        debug!("Opened conversation database");
        Ok(ConversationDatabase { db_path, conn })
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn conversation_repo(&self) -> Result<ConversationRepository, AppError> {
        Ok(ConversationRepository::new(self.conn.clone()))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn message_repo(&self) -> Result<MessageRepository, AppError> {
        Ok(MessageRepository::new(self.conn.clone()))
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn attachment_repo(&self) -> Result<MessageAttachmentRepository, AppError> {
        Ok(MessageAttachmentRepository::new(self.conn.clone()))
    }

    // Helper method to run async code in correct runtime context
    fn with_runtime<F, Fut, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(DatabaseConnection) -> Fut,
        Fut: std::future::Future<Output = Result<T, DbErr>>,
    {
        let conn = self.conn.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                tokio::task::block_in_place(|| handle.block_on(f(conn))).map_err(AppError::from)
            }
            Err(_) => {
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    AppError::from(format!("Failed to create Tokio runtime: {}", e))
                })?;
                rt.block_on(f(conn)).map_err(AppError::from)
            }
        }
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn create_tables(&self) -> Result<(), AppError> {
        let backend = self.conn.get_database_backend();
        let schema = Schema::new(backend);
        let sql_conversation = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(conversation::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(conversation::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(conversation::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(conversation::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_message = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(message::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(message::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(message::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(message::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_message_attachment = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(message_attachment::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(message_attachment::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(message_attachment::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(message_attachment::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };

        // Indexes to preserve performance characteristics
        let idx1 =
            "CREATE INDEX IF NOT EXISTS idx_message_conversation_id ON message(conversation_id)";
        let idx2 = "CREATE INDEX IF NOT EXISTS idx_message_conversation_created ON message(conversation_id, created_time)";
        let idx3 = "CREATE INDEX IF NOT EXISTS idx_message_parent_id ON message(parent_id)";
        let idx4 = "CREATE INDEX IF NOT EXISTS idx_message_attachment_message_id ON message_attachment(message_id)";

        self.with_runtime(|conn| async move {
            conn.execute_unprepared(&sql_conversation).await?;
            conn.execute_unprepared(&sql_message).await?;
            conn.execute_unprepared(&sql_message_attachment).await?;
            conn.execute_unprepared(idx1).await?;
            conn.execute_unprepared(idx2).await?;
            conn.execute_unprepared(idx3).await?;
            conn.execute_unprepared(idx4).await?;
            Ok(())
        })?;

        debug!("Created conversation tables and indexes");
        Ok(())
    }

    // Public accessor for raw DatabaseConnection (read-only style usage for export scenarios)
    pub fn get_conn(&self) -> DatabaseConnection {
        self.conn.clone()
    }
}
