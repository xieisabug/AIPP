use crate::db::get_db_path;
use sea_orm::Schema;
use sea_orm::{
    entity::prelude::*, ActiveValue, Database, DatabaseBackend, DatabaseConnection, DbErr, Set,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

// ============ SystemConfig Entity ============
pub mod system_config {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "system_config")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(unique)]
        pub key: String,
        pub value: String,
        pub created_time: Option<ChronoDateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ FeatureConfig Entity ============
pub mod feature_config {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "feature_config")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub feature_code: String,
        pub key: String,
        pub value: String,
        pub data_type: String,
        pub description: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// Legacy struct for backward compatibility
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FeatureConfig {
    pub id: Option<i64>,
    pub feature_code: String,
    pub key: String,
    pub value: String,
    pub data_type: String,
    pub description: Option<String>,
}

impl From<feature_config::Model> for FeatureConfig {
    fn from(model: feature_config::Model) -> Self {
        Self {
            id: Some(model.id),
            feature_code: model.feature_code,
            key: model.key,
            value: model.value,
            data_type: model.data_type,
            description: model.description,
        }
    }
}

// SystemDatabase 内部维护自己的本地 SQLite 连接，因为它必须始终使用本地存储
pub struct SystemDatabase {
    conn: DatabaseConnection,
}

impl SystemDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "system.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, DbErr> {
        let db_path = get_db_path(app_handle, "system.db").map_err(|e| DbErr::Custom(e))?;
        let url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());

        // Create a new Tokio runtime if we're not in one
        let conn = match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                // We're in a Tokio runtime; block in place to avoid nested-runtime panic
                tokio::task::block_in_place(|| {
                    handle.block_on(async { Database::connect(&url).await })
                })?
            }
            Err(_) => {
                // We're not in a Tokio runtime, create one
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| DbErr::Custom(format!("Failed to create Tokio runtime: {}", e)))?;
                rt.block_on(async { Database::connect(&url).await })?
            }
        };

        debug!("Opened system database");
        Ok(SystemDatabase { conn })
    }

    // 获取内部连接的引用（公开方法）
    pub fn get_conn(&self) -> &DatabaseConnection {
        &self.conn
    }

    #[instrument(level = "debug", skip(self, _app_handle))]
    pub fn create_tables(&self, _app_handle: &tauri::AppHandle) -> Result<(), DbErr> {
        let backend = self.get_conn().get_database_backend();
        let schema = Schema::new(backend);
        let sql1 = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(system_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(system_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(system_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(system_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql2 = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(feature_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(feature_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(feature_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(feature_config::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };

        self.with_runtime(|conn| async move {
            conn.execute_unprepared(&sql1).await?;
            conn.execute_unprepared(&sql2).await?;
            // Composite uniqueness for feature_config(feature_code, key)
            conn.execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_feature_config_unique ON feature_config(feature_code, key)",
            )
            .await?;
            Ok(())
        })
    }

    // Helper method to run async code in correct runtime context
    fn with_runtime<F, Fut, T>(&self, f: F) -> Result<T, DbErr>
    where
        F: FnOnce(DatabaseConnection) -> Fut,
        Fut: std::future::Future<Output = Result<T, DbErr>>,
    {
        let conn = self.get_conn().clone();
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f(conn))),
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| DbErr::Custom(format!("Failed to create Tokio runtime: {}", e)))?;
                rt.block_on(f(conn))
            }
        }
    }

    #[instrument(level = "debug", skip(self, _app_handle, value), fields(key))]
    pub fn add_system_config(
        &self,
        _app_handle: &tauri::AppHandle,
        key: &str,
        value: &str,
    ) -> Result<(), DbErr> {
        let key = key.to_string();
        let value = value.to_string();

        self.with_runtime(|conn| async move {
            let model = system_config::ActiveModel {
                id: ActiveValue::NotSet,
                key: Set(key),
                value: Set(value),
                created_time: ActiveValue::NotSet,
            };
            model.insert(&conn).await?;
            Ok(())
        })?;

        debug!("Inserted system config");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, _app_handle), fields(key))]
    pub fn get_config(&self, _app_handle: &tauri::AppHandle, key: &str) -> Result<String, DbErr> {
        let key = key.to_string();

        let result = self.with_runtime(|conn| async move {
            system_config::Entity::find()
                .filter(system_config::Column::Key.eq(key))
                .one(&conn)
                .await
        })?;

        if let Some(model) = result {
            debug!(found = true, "Fetched system config");
            Ok(model.value)
        } else {
            debug!(found = false, "System config not found");
            Ok(String::new())
        }
    }

    #[instrument(level = "debug", skip(self, _app_handle, value), fields(key))]
    pub fn update_system_config(
        &self,
        _app_handle: &tauri::AppHandle,
        key: &str,
        value: &str,
    ) -> Result<(), DbErr> {
        let key = key.to_string();
        let value = value.to_string();

        self.with_runtime(|conn| async move {
            system_config::Entity::update_many()
                .col_expr(system_config::Column::Value, Expr::value(value))
                .filter(system_config::Column::Key.eq(key))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated system config");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, _app_handle), fields(key))]
    pub fn delete_system_config(
        &self,
        _app_handle: &tauri::AppHandle,
        key: &str,
    ) -> Result<(), DbErr> {
        let key = key.to_string();

        self.with_runtime(|conn| async move {
            system_config::Entity::delete_many()
                .filter(system_config::Column::Key.eq(key))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Deleted system config");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, _app_handle, config), fields(feature_code = %config.feature_code, key = %config.key))]
    pub fn add_feature_config(
        &self,
        _app_handle: &tauri::AppHandle,
        config: &FeatureConfig,
    ) -> Result<(), DbErr> {
        let feature_code = config.feature_code.clone();
        let key = config.key.clone();
        let value = config.value.clone();
        let data_type = config.data_type.clone();
        let description = config.description.clone();

        self.with_runtime(|conn| async move {
            let model = feature_config::ActiveModel {
                id: ActiveValue::NotSet,
                feature_code: Set(feature_code),
                key: Set(key),
                value: Set(value),
                data_type: Set(data_type),
                description: Set(description),
            };
            model.insert(&conn).await?;
            Ok(())
        })?;

        debug!("Inserted feature config");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, _app_handle, config), fields(feature_code = %config.feature_code, key = %config.key))]
    pub fn update_feature_config(
        &self,
        _app_handle: &tauri::AppHandle,
        config: &FeatureConfig,
    ) -> Result<(), DbErr> {
        let feature_code = config.feature_code.clone();
        let key = config.key.clone();
        let value = config.value.clone();
        let data_type = config.data_type.clone();
        let description = config.description.clone();

        self.with_runtime(|conn| async move {
            feature_config::Entity::update_many()
                .col_expr(feature_config::Column::Value, Expr::value(value))
                .col_expr(feature_config::Column::DataType, Expr::value(data_type))
                .col_expr(feature_config::Column::Description, Expr::value(description))
                .filter(feature_config::Column::FeatureCode.eq(feature_code))
                .filter(feature_config::Column::Key.eq(key))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated feature config");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, _app_handle), fields(feature_code))]
    pub fn delete_feature_config_by_feature_code(
        &self,
        _app_handle: &tauri::AppHandle,
        feature_code: &str,
    ) -> Result<(), DbErr> {
        let feature_code = feature_code.to_string();

        self.with_runtime(|conn| async move {
            feature_config::Entity::delete_many()
                .filter(feature_config::Column::FeatureCode.eq(feature_code))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Deleted feature config by feature code");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, _app_handle), fields(feature_code, key))]
    pub fn get_feature_config(
        &self,
        _app_handle: &tauri::AppHandle,
        feature_code: &str,
        key: &str,
    ) -> Result<Option<FeatureConfig>, DbErr> {
        let feature_code = feature_code.to_string();
        let key = key.to_string();

        let result = self.with_runtime(|conn| async move {
            feature_config::Entity::find()
                .filter(feature_config::Column::FeatureCode.eq(feature_code))
                .filter(feature_config::Column::Key.eq(key))
                .one(&conn)
                .await
        })?;

        let config = result.map(|model| model.into());
        debug!(found = config.is_some(), "Fetched feature config");
        Ok(config)
    }

    // 查询特定模块( feature_code )的所有配置 - 对外公开，供启动时缓存 data_storage 使用
    #[instrument(level = "debug", skip(self, _app_handle), fields(feature_code))]
    pub fn get_feature_config_by_feature_code(
        &self,
        _app_handle: &tauri::AppHandle,
        feature_code: &str,
    ) -> Result<Vec<FeatureConfig>, DbErr> {
        let feature_code = feature_code.to_string();

        let models = self.with_runtime(|conn| async move {
            feature_config::Entity::find()
                .filter(feature_config::Column::FeatureCode.eq(feature_code))
                .all(&conn)
                .await
        })?;

        let configs: Vec<FeatureConfig> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = configs.len(), "Fetched feature configs by module");
        Ok(configs)
    }

    // 查询特定模块的所有配置
    #[instrument(level = "debug", skip(self, _app_handle))]
    pub fn get_all_feature_config(
        &self,
        _app_handle: &tauri::AppHandle,
    ) -> Result<Vec<FeatureConfig>, DbErr> {
        let models = self
            .with_runtime(|conn| async move { feature_config::Entity::find().all(&conn).await })?;

        let configs: Vec<FeatureConfig> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = configs.len(), "Fetched all feature configs");
        Ok(configs)
    }

    #[instrument(level = "debug", skip(self, app_handle))]
    pub fn init_feature_config(&self, app_handle: &tauri::AppHandle) -> Result<(), DbErr> {
        self.add_feature_config(
            app_handle,
            &FeatureConfig {
                id: None,
                feature_code: "conversation_summary".to_string(),
                key: "summary_length".to_string(),
                value: "100".to_string(),
                data_type: "string".to_string(),
                description: Some("对话总结使用长度".to_string()),
            },
        )?;
        self.add_feature_config(
            app_handle,
            &FeatureConfig {
                id: None,
                feature_code: "conversation_summary".to_string(),
                key: "prompt".to_string(),
                value: "请根据提供的大模型问答对话,总结一个简洁明了的标题。标题要求:
 - 字数在5-15个字左右，必须是中文，不要包含标点符号
 - 准确概括对话的核心主题，尽量贴近用户的提问
 - 不要透露任何私人信息
 - 用祈使句或陈述句"
                    .to_string(),
                data_type: "string".to_string(),
                description: Some("对话总结使用长度".to_string()),
            },
        )?;
        debug!("Initialized feature configs");
        Ok(())
    }
}
