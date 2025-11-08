use std::collections::HashMap;
use sea_orm::DatabaseBackend;

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
					if host.is_empty() || db.is_empty() || user.is_empty() { return None; }
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
					if host.is_empty() || db.is_empty() || user.is_empty() { return None; }
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
