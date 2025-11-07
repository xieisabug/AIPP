use crate::db::get_db_path;
use rusqlite::{params, Connection};
use sea_orm::{
    entity::prelude::*, ActiveValue, Database, DatabaseConnection, DbErr, QueryFilter, QuerySelect,
    Set,
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
    pub mcp_conn: Connection, // Keep rusqlite for complex join queries
    db_path: std::path::PathBuf,
}

impl AssistantDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "assistant.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, DbErr> {
        let db_path = get_db_path(app_handle, "assistant.db").map_err(|e| DbErr::Custom(e))?;
        let url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());

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

        let mcp_db_path = get_db_path(app_handle, "mcp.db").map_err(|e| DbErr::Custom(e))?;
        let mcp_conn = Connection::open(mcp_db_path)
            .map_err(|e| DbErr::Custom(format!("Failed to open mcp.db: {}", e)))?;

        debug!("Opened assistant database");
        Ok(AssistantDatabase { conn, mcp_conn, db_path })
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

    /// Get a rusqlite connection for migrations
    pub fn get_rusqlite_connection(
        &self,
        _app_handle: &tauri::AppHandle,
    ) -> Result<Connection, String> {
        Connection::open(&self.db_path).map_err(|e| e.to_string())
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn create_tables(&self) -> Result<(), DbErr> {
        let sql1 = r#"
            CREATE TABLE IF NOT EXISTS assistant (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                description TEXT,
                assistant_type INTEGER NOT NULL DEFAULT 0,
                is_addition BOOLEAN NOT NULL DEFAULT 0,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP
            );
        "#;
        let sql2 = r#"
            CREATE TABLE IF NOT EXISTS assistant_model (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                assistant_id INTEGER NOT NULL,
                provider_id INTEGER NOT NULL,
                model_code TEXT NOT NULL,
                alias TEXT
            );
        "#;
        let sql3 = r#"
            CREATE TABLE IF NOT EXISTS assistant_prompt (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                assistant_id INTEGER,
                prompt TEXT NOT NULL,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (assistant_id) REFERENCES assistant(id)
            );
        "#;
        let sql4 = r#"
            CREATE TABLE IF NOT EXISTS assistant_model_config (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                assistant_id INTEGER,
                assistant_model_id INTEGER,
                name TEXT NOT NULL,
                value TEXT,
                value_type TEXT default 'float' not null,
                FOREIGN KEY (assistant_id) REFERENCES assistant(id)
            );
        "#;
        let sql5 = r#"
            CREATE TABLE IF NOT EXISTS assistant_prompt_param (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                assistant_id INTEGER,
                assistant_prompt_id INTEGER,
                param_name TEXT NOT NULL,
                param_type TEXT,
                param_value TEXT,
                FOREIGN KEY (assistant_id) REFERENCES assistant(id),
                FOREIGN KEY (assistant_prompt_id) REFERENCES assistant_prompt(id)
            );
        "#;
        let sql6 = r#"
            CREATE TABLE IF NOT EXISTS assistant_mcp_config (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                assistant_id INTEGER NOT NULL,
                mcp_server_id INTEGER NOT NULL,
                is_enabled BOOLEAN NOT NULL DEFAULT 1,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (assistant_id) REFERENCES assistant(id) ON DELETE CASCADE,
                UNIQUE(assistant_id, mcp_server_id)
            );
        "#;
        let sql7 = r#"
            CREATE TABLE IF NOT EXISTS assistant_mcp_tool_config (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                assistant_id INTEGER NOT NULL,
                mcp_tool_id INTEGER NOT NULL,
                is_enabled BOOLEAN NOT NULL DEFAULT 1,
                is_auto_run BOOLEAN NOT NULL DEFAULT 0,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (assistant_id) REFERENCES assistant(id) ON DELETE CASCADE,
                UNIQUE(assistant_id, mcp_tool_id)
            );
        "#;

        self.with_runtime(|conn| async move {
            conn.execute_unprepared(sql1).await?;
            conn.execute_unprepared(sql2).await?;
            conn.execute_unprepared(sql3).await?;
            conn.execute_unprepared(sql4).await?;
            conn.execute_unprepared(sql5).await?;
            conn.execute_unprepared(sql6).await?;
            conn.execute_unprepared(sql7).await?;
            Ok(())
        })?;

        if let Err(err) = self.init_assistant() {
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
        let sql = "INSERT OR REPLACE INTO assistant_mcp_config (assistant_id, mcp_server_id, is_enabled) VALUES (?, ?, ?)";

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
        // This complex join query uses mcp_conn (rusqlite) for cross-database queries
        // 1. 获取所有启用的服务器及其配置状态
        let mut server_stmt = self
            .mcp_conn
            .prepare(
                "
            SELECT s.id, s.name
            FROM mcp_server s
            WHERE s.is_enabled = 1
            ORDER BY s.name
        ",
            )
            .map_err(|e| DbErr::Custom(format!("Failed to prepare server query: {}", e)))?;

        let servers: Vec<(i64, String)> = server_stmt
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| DbErr::Custom(format!("Failed to execute server query: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbErr::Custom(format!("Failed to collect servers: {}", e)))?;

        if servers.is_empty() {
            return Ok(Vec::new());
        }

        // 2. 批量获取所有服务器的配置状态
        let server_ids: Vec<String> = servers.iter().map(|(id, _)| id.to_string()).collect();
        let server_ids_placeholder = vec!["?"; server_ids.len()].join(",");
        let server_config_sql = format!(
            "SELECT mcp_server_id, is_enabled FROM assistant_mcp_config 
             WHERE assistant_id = ? AND mcp_server_id IN ({})",
            server_ids_placeholder
        );

        // Need to use rusqlite for assistant.db query in this cross-database operation
        let assistant_rusqlite_conn = Connection::open(&self.db_path)
            .map_err(|e| DbErr::Custom(format!("Failed to open assistant.db for query: {}", e)))?;

        let mut server_config_stmt = assistant_rusqlite_conn
            .prepare(&server_config_sql)
            .map_err(|e| DbErr::Custom(format!("Failed to prepare config query: {}", e)))?;
        let mut server_config_params = vec![assistant_id];
        server_config_params.extend(servers.iter().map(|(id, _)| *id));

        let server_configs: std::collections::HashMap<i64, bool> = server_config_stmt
            .query_map(rusqlite::params_from_iter(server_config_params), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?))
            })
            .map_err(|e| DbErr::Custom(format!("Failed to execute config query: {}", e)))?
            .collect::<Result<std::collections::HashMap<_, _>, _>>()
            .map_err(|e| DbErr::Custom(format!("Failed to collect configs: {}", e)))?;

        // 3. 获取所有工具信息（一次性获取所有服务器的工具）
        let tools_sql = format!(
            "SELECT t.id, t.server_id, t.tool_name, t.tool_description, t.parameters
             FROM mcp_server_tool t
             WHERE t.server_id IN ({}) AND t.is_enabled = 1
             ORDER BY t.server_id, t.tool_name",
            server_ids_placeholder
        );

        let mut tools_stmt = self
            .mcp_conn
            .prepare(&tools_sql)
            .map_err(|e| DbErr::Custom(format!("Failed to prepare tools query: {}", e)))?;

        let all_tools: Vec<(i64, i64, String, String, String)> = tools_stmt
            .query_map(rusqlite::params_from_iter(servers.iter().map(|(id, _)| *id)), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|e| DbErr::Custom(format!("Failed to execute tools query: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbErr::Custom(format!("Failed to collect tools: {}", e)))?;

        // 4. 批量获取工具配置状态
        let mut tool_configs: std::collections::HashMap<i64, (bool, bool)> =
            std::collections::HashMap::new();
        if !all_tools.is_empty() {
            let tool_ids: Vec<String> =
                all_tools.iter().map(|(id, _, _, _, _)| id.to_string()).collect();
            let tool_ids_placeholder = vec!["?"; tool_ids.len()].join(",");
            let tool_config_sql = format!(
                "SELECT mcp_tool_id, is_enabled, is_auto_run FROM assistant_mcp_tool_config 
                 WHERE assistant_id = ? AND mcp_tool_id IN ({})",
                tool_ids_placeholder
            );

            let mut tool_config_stmt =
                assistant_rusqlite_conn.prepare(&tool_config_sql).map_err(|e| {
                    DbErr::Custom(format!("Failed to prepare tool config query: {}", e))
                })?;
            let mut tool_config_params = vec![assistant_id];
            tool_config_params.extend(all_tools.iter().map(|(id, _, _, _, _)| *id));

            tool_configs = tool_config_stmt
                .query_map(rusqlite::params_from_iter(tool_config_params), |row| {
                    Ok((row.get::<_, i64>(0)?, (row.get::<_, bool>(1)?, row.get::<_, bool>(2)?)))
                })
                .map_err(|e| DbErr::Custom(format!("Failed to execute tool config query: {}", e)))?
                .collect::<Result<std::collections::HashMap<_, _>, _>>()
                .map_err(|e| DbErr::Custom(format!("Failed to collect tool configs: {}", e)))?;
        }

        // 5. 组织数据结构
        let mut result = Vec::new();
        for (server_id, server_name) in servers {
            let server_is_enabled = server_configs.get(&server_id).copied().unwrap_or(false);

            // 获取该服务器的所有工具
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
