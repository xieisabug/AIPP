use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Cond, Expr};
use sea_orm::ExprTrait;
use sea_orm::{
    ActiveValue, DatabaseBackend, DatabaseConnection, DbErr, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArtifactCollection {
    pub id: i64,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub artifact_type: String, // vue, react, html, svg, xml, markdown, mermaid
    pub code: String,
    pub tags: Option<String>, // JSON string for flexible tag storage
    pub created_time: String,
    pub last_used_time: Option<String>,
    pub use_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NewArtifactCollection {
    pub name: String,
    pub icon: String,
    pub description: String,
    pub artifact_type: String,
    pub code: String,
    pub tags: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateArtifactCollection {
    pub id: i64,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub tags: Option<String>,
}

// Reuse SeaORM entity defined under db module
use crate::db::artifacts_db::artifacts_collection as entity;

pub struct ArtifactsDatabase {
    pub conn: DatabaseConnection,
}

impl ArtifactsDatabase {
    #[instrument(level = "debug", skip(app_handle))]
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, DbErr> {
        // 从全局状态获取共享连接，而不是创建新连接
        let conn_arc = crate::db::conn_helper::get_db_conn(app_handle)?;
        let conn = (*conn_arc).clone(); // DatabaseConnection 内部是 Arc，clone 很轻量
        
        tracing::debug!("Acquired shared database connection for Artifacts");
        Ok(Self { conn })
    }

    pub fn create_tables(app_handle: &tauri::AppHandle) -> Result<(), DbErr> {
        use sea_orm::Schema;
        let db = Self::new(app_handle)?;
        let backend = db.conn.get_database_backend();
        let schema = Schema::new(backend);
        // Re-generate SQL per branch to avoid temporary borrow lifetime issues (E0716)
        let sql = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(entity::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(entity::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(entity::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(entity::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        db.with_runtime(|conn| async move {
            conn.execute_unprepared(&sql).await?;
            conn.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_artifacts_collection_type ON artifacts_collection(artifact_type);").await?;
            conn.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_artifacts_collection_name ON artifacts_collection(name);").await?;
            Ok(())
        })?;
        Ok(())
    }

    /// Save a new artifact to collection
    pub fn save_artifact(&self, artifact: NewArtifactCollection) -> Result<i64, DbErr> {
        let model = entity::ActiveModel {
            id: ActiveValue::NotSet,
            name: Set(artifact.name),
            icon: Set(artifact.icon),
            description: Set(artifact.description),
            artifact_type: Set(artifact.artifact_type),
            code: Set(artifact.code),
            tags: Set(artifact.tags),
            created_time: ActiveValue::NotSet,
            last_used_time: ActiveValue::NotSet,
            use_count: ActiveValue::NotSet,
        };
        let res =
            self.with_runtime(|conn| async move { model.insert(&conn).await }).map(|m| m.id)?;
        Ok(res)
    }

    /// Get all artifacts with optional type filter
    pub fn get_artifacts(
        &self,
        artifact_type: Option<&str>,
    ) -> Result<Vec<ArtifactCollection>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            let mut sel = entity::Entity::find();
            if let Some(t) = artifact_type {
                sel = sel.filter(entity::Column::ArtifactType.eq(t));
            }
            sel.order_by_desc(entity::Column::UseCount)
                .order_by_desc(entity::Column::LastUsedTime)
                .order_by_desc(entity::Column::CreatedTime)
                .all(&conn)
                .await
        })?;
        Ok(models.into_iter().map(map_model_to_artifact).collect())
    }

    /// Get artifact by ID
    pub fn get_artifact_by_id(&self, id: i64) -> Result<Option<ArtifactCollection>, DbErr> {
        let model = self
            .with_runtime(|conn| async move { entity::Entity::find_by_id(id).one(&conn).await })?;
        Ok(model.map(map_model_to_artifact))
    }

    /// Search artifacts by name, description, or tags
    pub fn search_artifacts(&self, query: &str) -> Result<Vec<ArtifactCollection>, DbErr> {
        use sea_orm::sea_query::Func;
        let q = query.to_lowercase();
        let models = self.with_runtime(|conn| async move {
            entity::Entity::find()
                .filter(
                    Cond::any()
                        .add(Func::lower(Expr::col(entity::Column::Name)).like(format!("%{}%", q)))
                        .add(
                            Func::lower(Expr::col(entity::Column::Description))
                                .like(format!("%{}%", q)),
                        )
                        .add(Func::lower(Expr::col(entity::Column::Tags)).like(format!("%{}%", q))),
                )
                .order_by_desc(entity::Column::UseCount)
                .order_by_desc(entity::Column::LastUsedTime)
                .order_by_desc(entity::Column::CreatedTime)
                .all(&conn)
                .await
        })?;
        Ok(models.into_iter().map(map_model_to_artifact).collect())
    }

    /// Update artifact metadata (name, icon, description, tags)
    pub fn update_artifact(&self, update: UpdateArtifactCollection) -> Result<(), DbErr> {
        use sea_orm::EntityTrait;
        let model_opt = self.with_runtime(|conn| async move {
            entity::Entity::find_by_id(update.id).one(&conn).await
        })?;
        if let Some(mut model) = model_opt.map(|m| entity::ActiveModel::from(m)) {
            if let Some(name) = update.name {
                model.name = Set(name);
            }
            if let Some(icon) = update.icon {
                model.icon = Set(icon);
            }
            if let Some(description) = update.description {
                model.description = Set(description);
            }
            if let Some(tags) = update.tags {
                model.tags = Set(Some(tags));
            }
            self.with_runtime(|conn| async move { model.update(&conn).await.map(|_| ()) })?;
        }
        Ok(())
    }

    /// Delete artifact by ID
    pub fn delete_artifact(&self, id: i64) -> Result<bool, DbErr> {
        let res = self.with_runtime(|conn| async move {
            entity::Entity::delete_many().filter(entity::Column::Id.eq(id)).exec(&conn).await
        })?;
        Ok(res.rows_affected > 0)
    }

    /// Increment use count and update last used time
    pub fn increment_use_count(&self, id: i64) -> Result<(), DbErr> {
        use sea_orm::sea_query::Expr as SExpr;
        self.with_runtime(|conn| async move {
            entity::Entity::update_many()
                .col_expr(entity::Column::UseCount, SExpr::col(entity::Column::UseCount).add(1))
                .col_expr(
                    entity::Column::LastUsedTime,
                    // set to now
                    Expr::value(Option::<ChronoDateTimeUtc>::Some(chrono::Utc::now().into())),
                )
                .filter(entity::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })
    }

    /// Get artifacts statistics
    pub fn get_statistics(&self) -> Result<(i64, i64), DbErr> {
        // total count
        let total_count = self
            .with_runtime(|conn| async move { entity::Entity::find().count(&conn).await })?
            as i64;
        // manual sum of use_count to avoid custom derive issues if any
        let total_uses = self.with_runtime(|conn| async move {
            let models = entity::Entity::find()
                .select_only()
                .column(entity::Column::UseCount)
                .all(&conn)
                .await?;
            Ok::<i64, DbErr>(models.into_iter().map(|m| m.use_count).sum())
        })?;
        Ok((total_count, total_uses))
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
}

fn fmt_dt(dt: Option<ChronoDateTimeUtc>) -> Option<String> {
    dt.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn map_model_to_artifact(m: entity::Model) -> ArtifactCollection {
    ArtifactCollection {
        id: m.id,
        name: m.name,
        icon: m.icon,
        description: m.description,
        artifact_type: m.artifact_type,
        code: m.code,
        tags: m.tags,
        created_time: fmt_dt(m.created_time).unwrap_or_default(),
        last_used_time: fmt_dt(m.last_used_time),
        use_count: m.use_count,
    }
}

// Removed TotalUse helper; using manual sum approach in get_statistics
