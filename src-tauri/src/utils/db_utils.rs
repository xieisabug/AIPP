use crate::db::get_db_path;
use sea_orm::{Database, DatabaseBackend, DatabaseConnection, DbErr};
use std::collections::HashMap;

/// 根据启动时缓存的 data_storage 展平配置构建远程数据库 DSN。
/// 返回 Some((dsn, backend)) 表示应使用远程连接；None 表示继续使用本地 SQLite。
/// 约定字段：
///   storage_mode = "remote" | "local"
///   remote_type  = "postgresql" | "mysql"
/// PostgreSQL:
///   pg_host, pg_port (默认5432), pg_database, pg_username, pg_password
/// MySQL:
///   mysql_host, mysql_port (默认3306), mysql_database, mysql_username, mysql_password
pub fn build_remote_dsn(flat: &HashMap<String, String>) -> Option<(String, DatabaseBackend)> {
    if flat.get("storage_mode").map(|v| v == "remote").unwrap_or(false) {
        if let Some(remote_type) = flat.get("remote_type") {
            match remote_type.as_str() {
                "postgresql" => {
                    let host = flat.get("pg_host")?.clone();
                    let db = flat.get("pg_database")?.clone();
                    let user = flat.get("pg_username")?.clone();
                    let pass = flat.get("pg_password").cloned().unwrap_or_default();
                    let port = flat.get("pg_port").cloned().unwrap_or_else(|| "5432".into());
                    if host.is_empty() || db.is_empty() || user.is_empty() {
                        return None;
                    }
                    let dsn = format!(
                        "postgres://{}:{}@{}:{}/{}",
                        urlencoding::encode(&user),
                        urlencoding::encode(&pass),
                        host,
                        port,
                        db
                    );
                    return Some((dsn, DatabaseBackend::Postgres));
                }
                "mysql" => {
                    let host = flat.get("mysql_host")?.clone();
                    let db = flat.get("mysql_database")?.clone();
                    let user = flat.get("mysql_username")?.clone();
                    let pass = flat.get("mysql_password").cloned().unwrap_or_default();
                    let port = flat.get("mysql_port").cloned().unwrap_or_else(|| "3306".into());
                    if host.is_empty() || db.is_empty() || user.is_empty() {
                        return None;
                    }
                    let dsn = format!(
                        "mysql://{}:{}@{}:{}/{}",
                        urlencoding::encode(&user),
                        urlencoding::encode(&pass),
                        host,
                        port,
                        db
                    );
                    return Some((dsn, DatabaseBackend::MySql));
                }
                _ => return None,
            }
        }
    }
    None
}

/// 根据配置初始化全局数据库连接
/// 如果是远程模式，连接到远程数据库
/// 如果是本地模式，创建 SQLite 连接（将来可以支持 ATTACH 多个数据库文件）
pub async fn initialize_database_connection(
    app_handle: &tauri::AppHandle,
    config: &HashMap<String, String>,
) -> Result<DatabaseConnection, Box<dyn std::error::Error>> {
    if let Some((dsn, _backend)) = build_remote_dsn(config) {
        // 远程数据库
        tracing::info!("Connecting to remote database");
        let conn = Database::connect(&dsn).await?;
        Ok(conn)
    } else {
        // 本地 SQLite - 暂时使用主数据库文件
        // TODO: 未来可以在这里使用 ATTACH DATABASE 合并多个 .db 文件
        tracing::info!("Connecting to local SQLite database");
        let db_path = get_db_path(app_handle, "main.db")?;
        let url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
        let conn = Database::connect(&url).await?;
        Ok(conn)
    }
}
