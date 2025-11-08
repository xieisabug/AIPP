use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};
use tauri::Manager; // for try_state
use crate::utils::db_utils::build_remote_dsn;
use sea_orm::{
    entity::prelude::*, Database, DatabaseBackend, DatabaseConnection, DbErr, Set, ActiveValue, QueryOrder,
};
use sea_orm::Schema;
use crate::db::get_db_path;

// ============ Plugins Entity ============
pub mod plugins {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "Plugins")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub plugin_id: i64,
        pub name: String,
        pub version: String,
        pub folder_name: String,
        pub description: Option<String>,
        pub author: Option<String>,
        pub created_at: ChronoDateTimeUtc,
        pub updated_at: ChronoDateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ PluginStatus Entity ============
pub mod plugin_status {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "PluginStatus")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub status_id: i64,
        pub plugin_id: i64,
        pub is_active: i64,
        pub last_run: Option<ChronoDateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ PluginConfigurations Entity ============
pub mod plugin_configurations {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "PluginConfigurations")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub config_id: i64,
        pub plugin_id: i64,
        pub config_key: String,
        pub config_value: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ PluginData Entity ============
pub mod plugin_data {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "PluginData")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub data_id: i64,
        pub plugin_id: i64,
        pub session_id: String,
        pub data_key: String,
        pub data_value: Option<String>,
        pub created_at: ChronoDateTimeUtc,
        pub updated_at: ChronoDateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// Legacy structs for backward compatibility
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Plugin {
    pub plugin_id: i64,
    pub name: String,
    pub version: String,
    pub folder_name: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub created_at: ChronoDateTimeUtc,
    pub updated_at: ChronoDateTimeUtc,
}

impl From<plugins::Model> for Plugin {
    fn from(model: plugins::Model) -> Self {
        Self {
            plugin_id: model.plugin_id,
            name: model.name,
            version: model.version,
            folder_name: model.folder_name,
            description: model.description,
            author: model.author,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginStatus {
    pub status_id: i64,
    pub plugin_id: i64,
    pub is_active: bool,
    pub last_run: Option<ChronoDateTimeUtc>,
}

impl From<plugin_status::Model> for PluginStatus {
    fn from(model: plugin_status::Model) -> Self {
        Self {
            status_id: model.status_id,
            plugin_id: model.plugin_id,
            is_active: model.is_active != 0,
            last_run: model.last_run,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginConfiguration {
    pub config_id: i64,
    pub plugin_id: i64,
    pub config_key: String,
    pub config_value: Option<String>,
}

impl From<plugin_configurations::Model> for PluginConfiguration {
    fn from(model: plugin_configurations::Model) -> Self {
        Self {
            config_id: model.config_id,
            plugin_id: model.plugin_id,
            config_key: model.config_key,
            config_value: model.config_value,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginData {
    pub data_id: i64,
    pub plugin_id: i64,
    pub session_id: String,
    pub data_key: String,
    pub data_value: Option<String>,
    pub created_at: ChronoDateTimeUtc,
    pub updated_at: ChronoDateTimeUtc,
}

impl From<plugin_data::Model> for PluginData {
    fn from(model: plugin_data::Model) -> Self {
        Self {
            data_id: model.data_id,
            plugin_id: model.plugin_id,
            session_id: model.session_id,
            data_key: model.data_key,
            data_value: model.data_value,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

pub struct PluginDatabase {
    pub conn: DatabaseConnection,
}

impl PluginDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "plugin.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, DbErr> {
        let db_path = get_db_path(app_handle, "plugin.db").map_err(|e| DbErr::Custom(e))?;
        let mut url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
        if let Some(ds_state) = app_handle.try_state::<crate::DataStorageState>() {
            let flat = ds_state.flat.blocking_lock();
            if let Some((dsn, _)) = build_remote_dsn(&flat) { url = dsn; }
        }
        
        // Create a new Tokio runtime if we're not in one
        let conn = match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                // We're in a Tokio runtime, use a blocking section to avoid panic
                tokio::task::block_in_place(|| handle.block_on(async { Database::connect(&url).await }))?
            }
            Err(_) => {
                // We're not in a Tokio runtime, create one
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| DbErr::Custom(format!("Failed to create Tokio runtime: {}", e)))?;
                rt.block_on(async { Database::connect(&url).await })?
            }
        };
        
        debug!("Opened plugin database");
        Ok(PluginDatabase { conn })
    }

    #[instrument(level = "debug", skip(self))]
    pub fn create_tables(&self) -> Result<(), DbErr> {
        let backend = self.conn.get_database_backend();
        let schema = Schema::new(backend);
        let sql_plugins = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(plugins::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(plugins::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(plugins::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(plugins::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_plugin_status = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(plugin_status::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(plugin_status::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(plugin_status::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(plugin_status::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_plugin_configurations = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(plugin_configurations::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(plugin_configurations::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(plugin_configurations::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(plugin_configurations::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_plugin_data = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(plugin_data::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(plugin_data::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(plugin_data::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(plugin_data::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };

        self.with_runtime(|conn| async move {
            conn.execute_unprepared(&sql_plugins).await?;
            conn.execute_unprepared(&sql_plugin_status).await?;
            conn.execute_unprepared(&sql_plugin_configurations).await?;
            conn.execute_unprepared(&sql_plugin_data).await?;
            Ok(())
        })
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

    // Plugin CRUD
    #[instrument(level = "debug", skip(self, description, author), fields(name, version, folder_name))]
    pub fn add_plugin(
        &self,
        name: &str,
        version: &str,
        folder_name: &str,
        description: Option<&str>,
        author: Option<&str>,
    ) -> Result<i64, DbErr> {
        let name = name.to_string();
        let version = version.to_string();
        let folder_name = folder_name.to_string();
        let description = description.map(|s| s.to_string());
        let author = author.map(|s| s.to_string());
        let now = chrono::Utc::now();
        
        let id = self.with_runtime(|conn| async move {
            let model = plugins::ActiveModel {
                plugin_id: ActiveValue::NotSet,
                name: Set(name),
                version: Set(version),
                folder_name: Set(folder_name),
                description: Set(description),
                author: Set(author),
                created_at: Set(now),
                updated_at: Set(now),
            };
            let result = model.insert(&conn).await?;
            Ok(result.plugin_id)
        })?;
        
        debug!(plugin_id = id, "Inserted plugin");
        Ok(id)
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_plugins(&self) -> Result<Vec<Plugin>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            plugins::Entity::find()
                .order_by_desc(plugins::Column::CreatedAt)
                .all(&conn)
                .await
        })?;
        
        let plugins: Vec<Plugin> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = plugins.len(), "Fetched plugins");
        Ok(plugins)
    }

    #[instrument(level = "debug", skip(self), fields(plugin_id))]
    pub fn get_plugin(&self, plugin_id: i64) -> Result<Option<Plugin>, DbErr> {
        let result = self.with_runtime(|conn| async move {
            plugins::Entity::find_by_id(plugin_id)
                .one(&conn)
                .await
        })?;
        
        let plugin = result.map(|m| m.into());
        debug!(found = plugin.is_some(), "Fetched plugin by id");
        Ok(plugin)
    }

    #[instrument(level = "debug", skip(self, plugin), fields(plugin_id = plugin.plugin_id))]
    pub fn update_plugin(&self, plugin: &Plugin) -> Result<(), DbErr> {
        let plugin_id = plugin.plugin_id;
        let name = plugin.name.clone();
        let version = plugin.version.clone();
        let folder_name = plugin.folder_name.clone();
        let description = plugin.description.clone();
        let author = plugin.author.clone();
        let updated_at = plugin.updated_at;
        
        self.with_runtime(|conn| async move {
            plugins::Entity::update_many()
                .col_expr(plugins::Column::Name, Expr::value(name))
                .col_expr(plugins::Column::Version, Expr::value(version))
                .col_expr(plugins::Column::FolderName, Expr::value(folder_name))
                .col_expr(plugins::Column::Description, Expr::value(description))
                .col_expr(plugins::Column::Author, Expr::value(author))
                .col_expr(plugins::Column::UpdatedAt, Expr::value(updated_at))
                .filter(plugins::Column::PluginId.eq(plugin_id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;
        
        debug!("Updated plugin");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(plugin_id))]
    pub fn delete_plugin(&self, plugin_id: i64) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            plugins::Entity::delete_many()
                .filter(plugins::Column::PluginId.eq(plugin_id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;
        
        debug!("Deleted plugin");
        Ok(())
    }

    // PluginStatus helpers
    #[instrument(level = "debug", skip(self), fields(plugin_id))]
    pub fn get_plugin_status(&self, plugin_id: i64) -> Result<Option<PluginStatus>, DbErr> {
        let result = self.with_runtime(|conn| async move {
            plugin_status::Entity::find()
                .filter(plugin_status::Column::PluginId.eq(plugin_id))
                .one(&conn)
                .await
        })?;
        
        let status = result.map(|m| m.into());
        debug!(found = status.is_some(), "Fetched plugin status");
        Ok(status)
    }

    #[instrument(level = "debug", skip(self, last_run), fields(plugin_id, is_active))]
    pub fn upsert_plugin_status(
        &self,
        plugin_id: i64,
        is_active: bool,
        last_run: Option<ChronoDateTimeUtc>,
    ) -> Result<i64, DbErr> {
        let existing = self.get_plugin_status(plugin_id)?;
        let is_active_int = if is_active { 1 } else { 0 };
        
        if let Some(status) = existing {
            let status_id = status.status_id;
            
            self.with_runtime(|conn| async move {
                plugin_status::Entity::update_many()
                    .col_expr(plugin_status::Column::IsActive, Expr::value(is_active_int))
                    .col_expr(plugin_status::Column::LastRun, Expr::value(last_run))
                    .filter(plugin_status::Column::StatusId.eq(status_id))
                    .exec(&conn)
                    .await?;
                Ok(())
            })?;
            
            debug!(status_id = status_id, "Updated plugin status");
            Ok(status_id)
        } else {
            let id = self.with_runtime(|conn| async move {
                let model = plugin_status::ActiveModel {
                    status_id: ActiveValue::NotSet,
                    plugin_id: Set(plugin_id),
                    is_active: Set(is_active_int),
                    last_run: Set(last_run),
                };
                let result = model.insert(&conn).await?;
                Ok(result.status_id)
            })?;
            
            debug!(status_id = id, "Inserted plugin status");
            Ok(id)
        }
    }

    #[instrument(level = "debug", skip(self), fields(status_id))]
    pub fn delete_plugin_status(&self, status_id: i64) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            plugin_status::Entity::delete_many()
                .filter(plugin_status::Column::StatusId.eq(status_id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;
        
        debug!("Deleted plugin status");
        Ok(())
    }

    // Configurations
    #[instrument(level = "debug", skip(self), fields(plugin_id))]
    pub fn get_plugin_configurations(&self, plugin_id: i64) -> Result<Vec<PluginConfiguration>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            plugin_configurations::Entity::find()
                .filter(plugin_configurations::Column::PluginId.eq(plugin_id))
                .all(&conn)
                .await
        })?;
        
        let configs: Vec<PluginConfiguration> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = configs.len(), "Fetched plugin configs");
        Ok(configs)
    }

    #[instrument(level = "debug", skip(self, config_value), fields(plugin_id, config_key))]
    pub fn set_plugin_configuration(
        &self,
        plugin_id: i64,
        config_key: &str,
        config_value: Option<&str>,
    ) -> Result<i64, DbErr> {
        let config_key_str = config_key.to_string();
        let config_value_opt = config_value.map(|s| s.to_string());
        
        // Check if exists
        let config_key_for_query = config_key_str.clone();
        let existing = self.with_runtime(|conn| async move {
            plugin_configurations::Entity::find()
                .filter(plugin_configurations::Column::PluginId.eq(plugin_id))
                .filter(plugin_configurations::Column::ConfigKey.eq(config_key_for_query))
                .one(&conn)
                .await
        })?;
        
        if let Some(model) = existing {
            let config_id = model.config_id;
            let config_value_clone = config_value_opt.clone();
            
            self.with_runtime(|conn| async move {
                plugin_configurations::Entity::update_many()
                    .col_expr(plugin_configurations::Column::ConfigValue, Expr::value(config_value_clone))
                    .filter(plugin_configurations::Column::ConfigId.eq(config_id))
                    .exec(&conn)
                    .await?;
                Ok(())
            })?;
            
            debug!(config_id = config_id, "Updated plugin config");
            Ok(config_id)
        } else {
            let id = self.with_runtime(|conn| async move {
                let model = plugin_configurations::ActiveModel {
                    config_id: ActiveValue::NotSet,
                    plugin_id: Set(plugin_id),
                    config_key: Set(config_key_str),
                    config_value: Set(config_value_opt),
                };
                let result = model.insert(&conn).await?;
                Ok(result.config_id)
            })?;
            
            debug!(config_id = id, "Inserted plugin config");
            Ok(id)
        }
    }

    #[instrument(level = "debug", skip(self), fields(config_id))]
    pub fn delete_plugin_configuration(&self, config_id: i64) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            plugin_configurations::Entity::delete_many()
                .filter(plugin_configurations::Column::ConfigId.eq(config_id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;
        
        debug!("Deleted plugin config");
        Ok(())
    }

    // Data
    #[instrument(level = "debug", skip(self), fields(plugin_id, session_id))]
    pub fn get_plugin_data_by_session(
        &self,
        plugin_id: i64,
        session_id: &str,
    ) -> Result<Vec<PluginData>, DbErr> {
        let session_id = session_id.to_string();
        
        let models = self.with_runtime(|conn| async move {
            plugin_data::Entity::find()
                .filter(plugin_data::Column::PluginId.eq(plugin_id))
                .filter(plugin_data::Column::SessionId.eq(session_id))
                .all(&conn)
                .await
        })?;
        
        let data: Vec<PluginData> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = data.len(), "Fetched plugin data by session");
        Ok(data)
    }

    #[instrument(level = "debug", skip(self, data), fields(plugin_id = data.plugin_id, session_id = %data.session_id, data_key = %data.data_key))]
    pub fn add_plugin_data(&self, data: &PluginData) -> Result<i64, DbErr> {
        let plugin_id = data.plugin_id;
        let session_id = data.session_id.clone();
        let data_key = data.data_key.clone();
        let data_value = data.data_value.clone();
        let created_at = data.created_at;
        let updated_at = data.updated_at;
        
        let id = self.with_runtime(|conn| async move {
            let model = plugin_data::ActiveModel {
                data_id: ActiveValue::NotSet,
                plugin_id: Set(plugin_id),
                session_id: Set(session_id),
                data_key: Set(data_key),
                data_value: Set(data_value),
                created_at: Set(created_at),
                updated_at: Set(updated_at),
            };
            let result = model.insert(&conn).await?;
            Ok(result.data_id)
        })?;
        
        debug!(data_id = id, "Inserted plugin data");
        Ok(id)
    }

    #[instrument(level = "debug", skip(self, data_value), fields(data_id))]
    pub fn update_plugin_data(&self, data_id: i64, data_value: Option<&str>, updated_at: ChronoDateTimeUtc) -> Result<(), DbErr> {
        let data_value = data_value.map(|s| s.to_string());
        
        self.with_runtime(|conn| async move {
            plugin_data::Entity::update_many()
                .col_expr(plugin_data::Column::DataValue, Expr::value(data_value))
                .col_expr(plugin_data::Column::UpdatedAt, Expr::value(updated_at))
                .filter(plugin_data::Column::DataId.eq(data_id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;
        
        debug!("Updated plugin data");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(data_id))]
    pub fn delete_plugin_data(&self, data_id: i64) -> Result<(), DbErr> {
        self.with_runtime(|conn| async move {
            plugin_data::Entity::delete_many()
                .filter(plugin_data::Column::DataId.eq(data_id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;
        
        debug!("Deleted plugin data");
        Ok(())
    }
}
