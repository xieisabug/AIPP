use sea_orm::{
    entity::prelude::*, ActiveValue, Database, DatabaseBackend, DatabaseConnection, DbErr, Set,
};
use serde::{Deserialize, Serialize};
use tauri::Manager; // for try_state
use crate::utils::db_utils::build_remote_dsn;
use tracing::{debug, instrument};

use super::get_db_path;

// ============ ArtifactsCollection Entity ============
pub mod artifacts_collection {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "artifacts_collection")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub icon: String,
        #[sea_orm(column_type = "Text", default_value = "")]
        pub description: String,
        pub artifact_type: String, // vue, react, html, svg, xml, markdown, mermaid
        #[sea_orm(column_type = "Text")]
        pub code: String,
        pub tags: Option<String>, // JSON string
        // NOTE: Original schema uses DEFAULT CURRENT_TIMESTAMP; omitted here due to SeaORM macro limitations for default_expr
        pub created_time: Option<ChronoDateTimeUtc>,
        pub last_used_time: Option<ChronoDateTimeUtc>,
        #[sea_orm(default_value = 0)]
        pub use_count: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub struct ArtifactsDatabase {
    pub conn: DatabaseConnection,
}

impl ArtifactsDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "artifacts.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, DbErr> {
        let db_path = get_db_path(app_handle, "artifacts.db").map_err(|e| DbErr::Custom(e))?;
        let mut url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
        if let Some(ds_state) = app_handle.try_state::<crate::DataStorageState>() {
            let flat = ds_state.flat.blocking_lock();
            if let Some((dsn, _)) = build_remote_dsn(&flat) { url = dsn; }
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
        debug!("Opened artifacts database");
        Ok(Self { conn })
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

    // Keep a simple create_tables to mirror legacy schema (indexes & CHECK constraint not auto-generated)
    #[instrument(level = "debug", skip(self))]
    pub fn create_tables(&self) -> Result<(), DbErr> {
        // Create table from Entity to keep column defaults
        use sea_orm::Schema;
        let schema = Schema::new(self.conn.get_database_backend());
        let sql = match self.conn.get_database_backend() {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(artifacts_collection::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(artifacts_collection::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(artifacts_collection::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(artifacts_collection::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        self.with_runtime(|conn| async move {
            conn.execute_unprepared(&sql).await?;
            // Create indexes similar to legacy implementation
            conn.execute_unprepared(
                "CREATE INDEX IF NOT EXISTS idx_artifacts_collection_type ON artifacts_collection(artifact_type);",
            ).await?;
            conn.execute_unprepared(
                "CREATE INDEX IF NOT EXISTS idx_artifacts_collection_name ON artifacts_collection(name);",
            ).await?;
            Ok(())
        })?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(name, artifact_type))]
    pub fn add_artifact(
        &self,
        name: &str,
        icon: &str,
        description: &str,
        artifact_type: &str,
        code: &str,
        tags: Option<String>,
    ) -> Result<i64, DbErr> {
        let name = name.to_string();
        let icon = icon.to_string();
        let description = description.to_string();
        let artifact_type = artifact_type.to_string();
        let code = code.to_string();
        self.with_runtime(|conn| async move {
            let model = artifacts_collection::ActiveModel {
                id: ActiveValue::NotSet,
                name: Set(name),
                icon: Set(icon),
                description: Set(description),
                artifact_type: Set(artifact_type),
                code: Set(code),
                tags: Set(tags),
                created_time: ActiveValue::NotSet,
                last_used_time: ActiveValue::NotSet,
                use_count: ActiveValue::NotSet,
            };
            let res = model.insert(&conn).await?;
            Ok(res.id)
        })
    }
}
