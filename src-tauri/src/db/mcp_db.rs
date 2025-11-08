use crate::db::get_db_path;
use sea_orm::prelude::Expr;
use sea_orm::{
    entity::prelude::*, ActiveValue, Database, DatabaseBackend, DatabaseConnection, DbErr, QueryOrder, Set,
};
use sea_orm::Schema;
use tauri::Manager; // for try_state
use crate::utils::db_utils::build_remote_dsn;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

// ============ MCPServer Entity ============
pub mod mcp_server {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "mcp_server")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(unique)]
        pub name: String,
        pub description: String,
        pub transport_type: String,
        pub command: Option<String>,
        pub environment_variables: Option<String>,
        pub headers: Option<String>,
        pub url: Option<String>,
        pub timeout: Option<i32>,
        pub is_long_running: bool,
        pub is_enabled: bool,
        pub is_builtin: bool,
        pub created_time: Option<ChronoDateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ MCPServerTool Entity ============
pub mod mcp_server_tool {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "mcp_server_tool")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub server_id: i64,
        pub tool_name: String,
        pub tool_description: Option<String>,
        pub is_enabled: bool,
        pub is_auto_run: bool,
        pub parameters: Option<String>,
        pub created_time: Option<ChronoDateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ MCPServerResource Entity ============
pub mod mcp_server_resource {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "mcp_server_resource")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub server_id: i64,
        pub resource_uri: String,
        pub resource_name: String,
        pub resource_type: String,
        pub resource_description: Option<String>,
        pub created_time: Option<ChronoDateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ MCPServerPrompt Entity ============
pub mod mcp_server_prompt {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "mcp_server_prompt")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub server_id: i64,
        pub prompt_name: String,
        pub prompt_description: Option<String>,
        pub is_enabled: bool,
        pub arguments: Option<String>,
        pub created_time: Option<ChronoDateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ============ MCPToolCall Entity ============
pub mod mcp_tool_call {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "mcp_tool_call")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub conversation_id: i64,
        pub message_id: Option<i64>,
        pub server_id: i64,
        pub server_name: String,
        pub tool_name: String,
        pub parameters: String,
        pub status: String,
        pub result: Option<String>,
        pub error: Option<String>,
        pub created_time: Option<ChronoDateTimeUtc>,
        pub started_time: Option<ChronoDateTimeUtc>,
        pub finished_time: Option<ChronoDateTimeUtc>,
        pub llm_call_id: Option<String>,
        pub assistant_message_id: Option<i64>,
        pub subtask_id: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// Legacy structs for backward compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServer {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub transport_type: String,
    pub command: Option<String>,
    pub environment_variables: Option<String>,
    pub headers: Option<String>,
    pub url: Option<String>,
    pub timeout: Option<i32>,
    pub is_long_running: bool,
    pub is_enabled: bool,
    pub is_builtin: bool,
    pub created_time: String,
}

impl From<mcp_server::Model> for MCPServer {
    fn from(model: mcp_server::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            description: model.description,
            transport_type: model.transport_type,
            command: model.command,
            environment_variables: model.environment_variables,
            headers: model.headers,
            url: model.url,
            timeout: model.timeout,
            is_long_running: model.is_long_running,
            is_enabled: model.is_enabled,
            is_builtin: model.is_builtin,
            created_time: model
                .created_time
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServerTool {
    pub id: i64,
    pub server_id: i64,
    pub tool_name: String,
    pub tool_description: Option<String>,
    pub is_enabled: bool,
    pub is_auto_run: bool,
    pub parameters: Option<String>,
}

impl From<mcp_server_tool::Model> for MCPServerTool {
    fn from(model: mcp_server_tool::Model) -> Self {
        Self {
            id: model.id,
            server_id: model.server_id,
            tool_name: model.tool_name,
            tool_description: model.tool_description,
            is_enabled: model.is_enabled,
            is_auto_run: model.is_auto_run,
            parameters: model.parameters,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPServerResource {
    pub id: i64,
    pub server_id: i64,
    pub resource_uri: String,
    pub resource_name: String,
    pub resource_type: String,
    pub resource_description: Option<String>,
}

impl From<mcp_server_resource::Model> for MCPServerResource {
    fn from(model: mcp_server_resource::Model) -> Self {
        Self {
            id: model.id,
            server_id: model.server_id,
            resource_uri: model.resource_uri,
            resource_name: model.resource_name,
            resource_type: model.resource_type,
            resource_description: model.resource_description,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPServerPrompt {
    pub id: i64,
    pub server_id: i64,
    pub prompt_name: String,
    pub prompt_description: Option<String>,
    pub is_enabled: bool,
    pub arguments: Option<String>,
}

impl From<mcp_server_prompt::Model> for MCPServerPrompt {
    fn from(model: mcp_server_prompt::Model) -> Self {
        Self {
            id: model.id,
            server_id: model.server_id,
            prompt_name: model.prompt_name,
            prompt_description: model.prompt_description,
            is_enabled: model.is_enabled,
            arguments: model.arguments,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolCall {
    pub id: i64,
    pub conversation_id: i64,
    pub message_id: Option<i64>,
    pub server_id: i64,
    pub server_name: String,
    pub tool_name: String,
    pub parameters: String,
    pub status: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_time: String,
    pub started_time: Option<String>,
    pub finished_time: Option<String>,
    pub llm_call_id: Option<String>,
    pub assistant_message_id: Option<i64>,
}

impl From<mcp_tool_call::Model> for MCPToolCall {
    fn from(model: mcp_tool_call::Model) -> Self {
        Self {
            id: model.id,
            conversation_id: model.conversation_id,
            message_id: model.message_id,
            server_id: model.server_id,
            server_name: model.server_name,
            tool_name: model.tool_name,
            parameters: model.parameters,
            status: model.status,
            result: model.result,
            error: model.error,
            created_time: model
                .created_time
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default(),
            started_time: model.started_time.map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string()),
            finished_time: model.finished_time.map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string()),
            llm_call_id: model.llm_call_id,
            assistant_message_id: model.assistant_message_id,
        }
    }
}

pub struct MCPDatabase {
    pub conn: DatabaseConnection,
}

impl MCPDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "mcp.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, DbErr> {
        let db_path = get_db_path(app_handle, "mcp.db").map_err(|e| DbErr::Custom(e))?;
        let mut url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
        if let Some(ds_state) = app_handle.try_state::<crate::DataStorageState>() {
            let flat = match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    tokio::task::block_in_place(|| handle.block_on(async {
                        ds_state.flat.lock().await.clone()
                    }))
                }
                Err(_) => {
                    let rt = tokio::runtime::Runtime::new()
                        .map_err(|e| DbErr::Custom(format!("Failed to create Tokio runtime: {}", e)))?;
                    rt.block_on(async { ds_state.flat.lock().await.clone() })
                }
            };
            if let Some((dsn, backend)) = build_remote_dsn(&flat) {
                url = dsn;
                match backend {
                    DatabaseBackend::Postgres => debug!("MCP DB using remote PostgreSQL"),
                    DatabaseBackend::MySql => debug!("MCP DB using remote MySQL"),
                    _ => debug!("MCP DB using local SQLite"),
                }
            } else {
                debug!("MCP DB using local SQLite (no remote config or incomplete)");
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

        debug!("Opened MCP database");
        Ok(MCPDatabase { conn })
    }

    #[instrument(level = "debug", skip(self))]
    pub fn create_tables(&self) -> Result<(), DbErr> {
        let backend = self.conn.get_database_backend();
        let schema = Schema::new(backend);
        let sql_mcp_server = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(mcp_server::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(mcp_server::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(mcp_server::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(mcp_server::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_mcp_server_tool = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(mcp_server_tool::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(mcp_server_tool::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(mcp_server_tool::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(mcp_server_tool::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_mcp_server_resource = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(mcp_server_resource::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(mcp_server_resource::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(mcp_server_resource::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(mcp_server_resource::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_mcp_server_prompt = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(mcp_server_prompt::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(mcp_server_prompt::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(mcp_server_prompt::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(mcp_server_prompt::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };
        let sql_mcp_tool_call = match backend {
            DatabaseBackend::Sqlite => schema
                .create_table_from_entity(mcp_tool_call::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
            DatabaseBackend::Postgres => schema
                .create_table_from_entity(mcp_tool_call::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::PostgresQueryBuilder),
            DatabaseBackend::MySql => schema
                .create_table_from_entity(mcp_tool_call::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::MysqlQueryBuilder),
            _ => schema
                .create_table_from_entity(mcp_tool_call::Entity)
                .if_not_exists()
                .to_string(sea_orm::sea_query::SqliteQueryBuilder),
        };

        self.with_runtime(|conn| async move {
            conn.execute_unprepared(&sql_mcp_server).await?;
            conn.execute_unprepared(&sql_mcp_server_tool).await?;
            conn.execute_unprepared(&sql_mcp_server_resource).await?;
            conn.execute_unprepared(&sql_mcp_server_prompt).await?;
            conn.execute_unprepared(&sql_mcp_tool_call).await?;
            // Composite uniques via indexes
            conn.execute_unprepared("CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_server_tool_unique ON mcp_server_tool(server_id, tool_name)").await?;
            conn.execute_unprepared("CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_server_resource_unique ON mcp_server_resource(server_id, resource_uri)").await?;
            conn.execute_unprepared("CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_server_prompt_unique ON mcp_server_prompt(server_id, prompt_name)").await?;
            Ok(())
        })?;

        debug!("Created MCP tables");
        Ok(())
    }

    // Legacy migration helpers removed in favor of initial schema creation via SeaORM

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

    #[instrument(level = "debug", skip(self))]
    pub fn get_mcp_servers(&self) -> Result<Vec<MCPServer>, DbErr> {
        debug!("query mcp servers");
        let models = self.with_runtime(|conn| async move {
            mcp_server::Entity::find()
                .order_by_desc(mcp_server::Column::CreatedTime)
                .all(&conn)
                .await
        })?;

        let servers: Vec<MCPServer> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = servers.len(), "Fetched MCP servers");
        Ok(servers)
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn get_mcp_server(&self, id: i64) -> Result<MCPServer, DbErr> {
        debug!(id, "query single mcp server");
        let model = self.with_runtime(|conn| async move {
            mcp_server::Entity::find_by_id(id).one(&conn).await
        })?;

        match model {
            Some(m) => {
                debug!(found = true, "Fetched MCP server");
                Ok(m.into())
            }
            None => {
                debug!(found = false, "MCP server not found");
                Err(DbErr::RecordNotFound("MCP server not found".to_string()))
            }
        }
    }

    #[instrument(
        level = "debug",
        skip(self),
        fields(id, name, transport_type, is_enabled, is_builtin)
    )]
    pub fn update_mcp_server_with_builtin(
        &self,
        id: i64,
        name: &str,
        description: Option<&str>,
        transport_type: &str,
        command: Option<&str>,
        environment_variables: Option<&str>,
        headers: Option<&str>,
        url: Option<&str>,
        timeout: Option<i32>,
        is_long_running: bool,
        is_enabled: bool,
        is_builtin: bool,
    ) -> Result<(), DbErr> {
        debug!(id, name, transport_type, is_enabled, is_builtin, "update mcp server");
        let name = name.to_string();
        let description = description.map(|s| s.to_string()).unwrap_or_default();
        let transport_type = transport_type.to_string();
        let command = command.map(|s| s.to_string());
        let environment_variables = environment_variables.map(|s| s.to_string());
        let headers = headers.map(|s| s.to_string());
        let url = url.map(|s| s.to_string());

        self.with_runtime(|conn| async move {
            mcp_server::Entity::update_many()
                .col_expr(mcp_server::Column::Name, Expr::value(name))
                .col_expr(mcp_server::Column::Description, Expr::value(description))
                .col_expr(mcp_server::Column::TransportType, Expr::value(transport_type))
                .col_expr(mcp_server::Column::Command, Expr::value(command))
                .col_expr(
                    mcp_server::Column::EnvironmentVariables,
                    Expr::value(environment_variables),
                )
                .col_expr(mcp_server::Column::Headers, Expr::value(headers))
                .col_expr(mcp_server::Column::Url, Expr::value(url))
                .col_expr(mcp_server::Column::Timeout, Expr::value(timeout))
                .col_expr(mcp_server::Column::IsLongRunning, Expr::value(is_long_running))
                .col_expr(mcp_server::Column::IsEnabled, Expr::value(is_enabled))
                .col_expr(mcp_server::Column::IsBuiltin, Expr::value(is_builtin))
                .filter(mcp_server::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated MCP server");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn delete_mcp_server(&self, id: i64) -> Result<(), DbErr> {
        debug!(id, "delete mcp server");
        self.with_runtime(|conn| async move {
            mcp_server::Entity::delete_many()
                .filter(mcp_server::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Deleted MCP server");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id, is_enabled))]
    pub fn toggle_mcp_server(&self, id: i64, is_enabled: bool) -> Result<(), DbErr> {
        debug!(id, is_enabled, "toggle mcp server");
        self.with_runtime(|conn| async move {
            mcp_server::Entity::update_many()
                .col_expr(mcp_server::Column::IsEnabled, Expr::value(is_enabled))
                .filter(mcp_server::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Toggled MCP server");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(name, transport_type, is_enabled, is_builtin))]
    pub fn upsert_mcp_server_with_builtin(
        &self,
        name: &str,
        description: Option<&str>,
        transport_type: &str,
        command: Option<&str>,
        environment_variables: Option<&str>,
        headers: Option<&str>,
        url: Option<&str>,
        timeout: Option<i32>,
        is_long_running: bool,
        is_enabled: bool,
        is_builtin: bool,
    ) -> Result<i64, DbErr> {
        debug!(name, transport_type, is_enabled, is_builtin, "upsert mcp server");
        let name_clone = name.to_string();
        let description_str = description.map(|s| s.to_string()).unwrap_or_default();
        let transport_type_str = transport_type.to_string();
        let command_opt = command.map(|s| s.to_string());
        let env_vars_opt = environment_variables.map(|s| s.to_string());
        let headers_opt = headers.map(|s| s.to_string());
        let url_opt = url.map(|s| s.to_string());

        // First try to get existing server by name
        let existing = self.with_runtime(|conn| async move {
            mcp_server::Entity::find()
                .filter(mcp_server::Column::Name.eq(name_clone))
                .one(&conn)
                .await
        })?;

        match existing {
            Some(model) => {
                let id = model.id;
                // Update existing server
                self.update_mcp_server_with_builtin(
                    id,
                    name,
                    description,
                    transport_type,
                    command,
                    environment_variables,
                    headers,
                    url,
                    timeout,
                    is_long_running,
                    is_enabled,
                    is_builtin,
                )?;
                debug!(id, "Updated existing MCP server");
                Ok(id)
            }
            None => {
                // Insert new server
                let model = mcp_server::ActiveModel {
                    id: ActiveValue::NotSet,
                    name: Set(name.to_string()),
                    description: Set(description_str),
                    transport_type: Set(transport_type_str),
                    command: Set(command_opt),
                    environment_variables: Set(env_vars_opt),
                    headers: Set(headers_opt),
                    url: Set(url_opt),
                    timeout: Set(timeout),
                    is_long_running: Set(is_long_running),
                    is_enabled: Set(is_enabled),
                    is_builtin: Set(is_builtin),
                    created_time: ActiveValue::NotSet,
                };

                let result = self.with_runtime(|conn| async move { model.insert(&conn).await })?;

                debug!(id = result.id, "Inserted new MCP server");
                Ok(result.id)
            }
        }
    }

    #[instrument(level = "debug", skip(self), fields(server_id))]
    pub fn get_mcp_server_tools(&self, server_id: i64) -> Result<Vec<MCPServerTool>, DbErr> {
        debug!(server_id, "query mcp server tools");
        let models = self.with_runtime(|conn| async move {
            mcp_server_tool::Entity::find()
                .filter(mcp_server_tool::Column::ServerId.eq(server_id))
                .order_by_asc(mcp_server_tool::Column::ToolName)
                .all(&conn)
                .await
        })?;

        let tools: Vec<MCPServerTool> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = tools.len(), "Fetched MCP server tools");
        Ok(tools)
    }

    #[instrument(level = "debug", skip(self), fields(id, is_enabled, is_auto_run))]
    pub fn update_mcp_server_tool(
        &self,
        id: i64,
        is_enabled: bool,
        is_auto_run: bool,
    ) -> Result<(), DbErr> {
        debug!(id, is_enabled, is_auto_run, "update mcp server tool flags");
        self.with_runtime(|conn| async move {
            mcp_server_tool::Entity::update_many()
                .col_expr(mcp_server_tool::Column::IsEnabled, Expr::value(is_enabled))
                .col_expr(mcp_server_tool::Column::IsAutoRun, Expr::value(is_auto_run))
                .filter(mcp_server_tool::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated MCP server tool");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(server_id, tool_name))]
    pub fn upsert_mcp_server_tool(
        &self,
        server_id: i64,
        tool_name: &str,
        tool_description: Option<&str>,
        parameters: Option<&str>,
    ) -> Result<i64, DbErr> {
        debug!(server_id, tool_name, "upsert mcp server tool");
        let tool_name_clone = tool_name.to_string();
        let tool_desc_opt = tool_description.map(|s| s.to_string());
        let params_opt = parameters.map(|s| s.to_string());

        // First try to get existing tool by server_id and tool_name
        let existing = self.with_runtime(|conn| async move {
            mcp_server_tool::Entity::find()
                .filter(mcp_server_tool::Column::ServerId.eq(server_id))
                .filter(mcp_server_tool::Column::ToolName.eq(tool_name_clone))
                .one(&conn)
                .await
        })?;

        match existing {
            Some(model) => {
                let id = model.id;
                // Update existing tool, preserve user settings
                let tool_desc_opt2 = tool_description.map(|s| s.to_string());
                let params_opt2 = parameters.map(|s| s.to_string());

                self.with_runtime(|conn| async move {
                    mcp_server_tool::Entity::update_many()
                        .col_expr(
                            mcp_server_tool::Column::ToolDescription,
                            Expr::value(tool_desc_opt2),
                        )
                        .col_expr(mcp_server_tool::Column::Parameters, Expr::value(params_opt2))
                        .filter(mcp_server_tool::Column::Id.eq(id))
                        .exec(&conn)
                        .await?;
                    Ok(())
                })?;

                debug!(id, "Updated existing MCP server tool");
                Ok(id)
            }
            None => {
                // Insert new tool with default settings
                let model = mcp_server_tool::ActiveModel {
                    id: ActiveValue::NotSet,
                    server_id: Set(server_id),
                    tool_name: Set(tool_name.to_string()),
                    tool_description: Set(tool_desc_opt),
                    is_enabled: Set(true),
                    is_auto_run: Set(false),
                    parameters: Set(params_opt),
                    created_time: ActiveValue::NotSet,
                };

                let result = self.with_runtime(|conn| async move { model.insert(&conn).await })?;

                debug!(id = result.id, "Inserted new MCP server tool");
                Ok(result.id)
            }
        }
    }

    #[instrument(level = "debug", skip(self), fields(server_id))]
    pub fn get_mcp_server_resources(
        &self,
        server_id: i64,
    ) -> Result<Vec<MCPServerResource>, DbErr> {
        debug!(server_id, "query mcp server resources");
        let models = self.with_runtime(|conn| async move {
            mcp_server_resource::Entity::find()
                .filter(mcp_server_resource::Column::ServerId.eq(server_id))
                .order_by_asc(mcp_server_resource::Column::ResourceName)
                .all(&conn)
                .await
        })?;

        let resources: Vec<MCPServerResource> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = resources.len(), "Fetched MCP server resources");
        Ok(resources)
    }

    #[instrument(level = "debug", skip(self), fields(server_id, resource_uri))]
    pub fn upsert_mcp_server_resource(
        &self,
        server_id: i64,
        resource_uri: &str,
        resource_name: &str,
        resource_type: &str,
        resource_description: Option<&str>,
    ) -> Result<i64, DbErr> {
        debug!(server_id, resource_uri, "upsert mcp server resource");
        let resource_uri_clone = resource_uri.to_string();
        let resource_name_str = resource_name.to_string();
        let resource_type_str = resource_type.to_string();
        let resource_desc_opt = resource_description.map(|s| s.to_string());

        // First try to get existing resource by server_id and resource_uri
        let existing = self.with_runtime(|conn| async move {
            mcp_server_resource::Entity::find()
                .filter(mcp_server_resource::Column::ServerId.eq(server_id))
                .filter(mcp_server_resource::Column::ResourceUri.eq(resource_uri_clone))
                .one(&conn)
                .await
        })?;

        match existing {
            Some(model) => {
                let id = model.id;
                // Update existing resource
                let resource_name_str2 = resource_name.to_string();
                let resource_type_str2 = resource_type.to_string();
                let resource_desc_opt2 = resource_description.map(|s| s.to_string());

                self.with_runtime(|conn| async move {
                    mcp_server_resource::Entity::update_many()
                        .col_expr(
                            mcp_server_resource::Column::ResourceName,
                            Expr::value(resource_name_str2),
                        )
                        .col_expr(
                            mcp_server_resource::Column::ResourceType,
                            Expr::value(resource_type_str2),
                        )
                        .col_expr(
                            mcp_server_resource::Column::ResourceDescription,
                            Expr::value(resource_desc_opt2),
                        )
                        .filter(mcp_server_resource::Column::Id.eq(id))
                        .exec(&conn)
                        .await?;
                    Ok(())
                })?;

                debug!(id, "Updated existing MCP server resource");
                Ok(id)
            }
            None => {
                // Insert new resource
                let model = mcp_server_resource::ActiveModel {
                    id: ActiveValue::NotSet,
                    server_id: Set(server_id),
                    resource_uri: Set(resource_uri.to_string()),
                    resource_name: Set(resource_name_str),
                    resource_type: Set(resource_type_str),
                    resource_description: Set(resource_desc_opt),
                    created_time: ActiveValue::NotSet,
                };

                let result = self.with_runtime(|conn| async move { model.insert(&conn).await })?;

                debug!(id = result.id, "Inserted new MCP server resource");
                Ok(result.id)
            }
        }
    }

    #[instrument(level = "debug", skip(self), fields(server_id))]
    pub fn get_mcp_server_prompts(&self, server_id: i64) -> Result<Vec<MCPServerPrompt>, DbErr> {
        debug!(server_id, "query mcp server prompts");
        let models = self.with_runtime(|conn| async move {
            mcp_server_prompt::Entity::find()
                .filter(mcp_server_prompt::Column::ServerId.eq(server_id))
                .order_by_asc(mcp_server_prompt::Column::PromptName)
                .all(&conn)
                .await
        })?;

        let prompts: Vec<MCPServerPrompt> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = prompts.len(), "Fetched MCP server prompts");
        Ok(prompts)
    }

    #[instrument(level = "debug", skip(self), fields(id, is_enabled))]
    pub fn update_mcp_server_prompt(&self, id: i64, is_enabled: bool) -> Result<(), DbErr> {
        debug!(id, is_enabled, "update mcp server prompt");
        self.with_runtime(|conn| async move {
            mcp_server_prompt::Entity::update_many()
                .col_expr(mcp_server_prompt::Column::IsEnabled, Expr::value(is_enabled))
                .filter(mcp_server_prompt::Column::Id.eq(id))
                .exec(&conn)
                .await?;
            Ok(())
        })?;

        debug!("Updated MCP server prompt");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(server_id, prompt_name))]
    pub fn upsert_mcp_server_prompt(
        &self,
        server_id: i64,
        prompt_name: &str,
        prompt_description: Option<&str>,
        arguments: Option<&str>,
    ) -> Result<i64, DbErr> {
        debug!(server_id, prompt_name, "upsert mcp server prompt");
        let prompt_name_clone = prompt_name.to_string();
        let prompt_desc_opt = prompt_description.map(|s| s.to_string());
        let args_opt = arguments.map(|s| s.to_string());

        // First try to get existing prompt by server_id and prompt_name
        let existing = self.with_runtime(|conn| async move {
            mcp_server_prompt::Entity::find()
                .filter(mcp_server_prompt::Column::ServerId.eq(server_id))
                .filter(mcp_server_prompt::Column::PromptName.eq(prompt_name_clone))
                .one(&conn)
                .await
        })?;

        match existing {
            Some(model) => {
                let id = model.id;
                // Update existing prompt, preserve user settings
                let prompt_desc_opt2 = prompt_description.map(|s| s.to_string());
                let args_opt2 = arguments.map(|s| s.to_string());

                self.with_runtime(|conn| async move {
                    mcp_server_prompt::Entity::update_many()
                        .col_expr(
                            mcp_server_prompt::Column::PromptDescription,
                            Expr::value(prompt_desc_opt2),
                        )
                        .col_expr(mcp_server_prompt::Column::Arguments, Expr::value(args_opt2))
                        .filter(mcp_server_prompt::Column::Id.eq(id))
                        .exec(&conn)
                        .await?;
                    Ok(())
                })?;

                debug!(id, "Updated existing MCP server prompt");
                Ok(id)
            }
            None => {
                // Insert new prompt with default settings
                let model = mcp_server_prompt::ActiveModel {
                    id: ActiveValue::NotSet,
                    server_id: Set(server_id),
                    prompt_name: Set(prompt_name.to_string()),
                    prompt_description: Set(prompt_desc_opt),
                    is_enabled: Set(true),
                    arguments: Set(args_opt),
                    created_time: ActiveValue::NotSet,
                };

                let result = self.with_runtime(|conn| async move { model.insert(&conn).await })?;

                debug!(id = result.id, "Inserted new MCP server prompt");
                Ok(result.id)
            }
        }
    }

    // MCP Tool Call methods
    #[instrument(level = "debug", skip(self), fields(conversation_id, server_id, tool_name))]
    pub fn create_mcp_tool_call(
        &self,
        conversation_id: i64,
        message_id: Option<i64>,
        server_id: i64,
        server_name: &str,
        tool_name: &str,
        parameters: &str,
    ) -> Result<MCPToolCall, DbErr> {
        debug!(conversation_id, server_id, tool_name, "create mcp tool call");
        let model = mcp_tool_call::ActiveModel {
            id: ActiveValue::NotSet,
            conversation_id: Set(conversation_id),
            message_id: Set(message_id),
            server_id: Set(server_id),
            server_name: Set(server_name.to_string()),
            tool_name: Set(tool_name.to_string()),
            parameters: Set(parameters.to_string()),
            status: Set("pending".to_string()),
            result: Set(None),
            error: Set(None),
            created_time: ActiveValue::NotSet,
            started_time: Set(None),
            finished_time: Set(None),
            llm_call_id: Set(None),
            assistant_message_id: Set(None),
            subtask_id: Set(None),
        };

        let result = self.with_runtime(|conn| async move { model.insert(&conn).await })?;

        debug!(id = result.id, "Created MCP tool call");
        self.get_mcp_tool_call(result.id)
    }

    #[instrument(
        level = "debug",
        skip(self),
        fields(conversation_id, server_id, tool_name, llm_call_id, assistant_message_id)
    )]
    pub fn create_mcp_tool_call_with_llm_id(
        &self,
        conversation_id: i64,
        message_id: Option<i64>,
        server_id: i64,
        server_name: &str,
        tool_name: &str,
        parameters: &str,
        llm_call_id: Option<&str>,
        assistant_message_id: Option<i64>,
    ) -> Result<MCPToolCall, DbErr> {
        debug!(conversation_id, server_id, tool_name, llm_call_id = ?llm_call_id, assistant_message_id = ?assistant_message_id, "create mcp tool call with llm id");
        let model = mcp_tool_call::ActiveModel {
            id: ActiveValue::NotSet,
            conversation_id: Set(conversation_id),
            message_id: Set(message_id),
            server_id: Set(server_id),
            server_name: Set(server_name.to_string()),
            tool_name: Set(tool_name.to_string()),
            parameters: Set(parameters.to_string()),
            status: Set("pending".to_string()),
            result: Set(None),
            error: Set(None),
            created_time: ActiveValue::NotSet,
            started_time: Set(None),
            finished_time: Set(None),
            llm_call_id: Set(llm_call_id.map(|s| s.to_string())),
            assistant_message_id: Set(assistant_message_id),
            subtask_id: Set(None),
        };

        let result = self.with_runtime(|conn| async move { model.insert(&conn).await })?;

        debug!(id = result.id, "Created MCP tool call with LLM ID");
        self.get_mcp_tool_call(result.id)
    }

    /// Create MCP tool call specifically for subtask execution
    #[instrument(
        level = "debug",
        skip(self),
        fields(conversation_id, server_id, tool_name, subtask_id)
    )]
    pub fn create_mcp_tool_call_for_subtask(
        &self,
        conversation_id: i64,
        subtask_id: i64,
        server_id: i64,
        server_name: &str,
        tool_name: &str,
        parameters: &str,
        llm_call_id: Option<&str>,
    ) -> Result<MCPToolCall, DbErr> {
        let model = mcp_tool_call::ActiveModel {
            id: ActiveValue::NotSet,
            conversation_id: Set(conversation_id),
            message_id: Set(None),
            server_id: Set(server_id),
            server_name: Set(server_name.to_string()),
            tool_name: Set(tool_name.to_string()),
            parameters: Set(parameters.to_string()),
            status: Set("pending".to_string()),
            result: Set(None),
            error: Set(None),
            created_time: ActiveValue::NotSet,
            started_time: Set(None),
            finished_time: Set(None),
            llm_call_id: Set(llm_call_id.map(|s| s.to_string())),
            assistant_message_id: Set(None),
            subtask_id: Set(Some(subtask_id)),
        };

        let result = self.with_runtime(|conn| async move { model.insert(&conn).await })?;

        debug!(id = result.id, "Created MCP tool call for subtask");
        self.get_mcp_tool_call(result.id)
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn get_mcp_tool_call(&self, id: i64) -> Result<MCPToolCall, DbErr> {
        debug!(id, "get mcp tool call");
        let model = self.with_runtime(|conn| async move {
            mcp_tool_call::Entity::find_by_id(id).one(&conn).await
        })?;

        match model {
            Some(m) => {
                debug!(found = true, "Fetched MCP tool call");
                Ok(m.into())
            }
            None => {
                debug!(found = false, "MCP tool call not found");
                Err(DbErr::RecordNotFound("MCP tool call not found".to_string()))
            }
        }
    }

    #[instrument(level = "debug", skip(self), fields(id, status, has_result, has_error))]
    pub fn update_mcp_tool_call_status(
        &self,
        id: i64,
        status: &str,
        result: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), DbErr> {
        debug!(
            id,
            status,
            has_result = result.is_some(),
            has_error = error.is_some(),
            "update mcp tool call status"
        );
        let now = chrono::Utc::now();
        let status_str = status.to_string();
        let result_opt = result.map(|s| s.to_string());
        let error_opt = error.map(|s| s.to_string());

        match status {
            "executing" => {
                self.with_runtime(|conn| async move {
                    mcp_tool_call::Entity::update_many()
                        .col_expr(mcp_tool_call::Column::Status, Expr::value(status_str))
                        .col_expr(mcp_tool_call::Column::StartedTime, Expr::value(now))
                        .filter(mcp_tool_call::Column::Id.eq(id))
                        .exec(&conn)
                        .await?;
                    Ok(())
                })?;
            }
            "success" | "failed" => {
                self.with_runtime(|conn| async move {
                    mcp_tool_call::Entity::update_many()
                        .col_expr(mcp_tool_call::Column::Status, Expr::value(status_str))
                        .col_expr(mcp_tool_call::Column::Result, Expr::value(result_opt))
                        .col_expr(mcp_tool_call::Column::Error, Expr::value(error_opt))
                        .col_expr(mcp_tool_call::Column::FinishedTime, Expr::value(now))
                        .filter(mcp_tool_call::Column::Id.eq(id))
                        .exec(&conn)
                        .await?;
                    Ok(())
                })?;
            }
            _ => {
                self.with_runtime(|conn| async move {
                    mcp_tool_call::Entity::update_many()
                        .col_expr(mcp_tool_call::Column::Status, Expr::value(status_str))
                        .filter(mcp_tool_call::Column::Id.eq(id))
                        .exec(&conn)
                        .await?;
                    Ok(())
                })?;
            }
        }

        debug!("Updated MCP tool call status");
        Ok(())
    }

    /// Try to transition a tool call to executing state only if it is currently pending/failed and not yet started.
    /// Returns true if the transition happened, false if another executor already took it.
    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn mark_mcp_tool_call_executing_if_pending(&self, id: i64) -> Result<bool, DbErr> {
        let now = chrono::Utc::now();

        let rows = self.with_runtime(|conn| async move {
            let result = mcp_tool_call::Entity::update_many()
                .col_expr(mcp_tool_call::Column::Status, Expr::value("executing"))
                .col_expr(mcp_tool_call::Column::StartedTime, Expr::value(now))
                .filter(mcp_tool_call::Column::Id.eq(id))
                .filter(mcp_tool_call::Column::Status.is_in(vec!["pending", "failed"]))
                .exec(&conn)
                .await?;
            Ok::<u64, DbErr>(result.rows_affected)
        })?;

        let transitioned = rows > 0;
        debug!(id, transitioned, "try mark executing");
        Ok(transitioned)
    }

    #[instrument(level = "debug", skip(self), fields(conversation_id))]
    pub fn get_mcp_tool_calls_by_conversation(
        &self,
        conversation_id: i64,
    ) -> Result<Vec<MCPToolCall>, DbErr> {
        debug!(conversation_id, "query tool calls by conversation");
        let models = self.with_runtime(|conn| async move {
            mcp_tool_call::Entity::find()
                .filter(mcp_tool_call::Column::ConversationId.eq(conversation_id))
                .order_by_desc(mcp_tool_call::Column::CreatedTime)
                .all(&conn)
                .await
        })?;

        let calls: Vec<MCPToolCall> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = calls.len(), "Fetched MCP tool calls by conversation");
        Ok(calls)
    }

    /// Fetch MCP tool calls linked to a specific subtask execution
    #[instrument(level = "debug", skip(self), fields(subtask_id))]
    pub fn get_mcp_tool_calls_by_subtask(&self, subtask_id: i64) -> Result<Vec<MCPToolCall>, DbErr> {
        let models = self.with_runtime(|conn| async move {
            mcp_tool_call::Entity::find()
                .filter(mcp_tool_call::Column::SubtaskId.eq(subtask_id))
                .order_by_asc(mcp_tool_call::Column::CreatedTime)
                .all(&conn)
                .await
        })?;

        let calls: Vec<MCPToolCall> = models.into_iter().map(|m| m.into()).collect();
        debug!(count = calls.len(), "Fetched MCP tool calls by subtask");
        Ok(calls)
    }
}
