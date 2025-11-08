use std::path::PathBuf;

use crate::utils::db_utils::build_remote_dsn;
use chrono::{DateTime, Utc};
use sea_orm::Schema;
use sea_orm::{
    entity::prelude::*, ActiveValue, Database, DatabaseBackend, DatabaseConnection, DbErr,
    QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use tauri::Manager; // for try_state
use tracing::{debug, instrument};

use super::get_db_path;

// ============ SubTaskDefinition Entity ============
pub mod sub_task_definition {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "sub_task_definition")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        #[sea_orm(unique)]
        pub code: String,
        pub description: String,
        pub system_prompt: String,
        pub plugin_source: String, // 'mcp' | 'plugin'
        pub source_id: i64,
        pub is_enabled: bool,
        pub created_time: ChronoDateTimeUtc,
        pub updated_time: ChronoDateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ SubTaskExecution Entity ============
pub mod sub_task_execution {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "sub_task_execution")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub task_definition_id: i64,
        pub task_code: String,
        pub task_name: String,
        pub task_prompt: String,
        pub parent_conversation_id: i64,
        pub parent_message_id: Option<i64>,
        pub status: String, // 'pending' | 'running' | 'success' | 'failed' | 'cancelled'
        pub result_content: Option<String>,
        pub error_message: Option<String>,
        pub mcp_result_json: Option<String>,

        // 消息消费相关字段 (参考 message 表)
        pub llm_model_id: Option<i64>,
        pub llm_model_name: Option<String>,
        pub token_count: i32,
        pub input_token_count: i32,
        pub output_token_count: i32,

        // 时间字段
        pub started_time: Option<ChronoDateTimeUtc>,
        pub finished_time: Option<ChronoDateTimeUtc>,
        pub created_time: ChronoDateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// Legacy structs for backward compatibility
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubTaskDefinition {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub description: String,
    pub system_prompt: String,
    pub plugin_source: String, // 'mcp' | 'plugin'
    pub source_id: i64,
    pub is_enabled: bool,
    pub created_time: DateTime<Utc>,
    pub updated_time: DateTime<Utc>,
}

impl From<sub_task_definition::Model> for SubTaskDefinition {
    fn from(model: sub_task_definition::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            code: model.code,
            description: model.description,
            system_prompt: model.system_prompt,
            plugin_source: model.plugin_source,
            source_id: model.source_id,
            is_enabled: model.is_enabled,
            created_time: model.created_time.into(),
            updated_time: model.updated_time.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubTaskExecution {
    pub id: i64,
    pub task_definition_id: i64,
    pub task_code: String,
    pub task_name: String,
    pub task_prompt: String,
    pub parent_conversation_id: i64,
    pub parent_message_id: Option<i64>,
    pub status: String, // 'pending' | 'running' | 'success' | 'failed' | 'cancelled'
    pub result_content: Option<String>,
    pub error_message: Option<String>,
    pub mcp_result_json: Option<String>,

    // 消息消费相关字段 (参考 message 表)
    pub llm_model_id: Option<i64>,
    pub llm_model_name: Option<String>,
    pub token_count: i32,
    pub input_token_count: i32,
    pub output_token_count: i32,

    // 时间字段
    pub started_time: Option<DateTime<Utc>>,
    pub finished_time: Option<DateTime<Utc>>,
    pub created_time: DateTime<Utc>,
}

impl From<sub_task_execution::Model> for SubTaskExecution {
    fn from(model: sub_task_execution::Model) -> Self {
        Self {
            id: model.id,
            task_definition_id: model.task_definition_id,
            task_code: model.task_code,
            task_name: model.task_name,
            task_prompt: model.task_prompt,
            parent_conversation_id: model.parent_conversation_id,
            parent_message_id: model.parent_message_id,
            status: model.status,
            result_content: model.result_content,
            error_message: model.error_message,
            mcp_result_json: model.mcp_result_json,
            llm_model_id: model.llm_model_id,
            llm_model_name: model.llm_model_name,
            token_count: model.token_count,
            input_token_count: model.input_token_count,
            output_token_count: model.output_token_count,
            started_time: model.started_time.map(|dt| dt.into()),
            finished_time: model.finished_time.map(|dt| dt.into()),
            created_time: model.created_time.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubTaskExecutionSummary {
    pub id: i64,
    pub task_code: String,
    pub task_name: String,
    pub task_prompt: String,
    pub status: String,
    pub created_time: DateTime<Utc>,
    pub token_count: i32,
}

impl SubTaskDefinition {
    // helper: nothing for now
}

pub struct SubTaskDatabase {
    pub conn: DatabaseConnection,
    pub db_path: PathBuf,
}

impl SubTaskDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "conversation.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, DbErr> {
        let db_path = get_db_path(app_handle, "conversation.db").map_err(|e| DbErr::Custom(e))?;
        let mut url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
        if let Some(ds_state) = app_handle.try_state::<crate::DataStorageState>() {
            let flat = ds_state.flat.blocking_lock();
            if let Some((dsn, _)) = build_remote_dsn(&flat) {
                url = dsn;
            }
        }

        let conn = match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| {
                handle.block_on(async { Database::connect(&url).await })
            })?,
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| DbErr::Custom(format!("Failed to create Tokio runtime: {}", e)))?;
                rt.block_on(async { Database::connect(&url).await })?
            }
        };

        debug!("Opened sub task database");
        Ok(SubTaskDatabase { conn, db_path })
    }

    // removed rusqlite connection helper

    // Helper method to run async code in correct runtime context
    fn with_runtime<F, Fut, T>(f: F) -> Result<T, DbErr>
    where
        F: FnOnce(Option<DatabaseConnection>) -> Fut,
        Fut: std::future::Future<Output = Result<T, DbErr>>,
    {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f(None))),
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| DbErr::Custom(format!("Failed to create Tokio runtime: {}", e)))?;
                rt.block_on(f(None))
            }
        }
    }

    // Helper method for instance operations
    fn with_runtime_conn<F, Fut, T>(&self, f: F) -> Result<T, DbErr>
    where
        F: FnOnce(DatabaseConnection) -> Fut,
        Fut: std::future::Future<Output = Result<T, DbErr>>,
    {
        let conn = self.conn.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f(conn))),
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| DbErr::Custom(format!("Failed to create Tokio runtime: {}", e)))?;
                rt.block_on(f(conn))
            }
        }
    }

    // Definition methods
    #[instrument(level = "debug", skip(self, plugin_source, source_id, is_enabled), fields(plugin_source = plugin_source.unwrap_or("*"), source_id = source_id.map(|v| v.to_string()).unwrap_or_else(|| "*".into()), is_enabled = is_enabled.map(|v| v.to_string()).unwrap_or_else(|| "*".into())))]
    pub fn list_definitions_by_source(
        &self,
        plugin_source: Option<&str>,
        source_id: Option<i64>,
        is_enabled: Option<bool>,
    ) -> Result<Vec<SubTaskDefinition>, DbErr> {
        let plugin_source = plugin_source.map(|s| s.to_string());

        let defs = self.with_runtime_conn(|conn| async move {
            let mut query = sub_task_definition::Entity::find();

            if let Some(source) = plugin_source {
                query = query.filter(sub_task_definition::Column::PluginSource.eq(source));
            }

            if let Some(sid) = source_id {
                query = query.filter(sub_task_definition::Column::SourceId.eq(sid));
            }

            if let Some(enabled) = is_enabled {
                query = query.filter(sub_task_definition::Column::IsEnabled.eq(enabled));
            }

            let models =
                query.order_by_desc(sub_task_definition::Column::CreatedTime).all(&conn).await?;

            Ok(models.into_iter().map(|m| m.into()).collect::<Vec<SubTaskDefinition>>())
        })?;

        debug!(count = defs.len(), "Fetched sub task definitions");
        Ok(defs)
    }

    #[instrument(level = "debug", skip(self), fields(code))]
    pub fn find_definition_by_code(&self, code: &str) -> Result<Option<SubTaskDefinition>, DbErr> {
        let code = code.to_string();

        let def = self.with_runtime_conn(|conn| async move {
            sub_task_definition::Entity::find()
                .filter(sub_task_definition::Column::Code.eq(code))
                .one(&conn)
                .await
        })?;

        let result = def.map(|m| m.into());
        debug!(found = result.is_some(), "Fetched definition by code");
        Ok(result)
    }

    #[instrument(level = "debug", skip(self), fields(plugin_source, source_id, code))]
    pub fn find_definition_by_source_and_code(
        &self,
        plugin_source: &str,
        source_id: i64,
        code: &str,
    ) -> Result<Option<SubTaskDefinition>, DbErr> {
        let plugin_source = plugin_source.to_string();
        let code = code.to_string();

        let def = self.with_runtime_conn(|conn| async move {
            sub_task_definition::Entity::find()
                .filter(sub_task_definition::Column::PluginSource.eq(plugin_source))
                .filter(sub_task_definition::Column::SourceId.eq(source_id))
                .filter(sub_task_definition::Column::Code.eq(code))
                .one(&conn)
                .await
        })?;

        let result = def.map(|m| m.into());
        debug!(found = result.is_some(), "Fetched definition by source & code");
        Ok(result)
    }

    #[instrument(level = "debug", skip(self, definition), fields(code = %definition.code, source_id = definition.source_id, plugin_source = %definition.plugin_source))]
    pub fn upsert_sub_task_definition(
        &self,
        definition: &SubTaskDefinition,
    ) -> Result<SubTaskDefinition, DbErr> {
        // First try to find existing definition by source and code
        if let Some(existing) = self.find_definition_by_source_and_code(
            &definition.plugin_source,
            definition.source_id,
            &definition.code,
        )? {
            // Update existing definition
            let updated_definition = SubTaskDefinition {
                id: existing.id,
                name: definition.name.clone(),
                code: definition.code.clone(),
                description: definition.description.clone(),
                system_prompt: definition.system_prompt.clone(),
                plugin_source: existing.plugin_source,
                source_id: existing.source_id,
                is_enabled: definition.is_enabled,
                created_time: existing.created_time,
                updated_time: Utc::now(),
            };

            self.update_sub_task_definition(&updated_definition)?;
            debug!(id = updated_definition.id, "Updated sub task definition");
            Ok(updated_definition)
        } else {
            // Create new definition
            let new_definition = SubTaskDefinition {
                id: 0,
                name: definition.name.clone(),
                code: definition.code.clone(),
                description: definition.description.clone(),
                system_prompt: definition.system_prompt.clone(),
                plugin_source: definition.plugin_source.clone(),
                source_id: definition.source_id,
                is_enabled: definition.is_enabled,
                created_time: Utc::now(),
                updated_time: Utc::now(),
            };

            let inserted = self.create_sub_task_definition(&new_definition)?;
            debug!(id = inserted.id, "Inserted sub task definition");
            Ok(inserted)
        }
    }

    #[instrument(level = "debug", skip(self), fields(id, is_enabled))]
    pub fn update_definition_enabled_status(&self, id: i64, is_enabled: bool) -> Result<(), DbErr> {
        let now = Utc::now();

        self.with_runtime_conn(|conn| async move {
            sub_task_definition::Entity::update_many()
                .col_expr(sub_task_definition::Column::IsEnabled, Expr::value(is_enabled))
                .col_expr(sub_task_definition::Column::UpdatedTime, Expr::value(now))
                .filter(sub_task_definition::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated definition enabled status");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, definition), fields(code = %definition.code, source_id = definition.source_id))]
    pub fn create_sub_task_definition(
        &self,
        definition: &SubTaskDefinition,
    ) -> Result<SubTaskDefinition, DbErr> {
        let name = definition.name.clone();
        let code = definition.code.clone();
        let description = definition.description.clone();
        let system_prompt = definition.system_prompt.clone();
        let plugin_source = definition.plugin_source.clone();
        let source_id = definition.source_id;
        let is_enabled = definition.is_enabled;
        let created_time = definition.created_time;
        let updated_time = definition.updated_time;

        let model = self.with_runtime_conn(|conn| async move {
            let active_model = sub_task_definition::ActiveModel {
                id: ActiveValue::NotSet,
                name: Set(name),
                code: Set(code),
                description: Set(description),
                system_prompt: Set(system_prompt),
                plugin_source: Set(plugin_source),
                source_id: Set(source_id),
                is_enabled: Set(is_enabled),
                created_time: Set(created_time.into()),
                updated_time: Set(updated_time.into()),
            };
            active_model.insert(&conn).await
        })?;

        let id = model.id;
        debug!(id, "Inserted sub task definition row");
        Ok(model.into())
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn read_sub_task_definition(&self, id: i64) -> Result<Option<SubTaskDefinition>, DbErr> {
        let def = self.with_runtime_conn(|conn| async move {
            sub_task_definition::Entity::find_by_id(id).one(&conn).await
        })?;

        let result = def.map(|m| m.into());
        debug!(found = result.is_some(), "Read definition by id");
        Ok(result)
    }

    #[instrument(level = "debug", skip(self, definition), fields(id = definition.id))]
    pub fn update_sub_task_definition(&self, definition: &SubTaskDefinition) -> Result<(), DbErr> {
        let id = definition.id;
        let name = definition.name.clone();
        let description = definition.description.clone();
        let system_prompt = definition.system_prompt.clone();
        let is_enabled = definition.is_enabled;
        let now = Utc::now();

        self.with_runtime_conn(|conn| async move {
            sub_task_definition::Entity::update_many()
                .col_expr(sub_task_definition::Column::Name, Expr::value(name))
                .col_expr(sub_task_definition::Column::Description, Expr::value(description))
                .col_expr(sub_task_definition::Column::SystemPrompt, Expr::value(system_prompt))
                .col_expr(sub_task_definition::Column::IsEnabled, Expr::value(is_enabled))
                .col_expr(sub_task_definition::Column::UpdatedTime, Expr::value(now))
                .filter(sub_task_definition::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated sub task definition row");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn delete_sub_task_definition_row(&self, id: i64) -> Result<(), DbErr> {
        self.with_runtime_conn(|conn| async move {
            sub_task_definition::Entity::delete_many()
                .filter(sub_task_definition::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Deleted sub task definition row");
        Ok(())
    }

    // Execution methods
    #[instrument(level = "debug", skip(self, status), fields(parent_conversation_id, parent_message_id, status = status.unwrap_or("*"), page, page_size))]
    pub fn list_executions_by_conversation(
        &self,
        parent_conversation_id: i64,
        parent_message_id: Option<i64>,
        status: Option<&str>,
        page: u32,
        page_size: u32,
    ) -> Result<Vec<SubTaskExecutionSummary>, DbErr> {
        let offset = (page - 1) * page_size;
        let status = status.map(|s| s.to_string());

        let summaries = self.with_runtime_conn(|conn| async move {
            let mut query = sub_task_execution::Entity::find().filter(
                sub_task_execution::Column::ParentConversationId.eq(parent_conversation_id),
            );

            // Handle parent_message_id filter
            match parent_message_id {
                Some(msg_id) => {
                    query = query.filter(sub_task_execution::Column::ParentMessageId.eq(msg_id));
                }
                None => {
                    // If you want to filter for NULL parent_message_id, uncomment:
                    // query = query.filter(sub_task_execution::Column::ParentMessageId.is_null());
                }
            }

            if let Some(st) = status {
                query = query.filter(sub_task_execution::Column::Status.eq(st));
            }

            // Apply ordering then manual offset & limit via query builder
            use sea_orm::QuerySelect;
            query = query.order_by_desc(sub_task_execution::Column::CreatedTime);
            query = query.offset(offset as u64);
            query = query.limit(page_size as u64);
            let models = query.all(&conn).await?;

            let summaries: Vec<SubTaskExecutionSummary> = models
                .into_iter()
                .map(|m| SubTaskExecutionSummary {
                    id: m.id,
                    task_code: m.task_code,
                    task_name: m.task_name,
                    task_prompt: m.task_prompt,
                    status: m.status,
                    created_time: m.created_time.into(),
                    token_count: m.token_count,
                })
                .collect();

            Ok(summaries)
        })?;

        debug!(count = summaries.len(), "Fetched executions by conversation");
        Ok(summaries)
    }

    #[instrument(level = "debug", skip(self, started_time), fields(id, status))]
    pub fn update_execution_status(
        &self,
        id: i64,
        status: &str,
        started_time: Option<DateTime<Utc>>,
    ) -> Result<(), DbErr> {
        let status = status.to_string();

        self.with_runtime_conn(|conn| async move {
            let mut update = sub_task_execution::Entity::update_many()
                .col_expr(sub_task_execution::Column::Status, Expr::value(status));

            if let Some(time) = started_time {
                update = update.col_expr(
                    sub_task_execution::Column::StartedTime,
                    Expr::value(Option::<ChronoDateTimeUtc>::Some(time.into())),
                );
            }

            update.filter(sub_task_execution::Column::Id.eq(id)).exec(&conn).await?;
            Ok(())
        })?;

        debug!("Updated execution status");
        Ok(())
    }

    #[instrument(
        level = "debug",
        skip(self, result_content, error_message, token_stats, finished_time),
        fields(id, status)
    )]
    pub fn update_execution_result(
        &self,
        id: i64,
        status: &str,
        result_content: Option<&str>,
        error_message: Option<&str>,
        token_stats: Option<(i32, i32, i32)>,
        finished_time: Option<DateTime<Utc>>,
    ) -> Result<(), DbErr> {
        let status = status.to_string();
        let result_content = result_content.map(|s| s.to_string());
        let error_message = error_message.map(|s| s.to_string());
        let (token_count, input_tokens, output_tokens) = token_stats.unwrap_or((0, 0, 0));

        self.with_runtime_conn(|conn| async move {
            sub_task_execution::Entity::update_many()
                .col_expr(sub_task_execution::Column::Status, Expr::value(status))
                .col_expr(sub_task_execution::Column::ResultContent, Expr::value(result_content))
                .col_expr(sub_task_execution::Column::ErrorMessage, Expr::value(error_message))
                .col_expr(sub_task_execution::Column::TokenCount, Expr::value(token_count))
                .col_expr(sub_task_execution::Column::InputTokenCount, Expr::value(input_tokens))
                .col_expr(sub_task_execution::Column::OutputTokenCount, Expr::value(output_tokens))
                .col_expr(
                    sub_task_execution::Column::FinishedTime,
                    Expr::value(finished_time.map(|dt| -> ChronoDateTimeUtc { dt.into() })),
                )
                .filter(sub_task_execution::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!(token_count, input_tokens, output_tokens, "Updated execution result");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, source_id), fields(parent_conversation_id, source_id = source_id.map(|v| v.to_string()).unwrap_or_else(|| "*".into())))]
    pub fn list_executions_by_source_filter(
        &self,
        parent_conversation_id: i64,
        source_id: Option<i64>,
    ) -> Result<Vec<SubTaskExecutionSummary>, DbErr> {
        // 使用 SeaORM 组合查询
        let summaries = self.with_runtime_conn(|conn| async move {
            use sub_task_definition as stdef;
            use sub_task_execution as ste;

            let mut exec_query = ste::Entity::find()
                .filter(ste::Column::ParentConversationId.eq(parent_conversation_id));

            if let Some(sid) = source_id {
                // 先查出符合 source_id 的定义 id 列表
                let def_ids: Vec<i64> = stdef::Entity::find()
                    .filter(stdef::Column::SourceId.eq(sid))
                    .all(&conn)
                    .await?
                    .into_iter()
                    .map(|m| m.id)
                    .collect();

                if !def_ids.is_empty() {
                    exec_query = exec_query.filter(ste::Column::TaskDefinitionId.is_in(def_ids));
                } else {
                    // 没有匹配的定义，直接返回空
                    return Ok::<Vec<SubTaskExecutionSummary>, DbErr>(vec![]);
                }
            }

            let models = exec_query.order_by_desc(ste::Column::CreatedTime).all(&conn).await?;

            let result = models
                .into_iter()
                .map(|m| SubTaskExecutionSummary {
                    id: m.id,
                    task_code: m.task_code,
                    task_name: m.task_name,
                    task_prompt: m.task_prompt,
                    status: m.status,
                    created_time: m.created_time.into(),
                    token_count: m.token_count,
                })
                .collect();
            Ok(result)
        })?;

        debug!(count = summaries.len(), "Fetched executions by source filter");
        Ok(summaries)
    }

    #[instrument(level = "debug", skip(self, execution), fields(task_code = %execution.task_code, parent_conversation_id = execution.parent_conversation_id))]
    pub fn create_sub_task_execution(
        &self,
        execution: &SubTaskExecution,
    ) -> Result<SubTaskExecution, DbErr> {
        let task_definition_id = execution.task_definition_id;
        let task_code = execution.task_code.clone();
        let task_name = execution.task_name.clone();
        let task_prompt = execution.task_prompt.clone();
        let parent_conversation_id = execution.parent_conversation_id;
        let parent_message_id = execution.parent_message_id;
        let status = execution.status.clone();
        let result_content = execution.result_content.clone();
        let error_message = execution.error_message.clone();
        let mcp_result_json = execution.mcp_result_json.clone();
        let llm_model_id = execution.llm_model_id;
        let llm_model_name = execution.llm_model_name.clone();
        let token_count = execution.token_count;
        let input_token_count = execution.input_token_count;
        let output_token_count = execution.output_token_count;
        let started_time = execution.started_time.map(|dt| dt.into());
        let finished_time = execution.finished_time.map(|dt| dt.into());
        let created_time = execution.created_time;

        let model = self.with_runtime_conn(|conn| async move {
            let active_model = sub_task_execution::ActiveModel {
                id: ActiveValue::NotSet,
                task_definition_id: Set(task_definition_id),
                task_code: Set(task_code),
                task_name: Set(task_name),
                task_prompt: Set(task_prompt),
                parent_conversation_id: Set(parent_conversation_id),
                parent_message_id: Set(parent_message_id),
                status: Set(status),
                result_content: Set(result_content),
                error_message: Set(error_message),
                mcp_result_json: Set(mcp_result_json),
                llm_model_id: Set(llm_model_id),
                llm_model_name: Set(llm_model_name),
                token_count: Set(token_count),
                input_token_count: Set(input_token_count),
                output_token_count: Set(output_token_count),
                started_time: Set(started_time),
                finished_time: Set(finished_time),
                created_time: Set(created_time.into()),
            };
            active_model.insert(&conn).await
        })?;

        let id = model.id;
        debug!(id, "Inserted sub task execution row");
        Ok(model.into())
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn read_sub_task_execution(&self, id: i64) -> Result<Option<SubTaskExecution>, DbErr> {
        let exec = self.with_runtime_conn(|conn| async move {
            sub_task_execution::Entity::find_by_id(id).one(&conn).await
        })?;

        let result = exec.map(|m| m.into());
        debug!(found = result.is_some(), "Read execution by id");
        Ok(result)
    }

    #[instrument(level = "debug", skip(self, execution), fields(id = execution.id))]
    pub fn update_sub_task_execution(&self, execution: &SubTaskExecution) -> Result<(), DbErr> {
        let id = execution.id;
        let task_prompt = execution.task_prompt.clone();
        let status = execution.status.clone();
        let result_content = execution.result_content.clone();
        let error_message = execution.error_message.clone();
        let mcp_result_json = execution.mcp_result_json.clone();
        let token_count = execution.token_count;
        let input_token_count = execution.input_token_count;
        let output_token_count = execution.output_token_count;
        let finished_time: Option<String> = execution.finished_time.map(|dt| dt.to_rfc3339());

        self.with_runtime_conn(|conn| async move {
            sub_task_execution::Entity::update_many()
                .col_expr(sub_task_execution::Column::TaskPrompt, Expr::value(task_prompt))
                .col_expr(sub_task_execution::Column::Status, Expr::value(status))
                .col_expr(sub_task_execution::Column::ResultContent, Expr::value(result_content))
                .col_expr(sub_task_execution::Column::ErrorMessage, Expr::value(error_message))
                .col_expr(sub_task_execution::Column::McpResultJson, Expr::value(mcp_result_json))
                .col_expr(sub_task_execution::Column::TokenCount, Expr::value(token_count))
                .col_expr(
                    sub_task_execution::Column::InputTokenCount,
                    Expr::value(input_token_count),
                )
                .col_expr(
                    sub_task_execution::Column::OutputTokenCount,
                    Expr::value(output_token_count),
                )
                .col_expr(sub_task_execution::Column::FinishedTime, Expr::value(finished_time))
                .filter(sub_task_execution::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated sub task execution row");
        Ok(())
    }

    /// Update only the mcp_result_json column for a given subtask execution
    #[instrument(level = "debug", skip(self, mcp_result_json), fields(id))]
    pub fn set_execution_mcp_result_json(
        &self,
        id: i64,
        mcp_result_json: Option<&str>,
    ) -> Result<(), DbErr> {
        let mcp_result_json = mcp_result_json.map(|s| s.to_string());

        self.with_runtime_conn(|conn| async move {
            sub_task_execution::Entity::update_many()
                .col_expr(sub_task_execution::Column::McpResultJson, Expr::value(mcp_result_json))
                .filter(sub_task_execution::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated execution mcp_result_json");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn delete_sub_task_execution_row(&self, id: i64) -> Result<(), DbErr> {
        self.with_runtime_conn(|conn| async move {
            sub_task_execution::Entity::delete_many()
                .filter(sub_task_execution::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Deleted sub task execution row");
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn create_tables(&self) -> Result<(), DbErr> {
        let backend = self.conn.get_database_backend();
        let schema = Schema::new(backend);
        let sql_def = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(sub_task_definition::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(sub_task_definition::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(sub_task_definition::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(sub_task_definition::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_exec = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(sub_task_execution::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(sub_task_execution::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(sub_task_execution::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(sub_task_execution::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };

        self.with_runtime_conn(|conn| async move {
            conn.execute_unprepared(&sql_def).await?;
            conn.execute_unprepared(&sql_exec).await?;
            // Indexes equivalent to previous definitions
            conn.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_sub_task_definition_code ON sub_task_definition(code)").await?;
            conn.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_sub_task_definition_source ON sub_task_definition(plugin_source, source_id)").await?;
            conn.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_sub_task_execution_conversation ON sub_task_execution(parent_conversation_id)").await?;
            conn.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_sub_task_execution_message ON sub_task_execution(parent_message_id)").await?;
            conn.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_sub_task_execution_status ON sub_task_execution(status)").await?;
            Ok(())
        })?;

        debug!("Sub task tables ensured");
        Ok(())
    }
}
