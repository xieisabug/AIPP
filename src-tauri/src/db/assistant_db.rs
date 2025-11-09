use sea_orm::Schema;
use sea_orm::{
    entity::prelude::*, ActiveValue, DatabaseBackend, DatabaseConnection, DbErr,
    QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, instrument};

// ============ Assistant Entity ============
pub mod assistant {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "assistant")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub description: Option<String>,
        pub assistant_type: Option<i64>,
        pub is_addition: bool,
        pub created_time: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ AssistantModel Entity ============
pub mod assistant_model {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "assistant_model")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub assistant_id: i64,
        pub provider_id: i64,
        pub model_code: String,
        pub alias: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ AssistantPrompt Entity ============
pub mod assistant_prompt {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "assistant_prompt")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub assistant_id: i64,
        pub prompt: String,
        pub created_time: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ AssistantModelConfig Entity ============
pub mod assistant_model_config {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "assistant_model_config")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub assistant_id: i64,
        pub assistant_model_id: i64,
        pub name: String,
        pub value: Option<String>,
        pub value_type: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ AssistantPromptParam Entity ============
pub mod assistant_prompt_param {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "assistant_prompt_param")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub assistant_id: i64,
        pub assistant_prompt_id: i64,
        pub param_name: String,
        pub param_type: Option<String>,
        pub param_value: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ AssistantMCPConfig Entity ============
pub mod assistant_mcp_config {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "assistant_mcp_config")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub assistant_id: i64,
        pub mcp_server_id: i64,
        pub is_enabled: bool,
        pub created_time: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ AssistantMCPToolConfig Entity ============
pub mod assistant_mcp_tool_config {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "assistant_mcp_tool_config")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub assistant_id: i64,
        pub mcp_tool_id: i64,
        pub is_enabled: bool,
        pub is_auto_run: bool,
        pub created_time: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// Legacy structs for backward compatibility
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Assistant {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub assistant_type: Option<i64>,
    pub is_addition: bool,
    pub created_time: String,
}

impl From<assistant::Model> for Assistant {
    fn from(model: assistant::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            description: model.description,
            assistant_type: model.assistant_type,
            is_addition: model.is_addition,
            created_time: model.created_time,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantModel {
    pub id: i64,
    pub assistant_id: i64,
    pub provider_id: i64,
    pub model_code: String,
    pub alias: String,
}

impl From<assistant_model::Model> for AssistantModel {
    fn from(model: assistant_model::Model) -> Self {
        Self {
            id: model.id,
            assistant_id: model.assistant_id,
            provider_id: model.provider_id,
            model_code: model.model_code,
            alias: model.alias,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantPrompt {
    pub id: i64,
    pub assistant_id: i64,
    pub prompt: String,
    pub created_time: Option<String>,
}

impl From<assistant_prompt::Model> for AssistantPrompt {
    fn from(model: assistant_prompt::Model) -> Self {
        Self {
            id: model.id,
            assistant_id: model.assistant_id,
            prompt: model.prompt,
            created_time: model.created_time,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantModelConfig {
    pub id: i64,
    pub assistant_id: i64,
    pub assistant_model_id: i64,
    pub name: String,
    pub value: Option<String>,
    pub value_type: String,
}

impl From<assistant_model_config::Model> for AssistantModelConfig {
    fn from(model: assistant_model_config::Model) -> Self {
        Self {
            id: model.id,
            assistant_id: model.assistant_id,
            assistant_model_id: model.assistant_model_id,
            name: model.name,
            value: model.value,
            value_type: model.value_type,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantPromptParam {
    pub id: i64,
    pub assistant_id: i64,
    pub assistant_prompt_id: i64,
    pub param_name: String,
    pub param_type: Option<String>,
    pub param_value: Option<String>,
}

impl From<assistant_prompt_param::Model> for AssistantPromptParam {
    fn from(model: assistant_prompt_param::Model) -> Self {
        Self {
            id: model.id,
            assistant_id: model.assistant_id,
            assistant_prompt_id: model.assistant_prompt_id,
            param_name: model.param_name,
            param_type: model.param_type,
            param_value: model.param_value,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantMCPConfig {
    pub id: i64,
    pub assistant_id: i64,
    pub mcp_server_id: i64,
    pub is_enabled: bool,
}

impl From<assistant_mcp_config::Model> for AssistantMCPConfig {
    fn from(model: assistant_mcp_config::Model) -> Self {
        Self {
            id: model.id,
            assistant_id: model.assistant_id,
            mcp_server_id: model.mcp_server_id,
            is_enabled: model.is_enabled,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantMCPToolConfig {
    pub id: i64,
    pub assistant_id: i64,
    pub mcp_tool_id: i64,
    pub is_enabled: bool,
    pub is_auto_run: bool,
}

impl From<assistant_mcp_tool_config::Model> for AssistantMCPToolConfig {
    fn from(model: assistant_mcp_tool_config::Model) -> Self {
        Self {
            id: model.id,
            assistant_id: model.assistant_id,
            mcp_tool_id: model.mcp_tool_id,
            is_enabled: model.is_enabled,
            is_auto_run: model.is_auto_run,
        }
    }
}

pub struct AssistantDatabase {
    pub conn: DatabaseConnection,
}

impl AssistantDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "assistant.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, DbErr> {
        // 从全局状态获取共享连接，而不是创建新连接
        let conn_arc = crate::db::conn_helper::get_db_conn(app_handle)?;
        let conn = (*conn_arc).clone(); // DatabaseConnection 内部是 Arc，clone 很轻量
        
        debug!("Acquired shared database connection for Assistant");
        Ok(AssistantDatabase { conn })
    }

    // Helper method to run async code in correct runtime context
    fn with_runtime<F, Fut, T>(&self, f: F) -> Result<T, DbErr>
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

    #[instrument(level = "debug", skip(app_handle), err)]
    pub fn create_tables(app_handle: &tauri::AppHandle) -> Result<(), DbErr> {
        let db = Self::new(app_handle)?;
        let backend = db.conn.get_database_backend();
        let schema = Schema::new(backend);
        let sql_assistant = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(assistant::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(assistant::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(assistant::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(assistant::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_assistant_model = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(assistant_model::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(assistant_model::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(assistant_model::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(assistant_model::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_assistant_prompt = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(assistant_prompt::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(assistant_prompt::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(assistant_prompt::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(assistant_prompt::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_assistant_model_config = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(assistant_model_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(assistant_model_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(assistant_model_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(assistant_model_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_assistant_prompt_param = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(assistant_prompt_param::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(assistant_prompt_param::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(assistant_prompt_param::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(assistant_prompt_param::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_assistant_mcp_config = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(assistant_mcp_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(assistant_mcp_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(assistant_mcp_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(assistant_mcp_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_assistant_mcp_tool_config = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(assistant_mcp_tool_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(assistant_mcp_tool_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(assistant_mcp_tool_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(assistant_mcp_tool_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };

        db.with_runtime(|conn| async move {
            conn.execute_unprepared(&sql_assistant).await?;
            conn.execute_unprepared(&sql_assistant_model).await?;
            conn.execute_unprepared(&sql_assistant_prompt).await?;
            conn.execute_unprepared(&sql_assistant_model_config).await?;
            conn.execute_unprepared(&sql_assistant_prompt_param).await?;
            conn.execute_unprepared(&sql_assistant_mcp_config).await?;
            conn.execute_unprepared(&sql_assistant_mcp_tool_config).await?;
            // Composite unique constraints mirrored via unique indexes
            conn.execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_assistant_mcp_cfg_unique ON assistant_mcp_config(assistant_id, mcp_server_id)",
            )
            .await?;
            conn.execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_assistant_mcp_tool_cfg_unique ON assistant_mcp_tool_config(assistant_id, mcp_tool_id)",
            )
            .await?;
            Ok(())
        })?;

        if let Err(err) = db.init_assistant() {
            error!(error = ?err, "init_assistant failed during create_tables");
        } else {
            debug!("assistant default initialization succeeded");
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(name = name, assistant_type = assistant_type))]
    pub fn add_assistant(
        &self,
        name: &str,
        description: &str,
        assistant_type: Option<i64>,
        is_addition: bool,
    ) -> Result<i64, DbErr> {
        let name = name.to_string();
        let description = description.to_string();

        let id = self.with_runtime(|conn| async move {
            let model = assistant::ActiveModel {
                id: ActiveValue::NotSet,
                name: Set(name),
                description: Set(Some(description)),
                assistant_type: Set(assistant_type),
                is_addition: Set(is_addition),
                created_time: ActiveValue::NotSet,
            };
            let result = model.insert(&conn).await?;
            Ok(result.id)
        })?;

        debug!(assistant_id = id, "assistant inserted");
        Ok(id)
    }

    #[instrument(level = "debug", skip(self), fields(id = id, name = name))]
    pub fn update_assistant(&self, id: i64, name: &str, description: &str) -> Result<(), DbErr> {
        let name = name.to_string();
        let description = description.to_string();

        self.with_runtime(|conn| async move {
            assistant::Entity::update_many()
                .col_expr(assistant::Column::Name, Expr::value(name))
                .col_expr(assistant::Column::Description, Expr::value(description))
                .filter(assistant::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("assistant updated");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    pub fn delete_assistant(&self, id: i64) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            assistant::Entity::delete_many()
                .filter(assistant::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("assistant deleted");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn add_assistant_prompt(&self, assistant_id: i64, prompt: &str) -> Result<i64, DbErr> {
        let prompt = prompt.to_string();

        let id = self.with_runtime(|conn| async move {
            let model = assistant_prompt::ActiveModel {
                id: ActiveValue::NotSet,
                assistant_id: Set(assistant_id),
                prompt: Set(prompt),
                created_time: ActiveValue::NotSet,
            };
            let result = model.insert(&conn).await?;
            Ok(result.id)
        })?;

        debug!(assistant_prompt_id = id, "assistant prompt inserted");
        Ok(id)
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    pub fn update_assistant_prompt(&self, id: i64, prompt: &str) -> Result<(), DbErr> {
        let prompt = prompt.to_string();

        self.with_runtime(|conn| async move {
            assistant_prompt::Entity::update_many()
                .col_expr(assistant_prompt::Column::Prompt, Expr::value(prompt))
                .filter(assistant_prompt::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("assistant prompt updated");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn delete_assistant_prompt_by_assistant_id(&self, assistant_id: i64) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            assistant_prompt::Entity::delete_many()
                .filter(assistant_prompt::Column::AssistantId.eq(assistant_id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("assistant prompts deleted by assistant_id");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id, provider_id = provider_id, model_code = model_code))]
    pub fn add_assistant_model(
        &self,
        assistant_id: i64,
        provider_id: i64,
        model_code: &str,
        alias: &str,
    ) -> Result<i64, DbErr> {
        let model_code = model_code.to_string();
        let alias = alias.to_string();

        let id = self.with_runtime(|conn| async move {
            let model = assistant_model::ActiveModel {
                id: ActiveValue::NotSet,
                assistant_id: Set(assistant_id),
                provider_id: Set(provider_id),
                model_code: Set(model_code),
                alias: Set(alias),
            };
            let result = model.insert(&conn).await?;
            Ok(result.id)
        })?;

        debug!(assistant_model_id = id, "assistant model inserted");
        Ok(id)
    }

    #[instrument(level = "debug", skip(self), fields(id = id, provider_id = provider_id, model_code = model_code))]
    pub fn update_assistant_model(
        &self,
        id: i64,
        provider_id: i64,
        model_code: &str,
        alias: &str,
    ) -> Result<(), DbErr> {
        let model_code = model_code.to_string();
        let alias = alias.to_string();

        self.with_runtime(|conn| async move {
            assistant_model::Entity::update_many()
                .col_expr(assistant_model::Column::ModelCode, Expr::value(model_code))
                .col_expr(assistant_model::Column::ProviderId, Expr::value(provider_id))
                .col_expr(assistant_model::Column::Alias, Expr::value(alias))
                .filter(assistant_model::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("assistant model updated");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id, assistant_model_id = assistant_model_id, name = name))]
    pub fn add_assistant_model_config(
        &self,
        assistant_id: i64,
        assistant_model_id: i64,
        name: &str,
        value: &str,
        value_type: &str,
    ) -> Result<i64, DbErr> {
        let name = name.to_string();
        let value = value.to_string();
        let value_type = value_type.to_string();

        let id = self.with_runtime(|conn| async move {
            let model = assistant_model_config::ActiveModel {
                id: ActiveValue::NotSet,
                assistant_id: Set(assistant_id),
                assistant_model_id: Set(assistant_model_id),
                name: Set(name),
                value: Set(Some(value)),
                value_type: Set(value_type),
            };
            let result = model.insert(&conn).await?;
            Ok(result.id)
        })?;

        debug!(assistant_model_config_id = id, "assistant model config inserted");
        Ok(id)
    }

    #[instrument(level = "debug", skip(self), fields(id = id, name = name))]
    pub fn update_assistant_model_config(
        &self,
        id: i64,
        name: &str,
        value: &str,
    ) -> Result<(), DbErr> {
        let name = name.to_string();
        let value = value.to_string();

        self.with_runtime(|conn| async move {
            assistant_model_config::Entity::update_many()
                .col_expr(assistant_model_config::Column::Name, Expr::value(name))
                .col_expr(assistant_model_config::Column::Value, Expr::value(value))
                .filter(assistant_model_config::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("assistant model config updated");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn delete_assistant_model_config_by_assistant_id(
        &self,
        assistant_id: i64,
    ) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            assistant_model_config::Entity::delete_many()
                .filter(assistant_model_config::Column::AssistantId.eq(assistant_id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("assistant model configs deleted by assistant_id");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id, assistant_prompt_id = assistant_prompt_id, param_name = param_name))]
    pub fn add_assistant_prompt_param(
        &self,
        assistant_id: i64,
        assistant_prompt_id: i64,
        param_name: &str,
        param_type: &str,
        param_value: &str,
    ) -> Result<(), DbErr> {
        let param_name = param_name.to_string();
        let param_type = param_type.to_string();
        let param_value = param_value.to_string();

        self.with_runtime(|conn| async move {
            let model = assistant_prompt_param::ActiveModel {
                id: ActiveValue::NotSet,
                assistant_id: Set(assistant_id),
                assistant_prompt_id: Set(assistant_prompt_id),
                param_name: Set(param_name),
                param_type: Set(Some(param_type)),
                param_value: Set(Some(param_value)),
            };
            model.insert(&conn).await?;
            Ok(())
        })?;

        debug!("assistant prompt param inserted");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id = id, param_name = param_name))]
    pub fn update_assistant_prompt_param(
        &self,
        id: i64,
        param_name: &str,
        param_type: &str,
        param_value: &str,
    ) -> Result<(), DbErr> {
        let param_name = param_name.to_string();
        let param_type = param_type.to_string();
        let param_value = param_value.to_string();

        self.with_runtime(|conn| async move {
            assistant_prompt_param::Entity::update_many()
                .col_expr(assistant_prompt_param::Column::ParamName, Expr::value(param_name))
                .col_expr(assistant_prompt_param::Column::ParamType, Expr::value(param_type))
                .col_expr(assistant_prompt_param::Column::ParamValue, Expr::value(param_value))
                .filter(assistant_prompt_param::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("assistant prompt param updated");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn delete_assistant_prompt_param_by_assistant_id(
        &self,
        assistant_id: i64,
    ) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            assistant_prompt_param::Entity::delete_many()
                .filter(assistant_prompt_param::Column::AssistantId.eq(assistant_id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("assistant prompt params deleted by assistant_id");
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_assistants(&self) -> Result<Vec<Assistant>, DbErr> {
        let models =
            self.with_runtime(|conn| async move { assistant::Entity::find().all(&conn).await })?;

        let assistants: Vec<Assistant> = models.into_iter().map(|m| m.into()).collect();
        Ok(assistants)
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn get_assistant(&self, assistant_id: i64) -> Result<Assistant, DbErr> {
        let model = self.with_runtime(|conn| async move {
            assistant::Entity::find()
                .filter(assistant::Column::Id.eq(assistant_id))
                .one(&conn)
                .await
        })?;

        model.map(|m| m.into()).ok_or_else(|| DbErr::Custom("Assistant not found".to_string()))
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn get_assistant_model(&self, assistant_id: i64) -> Result<Vec<AssistantModel>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            assistant_model::Entity::find()
                .filter(assistant_model::Column::AssistantId.eq(assistant_id))
                .all(&conn)
                .await
        })?;

        let assistant_models: Vec<AssistantModel> = models.into_iter().map(|m| m.into()).collect();
        Ok(assistant_models)
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn get_assistant_prompt(&self, assistant_id: i64) -> Result<Vec<AssistantPrompt>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            assistant_prompt::Entity::find()
                .filter(assistant_prompt::Column::AssistantId.eq(assistant_id))
                .all(&conn)
                .await
        })?;

        let assistant_prompts: Vec<AssistantPrompt> =
            models.into_iter().map(|m| m.into()).collect();
        Ok(assistant_prompts)
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn get_assistant_model_configs(
        &self,
        assistant_id: i64,
    ) -> Result<Vec<AssistantModelConfig>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            assistant_model_config::Entity::find()
                .filter(assistant_model_config::Column::AssistantId.eq(assistant_id))
                .all(&conn)
                .await
        })?;

        let assistant_model_configs: Vec<AssistantModelConfig> =
            models.into_iter().map(|m| m.into()).collect();
        Ok(assistant_model_configs)
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id, assistant_model_id = assistant_model_id))]
    pub fn get_assistant_model_configs_with_model_id(
        &self,
        assistant_id: i64,
        assistant_model_id: i64,
    ) -> Result<Vec<AssistantModelConfig>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            assistant_model_config::Entity::find()
                .filter(assistant_model_config::Column::AssistantId.eq(assistant_id))
                .filter(assistant_model_config::Column::AssistantModelId.eq(assistant_model_id))
                .all(&conn)
                .await
        })?;

        let assistant_model_configs: Vec<AssistantModelConfig> =
            models.into_iter().map(|m| m.into()).collect();
        Ok(assistant_model_configs)
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn get_assistant_prompt_params(
        &self,
        assistant_id: i64,
    ) -> Result<Vec<AssistantPromptParam>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            assistant_prompt_param::Entity::find()
                .filter(assistant_prompt_param::Column::AssistantId.eq(assistant_id))
                .all(&conn)
                .await
        })?;

        let assistant_prompt_params: Vec<AssistantPromptParam> =
            models.into_iter().map(|m| m.into()).collect();
        Ok(assistant_prompt_params)
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn init_assistant(&self) -> Result<(), DbErr> {
        // Check if assistant with id=1 already exists
        let exists = self.with_runtime(|conn| async move {
            assistant::Entity::find().filter(assistant::Column::Id.eq(1)).one(&conn).await
        })?;

        if exists.is_some() {
            debug!("Default assistant already exists, skipping initialization");
            return Ok(());
        }

        // Insert using raw SQL to set specific ID
        self.with_runtime(|conn| async move {
            conn.execute_unprepared(
                "INSERT INTO assistant (id, name, description, is_addition) VALUES (1, '快速使用助手', '快捷键呼出的快速使用助手', 0)"
            ).await?;
            Ok(())
        })?;

        self.add_assistant_prompt(1, "You are a helpful assistant.")?;
        self.add_assistant_model_config(1, -1, "max_tokens", "1000", "number")?;
        self.add_assistant_model_config(1, -1, "temperature", "0.75", "float")?;
        self.add_assistant_model_config(1, -1, "top_p", "1.0", "float")?;
        self.add_assistant_model_config(1, -1, "stream", "false", "boolean")?;

        debug!("Initialized default assistant");
        Ok(())
    }

    // MCP Configuration Methods
    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn get_assistant_mcp_configs(
        &self,
        assistant_id: i64,
    ) -> Result<Vec<AssistantMCPConfig>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            assistant_mcp_config::Entity::find()
                .filter(assistant_mcp_config::Column::AssistantId.eq(assistant_id))
                .all(&conn)
                .await
        })?;

        let mcp_configs: Vec<AssistantMCPConfig> = models.into_iter().map(|m| m.into()).collect();
        Ok(mcp_configs)
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id, mcp_server_id = mcp_server_id, is_enabled = is_enabled))]
    pub fn upsert_assistant_mcp_config(
        &self,
        assistant_id: i64,
        mcp_server_id: i64,
        is_enabled: bool,
    ) -> Result<(), DbErr> {
        // Use raw SQL for INSERT OR REPLACE
        // legacy sql variable removed (unused)

        self.with_runtime(|conn| async move {
            conn.execute_unprepared(&format!(
                "INSERT OR REPLACE INTO assistant_mcp_config (assistant_id, mcp_server_id, is_enabled) VALUES ({}, {}, {})",
                assistant_id, mcp_server_id, if is_enabled { 1 } else { 0 }
            )).await?;
            Ok(())
        })?;

        debug!("assistant mcp config upserted");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn get_assistant_mcp_tool_configs(
        &self,
        assistant_id: i64,
    ) -> Result<Vec<AssistantMCPToolConfig>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            assistant_mcp_tool_config::Entity::find()
                .filter(assistant_mcp_tool_config::Column::AssistantId.eq(assistant_id))
                .all(&conn)
                .await
        })?;

        let mcp_tool_configs: Vec<AssistantMCPToolConfig> =
            models.into_iter().map(|m| m.into()).collect();
        Ok(mcp_tool_configs)
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id, mcp_tool_id = mcp_tool_id, is_enabled = is_enabled, is_auto_run = is_auto_run))]
    pub fn upsert_assistant_mcp_tool_config(
        &self,
        assistant_id: i64,
        mcp_tool_id: i64,
        is_enabled: bool,
        is_auto_run: bool,
    ) -> Result<(), DbErr> {
        // Use raw SQL for INSERT OR REPLACE
        self.with_runtime(|conn| async move {
            conn.execute_unprepared(&format!(
                "INSERT OR REPLACE INTO assistant_mcp_tool_config (assistant_id, mcp_tool_id, is_enabled, is_auto_run) VALUES ({}, {}, {}, {})",
                assistant_id, mcp_tool_id, if is_enabled { 1 } else { 0 }, if is_auto_run { 1 } else { 0 }
            )).await?;
            Ok(())
        })?;

        debug!("assistant mcp tool config upserted");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(assistant_id = assistant_id))]
    pub fn get_assistant_mcp_servers_with_tools(
        &self,
        assistant_id: i64,
    ) -> Result<Vec<(i64, String, bool, Vec<(i64, String, String, bool, bool, String)>)>, DbErr>
    {
        // 现在所有数据都在同一个数据库中
        // 1. 读取启用的服务器及其工具
        let servers = {
            use crate::db::mcp_db::mcp_server;
            self.with_runtime(|conn| async move {
                let models = mcp_server::Entity::find()
                    .filter(mcp_server::Column::IsEnabled.eq(true))
                    .order_by_asc(mcp_server::Column::Name)
                    .all(&conn)
                    .await?;
                Ok::<Vec<(i64, String)>, DbErr>(
                    models.into_iter().map(|m| (m.id, m.name)).collect(),
                )
            })?
        };

        if servers.is_empty() {
            return Ok(Vec::new());
        }

        let server_ids: Vec<i64> = servers.iter().map(|(id, _)| *id).collect();

        // 2. 从 assistant.db 读取服务器配置状态
        let server_configs: std::collections::HashMap<i64, bool> = {
            use crate::db::assistant_db::assistant_mcp_config;
            let server_ids_clone = server_ids.clone();
            self.with_runtime(|conn| async move {
                let models = assistant_mcp_config::Entity::find()
                    .filter(assistant_mcp_config::Column::AssistantId.eq(assistant_id))
                    .filter(assistant_mcp_config::Column::McpServerId.is_in(server_ids_clone))
                    .all(&conn)
                    .await?;
                Ok::<_, DbErr>(
                    models.into_iter().map(|m| (m.mcp_server_id, m.is_enabled)).collect(),
                )
            })?
        };

        // 3. 读取所有启用的工具
        let all_tools: Vec<(i64, i64, String, String, String)> = {
            use crate::db::mcp_db::mcp_server_tool;
            let server_ids_clone = server_ids.clone();
            self.with_runtime(|conn| async move {
                let models = mcp_server_tool::Entity::find()
                    .filter(mcp_server_tool::Column::ServerId.is_in(server_ids_clone))
                    .filter(mcp_server_tool::Column::IsEnabled.eq(true))
                    .order_by_asc(mcp_server_tool::Column::ServerId)
                    .order_by_asc(mcp_server_tool::Column::ToolName)
                    .all(&conn)
                    .await?;
                Ok::<_, DbErr>(
                    models
                        .into_iter()
                        .map(|m| {
                            (
                                m.id,
                                m.server_id,
                                m.tool_name,
                                m.tool_description.unwrap_or_default(),
                                m.parameters.unwrap_or_default(),
                            )
                        })
                        .collect(),
                )
            })?
        };

        // 4. 从 assistant.db 读取工具配置状态
        let tool_configs: std::collections::HashMap<i64, (bool, bool)> = if all_tools.is_empty() {
            std::collections::HashMap::new()
        } else {
            let tool_ids: Vec<i64> = all_tools.iter().map(|(id, _, _, _, _)| *id).collect();
            self.with_runtime(|conn| async move {
                use crate::db::assistant_db::assistant_mcp_tool_config;
                let models = assistant_mcp_tool_config::Entity::find()
                    .filter(assistant_mcp_tool_config::Column::AssistantId.eq(assistant_id))
                    .filter(assistant_mcp_tool_config::Column::McpToolId.is_in(tool_ids.clone()))
                    .all(&conn)
                    .await?;
                Ok::<_, DbErr>(
                    models
                        .into_iter()
                        .map(|m| (m.mcp_tool_id, (m.is_enabled, m.is_auto_run)))
                        .collect(),
                )
            })?
        };

        // 5. 组装结果
        let mut result = Vec::new();
        for (server_id, server_name) in servers {
            let server_is_enabled = server_configs.get(&server_id).copied().unwrap_or(false);
            let server_tools: Vec<(i64, String, String, bool, bool, String)> = all_tools
                .iter()
                .filter(|(_, sid, _, _, _)| *sid == server_id)
                .map(|(tool_id, _, tool_name, tool_description, tool_parameters)| {
                    let (tool_is_enabled, tool_is_auto_run) =
                        tool_configs.get(tool_id).copied().unwrap_or((true, false));
                    (
                        *tool_id,
                        tool_name.clone(),
                        tool_description.clone(),
                        tool_is_enabled,
                        tool_is_auto_run,
                        tool_parameters.clone(),
                    )
                })
                .collect();

            result.push((server_id, server_name, server_is_enabled, server_tools));
        }

        debug!(server_count = result.len(), "fetched assistant mcp servers with tools");
        Ok(result)
    }
}
