use crate::db::connection::{DbError, Result as DbResult, Row};
use chrono::prelude::*;
use chrono::NaiveDateTime;

/// 统一的DateTime类型转换辅助函数
/// 用于处理SQLite中DATETIME字段的不同存储格式（字符串或时间戳）
///
/// 这个函数能够处理以下SQLite中DATETIME的存储格式：
/// 1. ISO 8601字符串格式 (如 "2024-01-01T10:00:00Z")
/// 2. Unix时间戳整数格式 - 支持秒级和毫秒级 (如 1704106800 或 1704106800000)
/// 3. NULL值
pub fn get_datetime_from_row(row: &Row, index: usize) -> DbResult<Option<DateTime<Utc>>> {
    match row.get::<_, Option<String>>(index) {
        Ok(Some(time_str)) => Ok(parse_datetime_string(&time_str)),
        Ok(None) => Ok(None),
        Err(_) => match row.get::<_, Option<i64>>(index) {
            Ok(Some(timestamp)) => Ok(timestamp_to_datetime(timestamp)),
            Ok(None) => Ok(None),
            Err(_) => Ok(None),
        },
    }
}

/// 统一的非空DateTime类型转换辅助函数
/// 用于处理必须有值的DateTime字段
///
/// 这个函数与 get_datetime_from_row 类似，但是对于必须有值的字段，
/// 如果解析失败会返回适当的错误信息
pub fn get_required_datetime_from_row(
    row: &Row,
    index: usize,
    field_name: &str,
) -> DbResult<DateTime<Utc>> {
    match row.get::<_, String>(index) {
        Ok(time_str) => parse_datetime_string(&time_str).ok_or_else(|| {
            DbError::InvalidColumnType(index, field_name.to_string(), "Text".to_string())
        }),
        Err(_) => {
            let timestamp: i64 = row.get(index)?;
            timestamp_to_datetime(timestamp).ok_or_else(|| {
                DbError::InvalidColumnType(index, field_name.to_string(), "Integer".to_string())
            })
        }
    }
}

/// 尝试使用常见格式解析 datetime 字符串，缺少时区时默认 UTC
fn parse_datetime_string(time_str: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(time_str) {
        return Some(dt.with_timezone(&Utc));
    }

    if let Ok(dt) = DateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S%.f%:z") {
        return Some(dt.with_timezone(&Utc));
    }

    if let Ok(dt) = NaiveDateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(Utc.from_utc_datetime(&dt));
    }

    if let Ok(dt) = NaiveDateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S") {
        return Some(Utc.from_utc_datetime(&dt));
    }

    None
}

/// 智能毫秒/秒时间戳转换
fn timestamp_to_datetime(timestamp: i64) -> Option<DateTime<Utc>> {
    if timestamp > 1_000_000_000_000 {
        DateTime::from_timestamp_millis(timestamp)
    } else {
        DateTime::from_timestamp(timestamp, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::Connection;
    use chrono::Utc;

    #[test]
    fn test_datetime_conversion_string() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER, datetime_field TEXT)", ()).unwrap();

        let test_time = "2024-01-01T10:00:00Z";
        conn.execute(
            "INSERT INTO test (id, datetime_field) VALUES (1, ?)",
            crate::db::connection::params![test_time],
        )
        .unwrap();

        let result = conn
            .query_row("SELECT datetime_field FROM test WHERE id = 1", (), |row| {
                get_required_datetime_from_row(row, 0, "datetime_field")
            })
            .unwrap();

        assert_eq!(result.to_rfc3339(), "2024-01-01T10:00:00+00:00");
    }

    #[test]
    fn test_datetime_conversion_timestamp() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER, datetime_field INTEGER)", ()).unwrap();

        let test_timestamp = 1704106800i64;
        conn.execute(
            "INSERT INTO test (id, datetime_field) VALUES (1, ?)",
            crate::db::connection::params![test_timestamp],
        )
        .unwrap();

        let result = conn
            .query_row("SELECT datetime_field FROM test WHERE id = 1", (), |row| {
                get_required_datetime_from_row(row, 0, "datetime_field")
            })
            .unwrap();

        let expected = DateTime::from_timestamp(test_timestamp, 0).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_datetime_conversion_millisecond_timestamp() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER, datetime_field INTEGER)", ()).unwrap();

        let test_timestamp = 1756646536000i64;
        conn.execute(
            "INSERT INTO test (id, datetime_field) VALUES (1, ?)",
            crate::db::connection::params![test_timestamp],
        )
        .unwrap();

        let result = conn
            .query_row("SELECT datetime_field FROM test WHERE id = 1", (), |row| {
                get_required_datetime_from_row(row, 0, "datetime_field")
            })
            .unwrap();

        let expected = DateTime::from_timestamp_millis(test_timestamp).unwrap();
        assert_eq!(result, expected);
        assert!(result.year() > 2020 && result.year() < 2100);
    }

    #[test]
    fn test_datetime_sqlite_style_string_with_timezone() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER, datetime_field TEXT)", ()).unwrap();

        let test_time = "2025-12-21 07:36:33.795681700+00:00";
        conn.execute(
            "INSERT INTO test (id, datetime_field) VALUES (1, ?)",
            crate::db::connection::params![test_time],
        )
        .unwrap();

        let result = conn
            .query_row("SELECT datetime_field FROM test WHERE id = 1", (), |row| {
                get_datetime_from_row(row, 0)
            })
            .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_rfc3339(), "2025-12-21T07:36:33.795681700+00:00");
    }

    #[test]
    fn test_datetime_sqlite_style_string_without_timezone() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER, datetime_field TEXT)", ()).unwrap();

        let test_time = "2025-12-21 07:36:47";
        conn.execute(
            "INSERT INTO test (id, datetime_field) VALUES (1, ?)",
            crate::db::connection::params![test_time],
        )
        .unwrap();

        let result = conn
            .query_row("SELECT datetime_field FROM test WHERE id = 1", (), |row| {
                get_datetime_from_row(row, 0)
            })
            .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_rfc3339(), "2025-12-21T07:36:47+00:00");
    }

    #[test]
    fn test_optional_datetime_conversion_null() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER, datetime_field TEXT)", ()).unwrap();

        conn.execute("INSERT INTO test (id, datetime_field) VALUES (1, NULL)", ()).unwrap();

        let result = conn
            .query_row("SELECT datetime_field FROM test WHERE id = 1", (), |row| {
                get_datetime_from_row(row, 0)
            })
            .unwrap();

        assert_eq!(result, None);
    }
}
