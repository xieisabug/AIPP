use sea_orm::Schema;
use sea_orm::{
    entity::prelude::*, ActiveValue, DatabaseBackend, DatabaseConnection, DbErr, Set,
};
use serde::{Deserialize, Serialize};
use tauri::Manager; // for try_state
use tracing::{debug, instrument, warn};

use super::get_db_path;

// ============ LLMProvider Entity ============
pub mod llm_provider {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "llm_provider")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub api_type: String,
        pub description: Option<String>,
        pub is_official: bool,
        pub is_enabled: bool,
        pub created_time: Option<ChronoDateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ LLMModel Entity ============
pub mod llm_model {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "llm_model")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub llm_provider_id: i64,
        pub code: String,
        pub description: Option<String>,
        pub vision_support: bool,
        pub audio_support: bool,
        pub video_support: bool,
        pub created_time: Option<ChronoDateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ LLMProviderConfig Entity ============
pub mod llm_provider_config {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "llm_provider_config")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub llm_provider_id: i64,
        pub value: Option<String>,
        pub append_location: String,
        pub is_addition: bool,
        pub created_time: Option<ChronoDateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// Legacy structs for backward compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMProvider {
    pub id: i64,
    pub name: String,
    pub api_type: String,
    pub description: String,
    pub is_official: bool,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMProviderConfig {
    pub id: i64,
    pub name: String,
    pub llm_provider_id: i64,
    pub value: String,
    pub append_location: String,
    pub is_addition: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMModel {
    pub id: i64,
    pub name: String,
    pub llm_provider_id: i64,
    pub code: String,
    pub description: String,
    pub vision_support: bool,
    pub audio_support: bool,
    pub video_support: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelDetail {
    pub model: LLMModel,
    pub provider: LLMProvider,
    pub configs: Vec<LLMProviderConfig>,
}

impl From<llm_provider::Model> for LLMProvider {
    fn from(model: llm_provider::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            api_type: model.api_type,
            description: model.description.unwrap_or_default(),
            is_official: model.is_official,
            is_enabled: model.is_enabled,
        }
    }
}

impl From<llm_provider_config::Model> for LLMProviderConfig {
    fn from(model: llm_provider_config::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            llm_provider_id: model.llm_provider_id,
            value: model.value.unwrap_or_default(),
            append_location: model.append_location,
            is_addition: model.is_addition,
        }
    }
}

impl From<llm_model::Model> for LLMModel {
    fn from(model: llm_model::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            llm_provider_id: model.llm_provider_id,
            code: model.code,
            description: model.description.unwrap_or_default(),
            vision_support: model.vision_support,
            audio_support: model.audio_support,
            video_support: model.video_support,
        }
    }
}

pub struct LLMDatabase {
    pub conn: DatabaseConnection,
}

impl LLMDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "llm.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, DbErr> {
        // 从全局状态获取共享连接，而不是创建新连接
        let conn_arc = crate::db::conn_helper::get_db_conn(app_handle)?;
        let conn = (*conn_arc).clone(); // DatabaseConnection 内部是 Arc，clone 很轻量
        
        debug!("Acquired shared database connection for LLM");
        Ok(LLMDatabase { conn })
    }
    
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

    #[instrument(level = "debug", skip(app_handle))]
    pub fn create_tables(app_handle: &tauri::AppHandle) -> Result<(), DbErr> {
        let db = Self::new(app_handle)?;
        let backend = db.conn.get_database_backend();
        let schema = Schema::new(backend);
        let sql_provider = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(llm_provider::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(llm_provider::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(llm_provider::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(llm_provider::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_model = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(llm_model::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(llm_model::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(llm_model::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(llm_model::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_provider_config = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(llm_provider_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(llm_provider_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(llm_provider_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(llm_provider_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };

        db.with_runtime(|conn| async move {
            conn.execute_unprepared(&sql_provider).await?;
            conn.execute_unprepared(&sql_model).await?;
            conn.execute_unprepared(&sql_provider_config).await?;
            Ok(())
        })?;

        if let Err(err) = db.init_llm_provider() {
            warn!(error = ?err, "init_llm_provider failed (may already be initialized)");
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(name, api_type, is_official, is_enabled))]
    pub fn add_llm_provider(
        &self,
        name: &str,
        api_type: &str,
        description: &str,
        is_official: bool,
        is_enabled: bool,
    ) -> Result<(), DbErr> {
        let name = name.to_string();
        let api_type = api_type.to_string();
        let description = description.to_string();

        self.with_runtime(|conn| async move {
            let model = llm_provider::ActiveModel {
                id: ActiveValue::NotSet,
                name: Set(name),
                api_type: Set(api_type),
                description: Set(Some(description)),
                is_official: Set(is_official),
                is_enabled: Set(is_enabled),
                created_time: ActiveValue::NotSet,
            };
            model.insert(&conn).await?;
            Ok(())
        })?;

        debug!("llm provider inserted");
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_llm_providers(
        &self,
    ) -> Result<Vec<(i64, String, String, String, bool, bool)>, String> {
        self.with_runtime(|conn| async move {
            let providers = llm_provider::Entity::find().all(&conn).await?;

            Ok(providers
                .into_iter()
                .map(|p| {
                    (
                        p.id,
                        p.name,
                        p.api_type,
                        p.description.unwrap_or_default(),
                        p.is_official,
                        p.is_enabled,
                    )
                })
                .collect())
        })
        .map_err(|e: DbErr| e.to_string())
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn get_llm_provider(&self, id: i64) -> Result<LLMProvider, DbErr> {
        self.with_runtime(|conn| async move {
            let provider = llm_provider::Entity::find_by_id(id)
                .one(&conn)
                .await?
                .ok_or(DbErr::RecordNotFound("Provider not found".to_string()))?;

            Ok(provider.into())
        })
    }

    #[instrument(level = "debug", skip(self), fields(id, name, api_type, is_enabled))]
    pub fn update_llm_provider(
        &self,
        id: i64,
        name: &str,
        api_type: &str,
        description: &str,
        is_enabled: bool,
    ) -> Result<(), DbErr> {
        let name = name.to_string();
        let api_type = api_type.to_string();
        let description = description.to_string();

        self.with_runtime(|conn| async move {
            llm_provider::Entity::update_many()
                .col_expr(llm_provider::Column::Name, Expr::value(name))
                .col_expr(llm_provider::Column::ApiType, Expr::value(api_type))
                .col_expr(llm_provider::Column::Description, Expr::value(Some(description)))
                .col_expr(llm_provider::Column::IsEnabled, Expr::value(is_enabled))
                .filter(llm_provider::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn delete_llm_provider(&self, id: i64) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            llm_provider_config::Entity::delete_many()
                .filter(llm_provider_config::Column::LlmProviderId.eq(id))
                .exec(&conn)
                .await?;

            llm_model::Entity::delete_many()
                .filter(llm_model::Column::LlmProviderId.eq(id))
                .exec(&conn)
                .await?;

            llm_provider::Entity::delete_by_id(id).exec(&conn).await?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(llm_provider_id))]
    pub fn get_llm_provider_config(
        &self,
        llm_provider_id: i64,
    ) -> Result<Vec<LLMProviderConfig>, DbErr> {
        self.with_runtime(|conn| async move {
            let configs = llm_provider_config::Entity::find()
                .filter(llm_provider_config::Column::LlmProviderId.eq(llm_provider_id))
                .all(&conn)
                .await?;

            Ok(configs.into_iter().map(|c| c.into()).collect())
        })
    }

    #[instrument(level = "debug", skip(self), fields(llm_provider_id, name))]
    pub fn update_llm_provider_config(
        &self,
        llm_provider_id: i64,
        name: &str,
        value: &str,
    ) -> Result<(), DbErr> {
        let name_clone = name.to_string();
        let value_clone = value.to_string();

        self.with_runtime(|conn| async move {
            // Check if config exists
            let existing = llm_provider_config::Entity::find()
                .filter(llm_provider_config::Column::LlmProviderId.eq(llm_provider_id))
                .filter(llm_provider_config::Column::Name.eq(&name_clone))
                .one(&conn)
                .await?;

            if let Some(existing) = existing {
                // Update existing
                llm_provider_config::Entity::update_many()
                    .col_expr(llm_provider_config::Column::Value, Expr::value(Some(value_clone)))
                    .filter(llm_provider_config::Column::Id.eq(existing.id))
                    .exec(&conn)
                    .await?;
            } else {
                // Insert new
                let model = llm_provider_config::ActiveModel {
                    id: ActiveValue::NotSet,
                    name: Set(name_clone),
                    llm_provider_id: Set(llm_provider_id),
                    value: Set(Some(value_clone)),
                    append_location: Set("header".to_string()),
                    is_addition: Set(false),
                    created_time: ActiveValue::NotSet,
                };
                model.insert(&conn).await?;
            }
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(llm_provider_id, name, is_addition))]
    pub fn add_llm_provider_config(
        &self,
        llm_provider_id: i64,
        name: &str,
        value: &str,
        append_location: &str,
        is_addition: bool,
    ) -> Result<(), DbErr> {
        let name = name.to_string();
        let value = value.to_string();
        let append_location = append_location.to_string();

        self.with_runtime(|conn| async move {
            let model = llm_provider_config::ActiveModel {
                id: ActiveValue::NotSet,
                name: Set(name),
                llm_provider_id: Set(llm_provider_id),
                value: Set(Some(value)),
                append_location: Set(append_location),
                is_addition: Set(is_addition),
                created_time: ActiveValue::NotSet,
            };
            model.insert(&conn).await?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(llm_provider_id, name, code))]
    pub fn add_llm_model(
        &self,
        name: &str,
        llm_provider_id: i64,
        code: &str,
        description: &str,
        vision_support: bool,
        audio_support: bool,
        video_support: bool,
    ) -> Result<(), DbErr> {
        let name = name.to_string();
        let code = code.to_string();
        let description = description.to_string();

        self.with_runtime(|conn| async move {
            let model = llm_model::ActiveModel {
                id: ActiveValue::NotSet,
                name: Set(name),
                llm_provider_id: Set(llm_provider_id),
                code: Set(code),
                description: Set(Some(description)),
                vision_support: Set(vision_support),
                audio_support: Set(audio_support),
                video_support: Set(video_support),
                created_time: ActiveValue::NotSet,
            };
            model.insert(&conn).await?;
            Ok(())
        })
    }

    pub fn get_all_llm_models(
        &self,
    ) -> Result<Vec<(i64, String, i64, String, String, bool, bool, bool)>, String> {
        self.with_runtime(|conn| async move {
            let models = llm_model::Entity::find().all(&conn).await?;

            Ok(models
                .into_iter()
                .map(|m| {
                    (
                        m.id,
                        m.name,
                        m.llm_provider_id,
                        m.code,
                        m.description.unwrap_or_default(),
                        m.vision_support,
                        m.audio_support,
                        m.video_support,
                    )
                })
                .collect())
        })
        .map_err(|e: DbErr| e.to_string())
    }

    pub fn get_llm_models(
        &self,
        provider_id: String,
    ) -> Result<Vec<(i64, String, i64, String, String, bool, bool, bool)>, String> {
        let provider_id: i64 =
            provider_id.parse().map_err(|e| format!("Invalid provider_id: {}", e))?;

        self.with_runtime(|conn| async move {
            let models = llm_model::Entity::find()
                .filter(llm_model::Column::LlmProviderId.eq(provider_id))
                .all(&conn)
                .await?;

            Ok(models
                .into_iter()
                .map(|m| {
                    (
                        m.id,
                        m.name,
                        m.llm_provider_id,
                        m.code,
                        m.description.unwrap_or_default(),
                        m.vision_support,
                        m.audio_support,
                        m.video_support,
                    )
                })
                .collect())
        })
        .map_err(|e: DbErr| e.to_string())
    }

    #[instrument(level = "debug", skip(self), fields(provider_id, model_code))]
    pub fn get_llm_model_detail(
        &self,
        provider_id: &i64,
        model_code: &String,
    ) -> Result<ModelDetail, DbErr> {
        let provider_id = *provider_id;
        let model_code = model_code.clone();

        self.with_runtime(|conn| async move {
            let model = llm_model::Entity::find()
                .filter(llm_model::Column::LlmProviderId.eq(provider_id))
                .filter(llm_model::Column::Code.eq(model_code))
                .one(&conn)
                .await?
                .ok_or(DbErr::RecordNotFound("Model not found".to_string()))?;

            let provider = llm_provider::Entity::find_by_id(model.llm_provider_id)
                .one(&conn)
                .await?
                .ok_or(DbErr::RecordNotFound("Provider not found".to_string()))?;

            let configs = llm_provider_config::Entity::find()
                .filter(llm_provider_config::Column::LlmProviderId.eq(provider.id))
                .all(&conn)
                .await?;

            Ok(ModelDetail {
                model: model.into(),
                provider: provider.into(),
                configs: configs.into_iter().map(|c| c.into()).collect(),
            })
        })
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn get_llm_model_detail_by_id(&self, id: &i64) -> Result<ModelDetail, DbErr> {
        let id = *id;

        self.with_runtime(|conn| async move {
            let model = llm_model::Entity::find_by_id(id)
                .one(&conn)
                .await?
                .ok_or(DbErr::RecordNotFound("Model not found".to_string()))?;

            let provider = llm_provider::Entity::find_by_id(model.llm_provider_id)
                .one(&conn)
                .await?
                .ok_or(DbErr::RecordNotFound("Provider not found".to_string()))?;

            let configs = llm_provider_config::Entity::find()
                .filter(llm_provider_config::Column::LlmProviderId.eq(provider.id))
                .all(&conn)
                .await?;

            Ok(ModelDetail {
                model: model.into(),
                provider: provider.into(),
                configs: configs.into_iter().map(|c| c.into()).collect(),
            })
        })
    }

    #[instrument(level = "debug", skip(self), fields(provider_id, code))]
    pub fn delete_llm_model(&self, provider_id: i64, code: String) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            llm_model::Entity::delete_many()
                .filter(llm_model::Column::LlmProviderId.eq(provider_id))
                .filter(llm_model::Column::Code.eq(code))
                .exec(&conn)
                .await?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(provider_id))]
    pub fn delete_llm_model_by_provider(&self, provider_id: i64) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            llm_model::Entity::delete_many()
                .filter(llm_model::Column::LlmProviderId.eq(provider_id))
                .exec(&conn)
                .await?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_models_for_select(&self) -> Result<Vec<(String, String, i64, i64)>, String> {
        self.with_runtime(|conn| async move {
            // 一次性查询所有启用的 providers，避免 N+1 查询
            let providers = llm_provider::Entity::find()
                .filter(llm_provider::Column::IsEnabled.eq(true))
                .all(&conn)
                .await?;

            // 构建 provider id 到 provider 的映射
            let provider_map: std::collections::HashMap<i64, llm_provider::Model> =
                providers.into_iter().map(|p| (p.id, p)).collect();

            // 查询所有模型
            let models = llm_model::Entity::find().all(&conn).await?;

            // 过滤并构建结果
            let result = models
                .into_iter()
                .filter_map(|model| {
                    provider_map.get(&model.llm_provider_id).map(|provider| {
                        let name = format!("{} / {}", provider.name, model.name);
                        (name, model.code.clone(), model.id, model.llm_provider_id)
                    })
                })
                .collect();

            Ok(result)
        })
        .map_err(|e: DbErr| e.to_string())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn init_llm_provider(&self) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            conn.execute_unprepared(
                "INSERT INTO llm_provider (id, name, api_type, description, is_official) VALUES (1, 'OpenAI', 'openai_api', 'OpenAI API', 1)"
            ).await?;
            conn.execute_unprepared(
                "INSERT INTO llm_provider (id, name, api_type, description, is_official) VALUES (10, 'Ollama', 'ollama', 'Ollama API', 1)"
            ).await?;
            conn.execute_unprepared(
                "INSERT INTO llm_provider (id, name, api_type, description, is_official) VALUES (20, 'Anthropic', 'anthropic', 'Anthropic API', 1)"
            ).await?;
            conn.execute_unprepared(
                "INSERT INTO llm_provider (id, name, api_type, description, is_official) VALUES (30, 'DeepSeek', 'deepseek', 'DeepSeek API', 1)"
            ).await?;
            Ok(())
        })
    }
}
