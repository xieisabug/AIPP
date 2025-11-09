use std::collections::HashMap;
use tauri::{Manager, State, Emitter};

// SeaORM entity imports for data aggregation (upload_local_data)
use crate::db::assistant_db::{
    assistant, assistant_model, assistant_prompt, assistant_model_config, assistant_prompt_param,
    assistant_mcp_config, assistant_mcp_tool_config,
};
use crate::db::conversation_db::{conversation, message, message_attachment};
use crate::db::llm_db::{llm_provider, llm_model, llm_provider_config};
use crate::db::mcp_db::{mcp_server, mcp_server_tool, mcp_server_resource, mcp_server_prompt, mcp_tool_call};
use crate::db::plugin_db::{plugins, plugin_status, plugin_configurations, plugin_data};
use crate::db::sub_task_db::{sub_task_definition, sub_task_execution};
use crate::db::artifacts_db::artifacts_collection;

use crate::template_engine::{BangType, TemplateEngine};
use crate::AppState;
use crate::utils::db_utils::build_remote_dsn;
use crate::FeatureConfigState;

use crate::db::system_db::{system_config, feature_config, FeatureConfig as FeatureConfigModel, SystemDatabase};

use sea_orm::{Database, DatabaseBackend, DatabaseConnection, EntityTrait, ColumnTrait, QueryFilter, QueryOrder, QuerySelect, Schema, ModelTrait, Value, DbErr, ConnectionTrait};
use sea_orm::sea_query::{SqliteQueryBuilder, PostgresQueryBuilder, MysqlQueryBuilder, Query, Expr, Alias, Func, Asterisk};

// 说明：已删除早期自定义迁移/数据上传逻辑；后续如需数据库升级，请使用 SeaORM 的迁移机制。

/// 将“本地”多个 SQLite 库的数据按表流式上传到“远程”数据库（PostgreSQL/MySQL）。
/// - 开始前进行远端连接健康检查
/// - 逐表、逐批次上传，避免一次性加载全部数据
/// - 表不存在时自动创建（仅表结构，不强制创建索引）
/// - 默认策略：先清空远端对应表，再全量上传
#[tauri::command]
pub async fn upload_local_data(app_handle: tauri::AppHandle, state: State<'_, FeatureConfigState>) -> Result<serde_json::Value, String> {
    // 读取远程存储配置
    let cfg_map = state.config_feature_map.lock().await;
    let ds = cfg_map.get("data_storage").ok_or("未找到数据存储配置(data_storage)")?;
    let storage_mode = ds.get("storage_mode").map(|c| c.value.clone()).unwrap_or_else(|| "local".to_string());
    if storage_mode != "remote" {
        return Err("当前存储模式不是 remote，请先在设置中切换为远程存储".to_string());
    }
    // 扁平化配置并复用统一 DSN 构建工具
    let mut flat: HashMap<String, String> = HashMap::new();
    for (k, v) in ds.iter() { flat.insert(k.clone(), v.value.clone()); }
    let (remote_url, backend) = build_remote_dsn(&flat)
        .ok_or_else(|| "暂不支持该远程类型，请使用 postgresql 或 mysql".to_string())?;

    let remote_conn = Database::connect(&remote_url).await.map_err(|e| format!("连接远端失败: {}", e))?;
    // 健康检查
    remote_conn.ping().await.map_err(|e| format!("远端 Ping 失败: {}", e))?;

    // 打开本地各个数据库连接
    let assistant_db = crate::db::assistant_db::AssistantDatabase::new(&app_handle)
        .map_err(|e| format!("assistant_db 打开失败: {}", e))?;
    let conversation_db = crate::db::conversation_db::ConversationDatabase::new(&app_handle)
        .map_err(|e| format!("conversation_db 打开失败: {}", e))?;
    let llm_db = crate::db::llm_db::LLMDatabase::new(&app_handle)
        .map_err(|e| format!("llm_db 打开失败: {}", e))?;
    let mcp_db = crate::db::mcp_db::MCPDatabase::new(&app_handle)
        .map_err(|e| format!("mcp_db 打开失败: {}", e))?;
    let plugin_db = crate::db::plugin_db::PluginDatabase::new(&app_handle)
        .map_err(|e| format!("plugin_db 打开失败: {}", e))?;
    let sub_task_db = crate::db::sub_task_db::SubTaskDatabase::new(&app_handle)
        .map_err(|e| format!("sub_task_db 打开失败: {}", e))?;
    let artifacts_db = crate::db::artifacts_db::ArtifactsDatabase::new(&app_handle)
        .map_err(|e| format!("artifacts_db 打开失败: {}", e))?;
    let system_db = SystemDatabase::new(&app_handle).map_err(|e| format!("system_db 打开失败: {}", e))?;

    // 后续建表时使用的 QueryBuilder
    // 小工具：清空远端表
    async fn truncate_table(conn: &DatabaseConnection, table: &str) -> Result<(), String> {
        // 兼容 PG/MySQL：使用 DELETE 而不是 TRUNCATE，避免额外权限
        let sql = format!("DELETE FROM {}", table);
        conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    // 小工具：批量上传（基于自增主键 id 顺序）
    async fn upload_by_pk<E>(app: &tauri::AppHandle, local: &DatabaseConnection, remote: &DatabaseConnection, id_col: <E as EntityTrait>::Column, table: &str, batch: u64)
        -> Result<(), String>
        where E: EntityTrait,
              <E as EntityTrait>::Model: Clone + Into<<E as EntityTrait>::ActiveModel>,
              <E as EntityTrait>::Column: ColumnTrait
    {
        // 统计总数（使用 sea_query COUNT(*)）
        let stmt = Query::select()
            .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("cnt"))
            .from(Alias::new(table))
            .to_owned();
        let total_row = local.query_one(&stmt).await.map_err(|e| e.to_string())?;
        let total: u64 = match total_row {
            Some(row) => {
                let v_i64: Result<i64, DbErr> = row.try_get("", "cnt");
                match v_i64 {
                    Ok(x) => x as u64,
                    Err(_) => {
                        let v_u64: Result<u64, DbErr> = row.try_get("", "cnt");
                        v_u64.map_err(|e| e.to_string())?
                    }
                }
            }
            None => 0,
        };
        let mut uploaded: u64 = 0;
        // 逐批读取（基于 id 递增）
        let mut last_id: i64 = i64::MIN; // 使用最小值避免过滤掉 id=0
        loop {
            let rows = E::find()
                .filter(id_col.clone().gt(last_id))
                .order_by_asc(id_col.clone())
                .limit(batch)
                .all(local)
                .await
                .map_err(|e| e.to_string())?;
            if rows.is_empty() { break; }

            let mut ams = Vec::with_capacity(rows.len());
            for m in rows.iter() {
                ams.push(m.clone().into());
            }

            // 单次批量插入（无需事务，默认 autocommit）
            E::insert_many(ams).exec(remote).await.map_err(|e| e.to_string())?;

            // 更新 last_id 与进度：通过 ModelTrait::get 读取主键值
            if let Some(last) = rows.last() {
                let v = last.get(id_col.clone());
                last_id = match v {
                    Value::BigInt(Some(x)) => x,
                    Value::Int(Some(x)) => x as i64,
                    Value::SmallInt(Some(x)) => x as i64,
                    Value::TinyInt(Some(x)) => x as i64,
                    Value::BigUnsigned(Some(x)) => x as i64,
                    Value::Unsigned(Some(x)) => x as i64,
                    Value::SmallUnsigned(Some(x)) => x as i64,
                    Value::TinyUnsigned(Some(x)) => x as i64,
                    other => return Err(format!("表 {} 主键列类型不受支持: {:?}", table, other)),
                };
            }

            uploaded += rows.len() as u64;
            let _ = app.emit("upload_local_data_progress", serde_json::json!({
                "table": table,
                "uploaded": uploaded,
                "total": total,
            }));
        }

        Ok(())
    }

    // 执行上传：逐库、逐表
    let conv_conn = conversation_db.get_conn();
    let batch_size: u64 = 500;

    // assistant 库
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(assistant::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(assistant::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(assistant::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "assistant").await?;
        upload_by_pk::<assistant::Entity>(&app_handle, &assistant_db.conn, &remote_conn, assistant::Column::Id, "assistant", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(assistant_model::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(assistant_model::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(assistant_model::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "assistant_model").await?;
        upload_by_pk::<assistant_model::Entity>(&app_handle, &assistant_db.conn, &remote_conn, assistant_model::Column::Id, "assistant_model", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(assistant_prompt::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(assistant_prompt::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(assistant_prompt::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "assistant_prompt").await?;
        upload_by_pk::<assistant_prompt::Entity>(&app_handle, &assistant_db.conn, &remote_conn, assistant_prompt::Column::Id, "assistant_prompt", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(assistant_model_config::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(assistant_model_config::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(assistant_model_config::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "assistant_model_config").await?;
        upload_by_pk::<assistant_model_config::Entity>(&app_handle, &assistant_db.conn, &remote_conn, assistant_model_config::Column::Id, "assistant_model_config", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(assistant_prompt_param::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(assistant_prompt_param::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(assistant_prompt_param::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "assistant_prompt_param").await?;
        upload_by_pk::<assistant_prompt_param::Entity>(&app_handle, &assistant_db.conn, &remote_conn, assistant_prompt_param::Column::Id, "assistant_prompt_param", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(assistant_mcp_config::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(assistant_mcp_config::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(assistant_mcp_config::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "assistant_mcp_config").await?;
        upload_by_pk::<assistant_mcp_config::Entity>(&app_handle, &assistant_db.conn, &remote_conn, assistant_mcp_config::Column::Id, "assistant_mcp_config", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(assistant_mcp_tool_config::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(assistant_mcp_tool_config::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(assistant_mcp_tool_config::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "assistant_mcp_tool_config").await?;
        upload_by_pk::<assistant_mcp_tool_config::Entity>(&app_handle, &assistant_db.conn, &remote_conn, assistant_mcp_tool_config::Column::Id, "assistant_mcp_tool_config", batch_size).await?;
    }

    // conversation 库
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(conversation::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(conversation::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(conversation::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "conversation").await?;
        upload_by_pk::<conversation::Entity>(&app_handle, &conv_conn, &remote_conn, conversation::Column::Id, "conversation", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(message::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(message::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(message::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "message").await?;
        upload_by_pk::<message::Entity>(&app_handle, &conv_conn, &remote_conn, message::Column::Id, "message", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(message_attachment::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(message_attachment::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(message_attachment::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "message_attachment").await?;
        upload_by_pk::<message_attachment::Entity>(&app_handle, &conv_conn, &remote_conn, message_attachment::Column::Id, "message_attachment", batch_size).await?;
    }

    // llm 库
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(llm_provider::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(llm_provider::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(llm_provider::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "llm_provider").await?;
        upload_by_pk::<llm_provider::Entity>(&app_handle, &llm_db.conn, &remote_conn, llm_provider::Column::Id, "llm_provider", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(llm_model::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(llm_model::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(llm_model::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "llm_model").await?;
        upload_by_pk::<llm_model::Entity>(&app_handle, &llm_db.conn, &remote_conn, llm_model::Column::Id, "llm_model", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(llm_provider_config::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(llm_provider_config::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(llm_provider_config::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "llm_provider_config").await?;
        upload_by_pk::<llm_provider_config::Entity>(&app_handle, &llm_db.conn, &remote_conn, llm_provider_config::Column::Id, "llm_provider_config", batch_size).await?;
    }

    // mcp 库
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(mcp_server::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(mcp_server::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(mcp_server::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "mcp_server").await?;
        upload_by_pk::<mcp_server::Entity>(&app_handle, &mcp_db.conn, &remote_conn, mcp_server::Column::Id, "mcp_server", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(mcp_server_tool::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(mcp_server_tool::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(mcp_server_tool::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "mcp_server_tool").await?;
        upload_by_pk::<mcp_server_tool::Entity>(&app_handle, &mcp_db.conn, &remote_conn, mcp_server_tool::Column::Id, "mcp_server_tool", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(mcp_server_resource::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(mcp_server_resource::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(mcp_server_resource::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "mcp_server_resource").await?;
        upload_by_pk::<mcp_server_resource::Entity>(&app_handle, &mcp_db.conn, &remote_conn, mcp_server_resource::Column::Id, "mcp_server_resource", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(mcp_server_prompt::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(mcp_server_prompt::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(mcp_server_prompt::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "mcp_server_prompt").await?;
        upload_by_pk::<mcp_server_prompt::Entity>(&app_handle, &mcp_db.conn, &remote_conn, mcp_server_prompt::Column::Id, "mcp_server_prompt", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(mcp_tool_call::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(mcp_tool_call::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(mcp_tool_call::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "mcp_tool_call").await?;
        upload_by_pk::<mcp_tool_call::Entity>(&app_handle, &mcp_db.conn, &remote_conn, mcp_tool_call::Column::Id, "mcp_tool_call", batch_size).await?;
    }

    // plugin 库
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(plugins::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(plugins::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(plugins::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "Plugins").await?; // 注意大小写
        upload_by_pk::<plugins::Entity>(&app_handle, &plugin_db.conn, &remote_conn, plugins::Column::PluginId, "Plugins", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(plugin_status::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(plugin_status::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(plugin_status::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "PluginStatus").await?;
        upload_by_pk::<plugin_status::Entity>(&app_handle, &plugin_db.conn, &remote_conn, plugin_status::Column::StatusId, "PluginStatus", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(plugin_configurations::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(plugin_configurations::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(plugin_configurations::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "PluginConfigurations").await?;
        upload_by_pk::<plugin_configurations::Entity>(&app_handle, &plugin_db.conn, &remote_conn, plugin_configurations::Column::ConfigId, "PluginConfigurations", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(plugin_data::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(plugin_data::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(plugin_data::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "PluginData").await?;
        upload_by_pk::<plugin_data::Entity>(&app_handle, &plugin_db.conn, &remote_conn, plugin_data::Column::DataId, "PluginData", batch_size).await?;
    }

    // sub_task 库
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(sub_task_definition::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(sub_task_definition::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(sub_task_definition::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "sub_task_definition").await?;
        upload_by_pk::<sub_task_definition::Entity>(&app_handle, &sub_task_db.conn, &remote_conn, sub_task_definition::Column::Id, "sub_task_definition", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(sub_task_execution::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(sub_task_execution::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(sub_task_execution::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "sub_task_execution").await?;
        upload_by_pk::<sub_task_execution::Entity>(&app_handle, &sub_task_db.conn, &remote_conn, sub_task_execution::Column::Id, "sub_task_execution", batch_size).await?;
    }

    // artifacts 库
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(artifacts_collection::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(artifacts_collection::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(artifacts_collection::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "artifacts_collection").await?;
        upload_by_pk::<artifacts_collection::Entity>(&app_handle, &artifacts_db.conn, &remote_conn, artifacts_collection::Column::Id, "artifacts_collection", batch_size).await?;
    }

    // system 库
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(system_config::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(system_config::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(system_config::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "system_config").await?;
        upload_by_pk::<system_config::Entity>(&app_handle, system_db.get_conn(), &remote_conn, system_config::Column::Id, "system_config", batch_size).await?;
    }
    {
        let schema = Schema::new(backend);
        let sql = match backend { DatabaseBackend::Postgres => schema.create_table_from_entity(feature_config::Entity).if_not_exists().to_string(PostgresQueryBuilder), DatabaseBackend::MySql => schema.create_table_from_entity(feature_config::Entity).if_not_exists().to_string(MysqlQueryBuilder), _ => schema.create_table_from_entity(feature_config::Entity).if_not_exists().to_string(SqliteQueryBuilder) };
        remote_conn.execute_unprepared(&sql).await.map_err(|e| e.to_string())?;
        truncate_table(&remote_conn, "feature_config").await?;
        upload_by_pk::<feature_config::Entity>(&app_handle, system_db.get_conn(), &remote_conn, feature_config::Column::Id, "feature_config", batch_size).await?;
    }

    Ok(serde_json::json!({"status": "ok"}))
}

#[tauri::command]
pub async fn get_all_feature_config(
    state: State<'_, FeatureConfigState>,
) -> Result<Vec<FeatureConfigModel>, String> {
    let configs = state.configs.lock().await;
    Ok(configs.clone())
}

/// 获取数据存储配置（feature_code = "data_storage"），以扁平 map 返回
#[tauri::command]
pub async fn get_data_storage_config(
    state: State<'_, FeatureConfigState>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let config_feature_map = state.config_feature_map.lock().await;
    let mut result: std::collections::HashMap<String, String> = Default::default();
    if let Some(map) = config_feature_map.get("data_storage") {
        for (k, v) in map.iter() {
            result.insert(k.clone(), v.value.clone());
        }
    } else {
        // 默认本地存储
        result.insert("storage_mode".to_string(), "local".to_string());
    }
    Ok(result)
}

#[tauri::command]
pub async fn save_feature_config(
    app_handle: tauri::AppHandle,
    state: State<'_, FeatureConfigState>,
    feature_code: String,
    config: HashMap<String, String>,
) -> Result<(), String> {
    let db = SystemDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let _ = db.delete_feature_config_by_feature_code(&app_handle, feature_code.as_str());
    for (key, value) in config.iter() {
        db.add_feature_config(&app_handle, &FeatureConfigModel {
            id: None,
            feature_code: feature_code.clone(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        })
        .map_err(|e| e.to_string())?;
    }

    // 更新内存状态
    let mut configs = state.configs.lock().await;
    let mut config_feature_map = state.config_feature_map.lock().await;

    // 删除旧的配置
    configs.retain(|c| c.feature_code != feature_code);
    config_feature_map.remove(&feature_code);

    // 添加新的配置
    for (key, value) in config.iter() {
        let new_config = FeatureConfigModel {
            id: None,
            feature_code: feature_code.clone(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        };
        configs.push(new_config.clone());
        config_feature_map
            .entry(feature_code.clone())
            .or_insert(HashMap::new())
            .insert(key.clone(), new_config);
    }
    // 如果更新的是快捷键配置，则尝试重新注册全局快捷键（异步，避免阻塞 runtime）
    #[cfg(desktop)]
    if feature_code == "shortcuts" {
        let app = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            crate::reconfigure_global_shortcuts_async(&app).await;
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn open_data_folder(app: tauri::AppHandle) -> Result<(), String> {
    let app_dir = app.path().app_data_dir().unwrap();
    let db_path = app_dir.join("db");
    if let Err(e) = open::that(db_path) {
        return Err(format!("无法打开数据文件夹: {}", e));
    }
    Ok(())
}

/// Save data storage configuration into feature_config under feature_code = "data_storage"
#[tauri::command]
pub async fn save_data_storage_config(
    app_handle: tauri::AppHandle,
    state: State<'_, FeatureConfigState>,
    storage_mode: String,                 // "local" | "remote"
    remote_type: Option<String>,          // Some("supabase"|"postgresql"|"mysql") when remote
    payload: std::collections::HashMap<String, String>,
)
-> Result<(), String> {
    let db = SystemDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // Build a flattened map to store
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("storage_mode".to_string(), storage_mode.clone());
    if let Some(rt) = remote_type.clone() { config.insert("remote_type".to_string(), rt); }
    for (k,v) in payload.iter() { config.insert(k.clone(), v.clone()); }

    // Persist by replacing feature_code = data_storage
    let feature_code = "data_storage".to_string();
    let _ = db.delete_feature_config_by_feature_code(&app_handle, &feature_code);
    for (key, value) in config.iter() {
        db.add_feature_config(&app_handle, &FeatureConfigModel {
            id: None,
            feature_code: feature_code.clone(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        }).map_err(|e| e.to_string())?;
    }

    // Update in-memory state
    let mut configs = state.configs.lock().await;
    let mut config_feature_map = state.config_feature_map.lock().await;
    configs.retain(|c| c.feature_code != feature_code);
    config_feature_map.remove("data_storage");
    for (key, value) in config.iter() {
        let new_config = FeatureConfigModel {
            id: None,
            feature_code: "data_storage".to_string(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        };
        configs.push(new_config.clone());
        config_feature_map
            .entry("data_storage".to_string())
            .or_insert(Default::default())
            .insert(key.clone(), new_config);
    }

    Ok(())
}

/// Test connectivity to a remote storage based on type and payload
#[tauri::command]
pub async fn test_remote_storage_connection(
    remote_type: String,
    payload: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    match remote_type.as_str() {
        "supabase" => {
            let url = payload.get("supabase_url").cloned().ok_or("缺少 supabase_url")?;
            let key = payload.get("supabase_key").cloned().ok_or("缺少 supabase_key")?;
            // Try a lightweight request to the REST endpoint root; any non-5xx status counts as reachable
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
                .map_err(|e| e.to_string())?;
            let rest_url = format!("{}/rest/v1/", url.trim_end_matches('/'));
            let resp = client
                .get(&rest_url)
                .header("apikey", key.clone())
                .header("Authorization", format!("Bearer {}", key))
                .send()
                .await
                .map_err(|e| format!("请求失败: {}", e))?;
            if resp.status().is_server_error() {
                return Err(format!("Supabase 服务返回错误状态: {}", resp.status()));
            }
            Ok(())
        }
        "postgresql" | "mysql" => {
            // 使用统一 DSN 工具
            let mut flat = payload.clone();
            flat.insert("storage_mode".to_string(), "remote".to_string());
            flat.insert("remote_type".to_string(), remote_type.clone());
            let (url, _backend) = build_remote_dsn(&flat)
                .ok_or_else(|| "参数不完整或类型不支持".to_string())?;
            let conn = sea_orm::Database::connect(&url).await.map_err(|e| format!("连接失败: {}", e))?;
            conn.ping().await.map_err(|e| format!("Ping 失败: {}", e))?;
            Ok(())
        }
        _ => Err("不支持的远程类型".to_string()),
    }
}

#[tauri::command]
pub async fn get_bang_list() -> Result<Vec<(String, String, String, BangType)>, String> {
    let engine = TemplateEngine::new();
    let mut list = vec![];
    for bang in engine.get_commands().iter() {
        list.push((
            bang.name.clone(),
            bang.complete.clone(),
            bang.description.clone(),
            bang.bang_type.clone(),
        ));
    }
    list.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(list)
}
#[tauri::command]
pub async fn get_selected_text_api(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let selected_text = state.selected_text.lock().await;
    Ok(selected_text.clone())
}

#[tauri::command]
pub async fn set_shortcut_recording(state: tauri::State<'_, AppState>, active: bool) -> Result<(), String> {
    let mut flag = state.recording_shortcut.lock().await;
    *flag = active;
    Ok(())
}

#[tauri::command]
pub async fn suspend_global_shortcut(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        if let Err(e) = app.global_shortcut().unregister_all() {
            return Err(format!("无法暂停全局快捷键: {}", e));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn resume_global_shortcut(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(desktop)]
    {
        crate::reconfigure_global_shortcuts_async(&app).await;
    }
    Ok(())
}
