//! Compatibility layer providing a rusqlite-like **sync** API over libsql's async connection.
//!
//! This module enables incremental migration from rusqlite to libsql by providing
//! familiar types (`Connection`, `Statement`, `Row`) with synchronous methods that
//! bridge to libsql's async API using `tokio::task::block_in_place`.
//!
//! # Usage
//! ```rust
//! use crate::db::connection::{Connection, Result, OptionalExtension, params};
//!
//! let conn = Connection::open("path/to/db.sqlite")?;
//! conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)", ())?;
//! conn.execute("INSERT INTO t (name) VALUES (?1)", params!["alice"])?;
//! ```

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use reqwest::Url;

/// Run an async future synchronously. Uses `block_in_place` when inside a
/// tokio multi-thread runtime, otherwise spins up a temporary runtime.
fn run_async<F: std::future::Future>(f: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f)),
        Err(_) => {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime for DB");
            rt.block_on(f)
        }
    }
}

// ─── Param conversion ───────────────────────────────────────────────────────

/// Trait for converting Rust values into `libsql::Value` for use in query params.
///
/// Unlike libsql's sealed `IntoValue`, this trait is open and supports chrono types.
pub trait IntoDbParam {
    fn into_db_param(self) -> libsql::Value;
}

impl IntoDbParam for i64 {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Integer(self)
    }
}
impl IntoDbParam for i32 {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Integer(self as i64)
    }
}
impl IntoDbParam for u32 {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Integer(self as i64)
    }
}
impl IntoDbParam for u64 {
    fn into_db_param(self) -> libsql::Value {
        match i64::try_from(self) {
            Ok(value) => libsql::Value::Integer(value),
            Err(_) => libsql::Value::Text(self.to_string()),
        }
    }
}
impl IntoDbParam for f64 {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Real(self)
    }
}
impl IntoDbParam for bool {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Integer(if self { 1 } else { 0 })
    }
}
impl IntoDbParam for String {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Text(self)
    }
}
impl IntoDbParam for &str {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Text(self.to_string())
    }
}
impl IntoDbParam for &String {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Text(self.clone())
    }
}
impl IntoDbParam for Vec<u8> {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Blob(self)
    }
}
impl IntoDbParam for libsql::Value {
    fn into_db_param(self) -> libsql::Value {
        self
    }
}
impl IntoDbParam for chrono::DateTime<chrono::Utc> {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Text(self.to_rfc3339())
    }
}
impl IntoDbParam for &chrono::DateTime<chrono::Utc> {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Text(self.to_rfc3339())
    }
}
impl IntoDbParam for chrono::NaiveDateTime {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Text(self.format("%Y-%m-%d %H:%M:%S").to_string())
    }
}
impl IntoDbParam for u8 {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Integer(self as i64)
    }
}
impl IntoDbParam for &i64 {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Integer(*self)
    }
}
impl IntoDbParam for &i32 {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Integer(*self as i64)
    }
}
impl IntoDbParam for &f64 {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Real(*self)
    }
}
impl IntoDbParam for &bool {
    fn into_db_param(self) -> libsql::Value {
        libsql::Value::Integer(if *self { 1 } else { 0 })
    }
}
impl<T: IntoDbParam> IntoDbParam for Option<T> {
    fn into_db_param(self) -> libsql::Value {
        match self {
            Some(v) => v.into_db_param(),
            None => libsql::Value::Null,
        }
    }
}
impl<T: IntoDbParam + Clone> IntoDbParam for &Option<T> {
    fn into_db_param(self) -> libsql::Value {
        match self {
            Some(v) => v.clone().into_db_param(),
            None => libsql::Value::Null,
        }
    }
}

/// Build a parameter list for SQL queries.
///
/// Drop-in replacement for `libsql::params!` / `rusqlite::params!` with
/// added chrono support.
///
/// ```rust
/// params![42, "alice", chrono::Utc::now()]
/// ```
#[macro_export]
macro_rules! db_params {
    ($($val:expr),* $(,)?) => {
        ::libsql::params_from_iter(vec![$($crate::db::connection::IntoDbParam::into_db_param($val)),*])
    };
}

pub use db_params as params;

/// Build a param list from an iterator of values convertible to `libsql::Value`.
pub fn params_from_iter<I>(iter: I) -> impl libsql::params::IntoParams
where
    I: IntoIterator,
    I::Item: IntoDbParam,
{
    let values: Vec<libsql::Value> = iter.into_iter().map(|v| v.into_db_param()).collect();
    libsql::params_from_iter(values)
}

// ─── Error ──────────────────────────────────────────────────────────────────

/// Unified database error type, replacing `rusqlite::Error`.
#[derive(Debug)]
pub enum DbError {
    /// Error from the underlying libsql driver.
    LibSql(libsql::Error),
    /// A query that was expected to return at least one row returned none.
    QueryReturnedNoRows,
    /// Free-form error message (used during migrations, etc.).
    Custom(String),
    /// Column type mismatch (index, expected_name, actual_type_description).
    InvalidColumnType(usize, String, String),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::LibSql(e) => write!(f, "libsql error: {e}"),
            DbError::QueryReturnedNoRows => write!(f, "Query returned no rows"),
            DbError::Custom(s) => write!(f, "{s}"),
            DbError::InvalidColumnType(idx, name, desc) => {
                write!(f, "Invalid column type at {idx} ({name}): {desc}")
            }
        }
    }
}

impl std::error::Error for DbError {}

impl From<libsql::Error> for DbError {
    fn from(e: libsql::Error) -> Self {
        DbError::LibSql(e)
    }
}

impl From<String> for DbError {
    fn from(s: String) -> Self {
        DbError::Custom(s)
    }
}

impl From<&str> for DbError {
    fn from(s: &str) -> Self {
        DbError::Custom(s.to_string())
    }
}

/// Result alias using `DbError`, replacing `rusqlite::Result`.
pub type Result<T> = std::result::Result<T, DbError>;

// ─── Row ────────────────────────────────────────────────────────────────────

/// Wrapper around `libsql::Row` providing a rusqlite-compatible `get` API.
///
/// libsql's `FromValue` trait is sealed, so we use `get_value()` + our own
/// `FromDbValue` trait to extract typed values from rows.
pub struct Row {
    pub(crate) inner: libsql::Row,
}

impl Row {
    /// Retrieve a column value by index.
    ///
    /// Accepts two type parameters for rusqlite compatibility:
    /// ```rust
    /// let name: String = row.get::<_, String>(0)?;
    /// let id: i64 = row.get(0)?;          // both type params inferred
    /// ```
    /// The first type parameter `I` is the index type (always inferred from the
    /// argument) and exists solely so that `row.get::<_, T>(idx)` compiles.
    pub fn get<I: ColumnIndex, T: FromDbValue>(&self, idx: I) -> Result<T> {
        let value = self.inner.get_value(idx.to_i32()).map_err(DbError::from)?;
        T::from_db_value(value)
    }

    /// Get the raw `libsql::Value` at the given column index.
    pub fn get_value(&self, idx: usize) -> Result<libsql::Value> {
        self.inner.get_value(idx as i32).map_err(Into::into)
    }

    /// Number of columns in this row.
    pub fn column_count(&self) -> i32 {
        self.inner.column_count()
    }

    /// Column name at the given index.
    pub fn column_name(&self, idx: i32) -> Option<&str> {
        self.inner.column_name(idx)
    }
}

/// Trait for types that can serve as a column index.
///
/// Allows integer literals to work transparently in `row.get(0)`.
pub trait ColumnIndex {
    fn to_i32(&self) -> i32;
}

impl ColumnIndex for usize {
    fn to_i32(&self) -> i32 {
        *self as i32
    }
}

impl ColumnIndex for i32 {
    fn to_i32(&self) -> i32 {
        *self
    }
}

impl ColumnIndex for u32 {
    fn to_i32(&self) -> i32 {
        *self as i32
    }
}

// ─── FromDbValue ────────────────────────────────────────────────────────────

/// Trait for converting `libsql::Value` to concrete Rust types.
///
/// This replaces libsql's sealed `FromValue` trait, allowing us to use
/// `row.get::<_, T>(idx)` in the same pattern as rusqlite.
pub trait FromDbValue: Sized {
    fn from_db_value(value: libsql::Value) -> Result<Self>;
}

impl FromDbValue for i64 {
    fn from_db_value(value: libsql::Value) -> Result<Self> {
        match value {
            libsql::Value::Integer(v) => Ok(v),
            libsql::Value::Null => Err(DbError::Custom("unexpected NULL for i64".into())),
            other => Err(DbError::Custom(format!("expected Integer, got {other:?}"))),
        }
    }
}

impl FromDbValue for i32 {
    fn from_db_value(value: libsql::Value) -> Result<Self> {
        match value {
            libsql::Value::Integer(v) => Ok(v as i32),
            libsql::Value::Null => Err(DbError::Custom("unexpected NULL for i32".into())),
            other => Err(DbError::Custom(format!("expected Integer, got {other:?}"))),
        }
    }
}

impl FromDbValue for u32 {
    fn from_db_value(value: libsql::Value) -> Result<Self> {
        match value {
            libsql::Value::Integer(v) => Ok(v as u32),
            libsql::Value::Null => Err(DbError::Custom("unexpected NULL for u32".into())),
            other => Err(DbError::Custom(format!("expected Integer, got {other:?}"))),
        }
    }
}

impl FromDbValue for f64 {
    fn from_db_value(value: libsql::Value) -> Result<Self> {
        match value {
            libsql::Value::Real(v) => Ok(v),
            libsql::Value::Integer(v) => Ok(v as f64),
            libsql::Value::Null => Err(DbError::Custom("unexpected NULL for f64".into())),
            other => Err(DbError::Custom(format!("expected Real, got {other:?}"))),
        }
    }
}

impl FromDbValue for String {
    fn from_db_value(value: libsql::Value) -> Result<Self> {
        match value {
            libsql::Value::Text(v) => Ok(v),
            libsql::Value::Null => Err(DbError::Custom("unexpected NULL for String".into())),
            other => Err(DbError::Custom(format!("expected Text, got {other:?}"))),
        }
    }
}

impl FromDbValue for Vec<u8> {
    fn from_db_value(value: libsql::Value) -> Result<Self> {
        match value {
            libsql::Value::Blob(v) => Ok(v),
            libsql::Value::Null => Err(DbError::Custom("unexpected NULL for Blob".into())),
            other => Err(DbError::Custom(format!("expected Blob, got {other:?}"))),
        }
    }
}

impl FromDbValue for bool {
    fn from_db_value(value: libsql::Value) -> Result<Self> {
        match value {
            libsql::Value::Integer(v) => Ok(v != 0),
            libsql::Value::Null => Err(DbError::Custom("unexpected NULL for bool".into())),
            other => Err(DbError::Custom(format!("expected Integer for bool, got {other:?}"))),
        }
    }
}

impl<T: FromDbValue> FromDbValue for Option<T> {
    fn from_db_value(value: libsql::Value) -> Result<Self> {
        match value {
            libsql::Value::Null => Ok(None),
            other => T::from_db_value(other).map(Some),
        }
    }
}

impl FromDbValue for libsql::Value {
    fn from_db_value(value: libsql::Value) -> Result<Self> {
        Ok(value)
    }
}

// ─── Chrono support ────────────────────────────────────────────────────────

impl FromDbValue for chrono::DateTime<chrono::Utc> {
    fn from_db_value(value: libsql::Value) -> Result<Self> {
        match value {
            libsql::Value::Text(s) => {
                // Try RFC3339 first (e.g. "2024-01-15T10:30:00+00:00")
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&s) {
                    return Ok(dt.with_timezone(&chrono::Utc));
                }
                // Try SQLite CURRENT_TIMESTAMP format (e.g. "2024-01-15 10:30:00")
                if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S") {
                    return Ok(naive.and_utc());
                }
                Err(DbError::InvalidColumnType(
                    0,
                    "DateTime<Utc>".into(),
                    format!("unparseable datetime string: {s}"),
                ))
            }
            other => Err(DbError::InvalidColumnType(
                0,
                "DateTime<Utc>".into(),
                format!("expected Text, got {other:?}"),
            )),
        }
    }
}

// ─── MappedRows ─────────────────────────────────────────────────────────────

/// An eagerly-collected row iterator, matching the shape of `rusqlite::MappedRows`
/// so that `.collect::<Result<Vec<_>, _>>()` works unchanged.
pub struct MappedRows<T> {
    items: std::vec::IntoIter<Result<T>>,
}

impl<T> Iterator for MappedRows<T> {
    type Item = Result<T>;
    fn next(&mut self) -> Option<Self::Item> {
        self.items.next()
    }
}

// ─── Statement ──────────────────────────────────────────────────────────────

/// Wrapper around a pre-parsed SQL string that executes queries against a
/// `libsql::Connection`.
///
/// Unlike rusqlite's `Statement` (which wraps a compiled sqlite3_stmt),
/// this stores the SQL text and delegates to `conn.query` / `conn.execute`.
/// The libsql driver caches compiled statements internally.
pub struct Statement<'conn> {
    conn: &'conn libsql::Connection,
    sql: String,
}

impl<'conn> Statement<'conn> {
    /// Execute a query and map each row through `f`, collecting results
    /// into a `MappedRows` iterator.
    ///
    /// ```rust
    /// let configs = stmt.query_map(params![code], |row| {
    ///     Ok(Config { id: row.get(0)?, name: row.get(1)? })
    /// })?.collect::<Result<Vec<_>, _>>()?;
    /// ```
    pub fn query_map<T, P, F>(&mut self, params: P, mut f: F) -> Result<MappedRows<T>>
    where
        P: libsql::params::IntoParams,
        F: FnMut(&Row) -> Result<T>,
    {
        let items: Vec<Result<T>> = run_async(async {
            let mut rows = self.conn.query(&self.sql, params).await?;
            let mut results: Vec<Result<T>> = Vec::new();
            while let Some(row) = rows.next().await? {
                let wrapped = Row { inner: row };
                results.push(f(&wrapped));
            }
            Ok::<_, libsql::Error>(results)
        })?;
        Ok(MappedRows { items: items.into_iter() })
    }

    /// Execute a query that is expected to return exactly one row.
    ///
    /// Returns `DbError::QueryReturnedNoRows` if the result set is empty.
    pub fn query_row<T, P, F>(&self, params: P, f: F) -> Result<T>
    where
        P: libsql::params::IntoParams,
        F: FnOnce(&Row) -> Result<T>,
    {
        run_async(async {
            let mut rows = self.conn.query(&self.sql, params).await.map_err(DbError::from)?;
            match rows.next().await.map_err(DbError::from)? {
                Some(row) => f(&Row { inner: row }),
                None => Err(DbError::QueryReturnedNoRows),
            }
        })
    }

    /// Execute a non-SELECT statement, returning the number of rows changed.
    pub fn execute<P: libsql::params::IntoParams>(&self, params: P) -> Result<usize> {
        run_async(self.conn.execute(&self.sql, params)).map(|n| n as usize).map_err(Into::into)
    }
}

// ─── OptionalExtension ─────────────────────────────────────────────────────

/// Trait to convert a `QueryReturnedNoRows` error into `Ok(None)`,
/// matching `rusqlite::OptionalExtension`.
pub trait OptionalExtension<T> {
    fn optional(self) -> Result<Option<T>>;
}

impl<T> OptionalExtension<T> for Result<T> {
    fn optional(self) -> Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(DbError::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

// ─── Connection ─────────────────────────────────────────────────────────────

/// A synchronous database connection wrapping `libsql::Connection`.
///
/// Drop-in replacement for `rusqlite::Connection` — all methods are sync
/// and internally bridge to libsql's async API.
pub struct Connection {
    /// Keeps the `libsql::Database` alive for standalone connections.
    /// When connections are obtained from `DatabaseManager`, this is `None`.
    _db: Option<libsql::Database>,
    inner: libsql::Connection,
}

impl Connection {
    /// Open a local database file.
    ///
    /// This is the direct replacement for `rusqlite::Connection::open(path)`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        if let Some(state) = ManagedDatabaseState::global() {
            if let Some(conn) = state.connect_for_path(path) {
                return conn;
            }
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| DbError::Custom("Invalid UTF-8 in database path".into()))?;
        let db = run_async(libsql::Builder::new_local(path_str).build())?;
        let conn = db.connect()?;
        Ok(Self { _db: Some(db), inner: conn })
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let db = run_async(libsql::Builder::new_local(":memory:").build())?;
        let conn = db.connect()?;
        Ok(Self { _db: Some(db), inner: conn })
    }

    /// Create a connection from an existing `libsql::Database` instance.
    /// Used by `DatabaseManager` where the Database is long-lived.
    pub(crate) fn from_database(db: &libsql::Database) -> Result<Self> {
        let conn = db.connect()?;
        Ok(Self { _db: None, inner: conn })
    }

    pub(crate) fn from_owned_database(db: libsql::Database) -> Result<Self> {
        let conn = db.connect()?;
        Ok(Self { _db: Some(db), inner: conn })
    }

    /// Execute a SQL statement, returning the number of rows changed.
    ///
    /// ```rust
    /// conn.execute("INSERT INTO t (name) VALUES (?1)", params!["alice"])?;
    /// conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY)", ())?;
    /// ```
    pub fn execute<P: libsql::params::IntoParams>(&self, sql: &str, params: P) -> Result<usize> {
        run_async(self.inner.execute(sql, params)).map(|n| n as usize).map_err(Into::into)
    }

    /// Prepare a SQL statement for parameterized execution.
    pub fn prepare(&self, sql: &str) -> Result<Statement<'_>> {
        Ok(Statement { conn: &self.inner, sql: sql.to_string() })
    }

    /// Execute a query expecting exactly one row.
    ///
    /// ```rust
    /// let name: String = conn.query_row(
    ///     "SELECT name FROM users WHERE id = ?1",
    ///     params![42],
    ///     |row| row.get(0),
    /// )?;
    /// ```
    pub fn query_row<T, P, F>(&self, sql: &str, params: P, f: F) -> Result<T>
    where
        P: libsql::params::IntoParams,
        F: FnOnce(&Row) -> Result<T>,
    {
        run_async(async {
            let mut rows = self.inner.query(sql, params).await.map_err(DbError::from)?;
            match rows.next().await.map_err(DbError::from)? {
                Some(row) => f(&Row { inner: row }),
                None => Err(DbError::QueryReturnedNoRows),
            }
        })
    }

    /// Execute a batch of SQL statements (separated by `;`).
    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        run_async(self.inner.execute_batch(sql)).map(|_| ()).map_err(Into::into)
    }

    /// Return the row ID of the most recent successful INSERT on this connection.
    pub fn last_insert_rowid(&self) -> i64 {
        self.inner.last_insert_rowid()
    }

    /// Get a reference to the underlying `libsql::Connection` for advanced usage.
    pub fn inner(&self) -> &libsql::Connection {
        &self.inner
    }
}

// ─── DatabaseManager ────────────────────────────────────────────────────────

pub const CORE_DATABASE_NAMES: &[&str] = &[
    "system.db",
    "llm.db",
    "assistant.db",
    "mcp.db",
    "conversation.db",
    "plugin.db",
    "artifacts.db",
];

const SYNC_NAMESPACE_ROUTE_SEGMENT: &str = "dev";

fn sanitize_namespace_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

pub fn database_namespace(db_name: &str) -> String {
    let stem = db_name.strip_suffix(".db").unwrap_or(db_name);
    sanitize_namespace_component(stem)
}

pub fn sync_url_for_database(base_url: &str, db_name: &str) -> Result<String> {
    let namespace = database_namespace(db_name);
    let mut url = Url::parse(base_url)
        .map_err(|err| DbError::Custom(format!("Invalid sync server URL `{base_url}`: {err}")))?;

    let mut segments = url
        .path_segments()
        .map(|current| {
            current.filter(|segment| !segment.is_empty()).map(str::to_string).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    segments.push(SYNC_NAMESPACE_ROUTE_SEGMENT.to_string());
    segments.push(namespace);

    {
        let mut path_segments = url.path_segments_mut().map_err(|_| {
            DbError::Custom(format!("Sync server URL `{base_url}` cannot be used as a base path"))
        })?;
        path_segments.clear();
        for segment in &segments {
            path_segments.push(segment);
        }
    }

    Ok(url.to_string())
}

pub(crate) fn open_remote_database_connection(
    base_url: &str,
    auth_token: &str,
    db_name: &str,
) -> Result<Connection> {
    let sync_url = sync_url_for_database(base_url, db_name)?;
    let db = run_async(libsql::Builder::new_remote(sync_url, auth_token.to_string()).build())?;
    Connection::from_owned_database(db)
}

pub(crate) fn sync_metadata_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}-info", path.to_string_lossy()))
}

/// Describes how a database is opened.
#[derive(Debug, Clone)]
pub enum DatabaseMode {
    /// Local-only, no sync.
    Local,
    /// Embedded replica that syncs with a remote sqld instance.
    Synced { url: String, auth_token: String },
}

/// Manages `libsql::Database` instances for all AIPP database files.
///
/// Stored in Tauri managed state so that every command handler can obtain
/// a `Connection` from the shared, long-lived `Database` objects.
pub struct DatabaseManager {
    databases: std::sync::RwLock<HashMap<String, std::sync::Arc<libsql::Database>>>,
    db_dir: std::path::PathBuf,
    mode: DatabaseMode,
}

impl DatabaseManager {
    /// Create a new manager. Databases are opened lazily on first `connect()`.
    pub fn new(db_dir: std::path::PathBuf, mode: DatabaseMode) -> Self {
        Self { databases: std::sync::RwLock::new(HashMap::new()), db_dir, mode }
    }

    /// Get or create a `Connection` for the named database file (e.g. `"system.db"`).
    pub fn connect(&self, db_name: &str) -> Result<Connection> {
        let db = self.get_or_create_database(db_name)?;
        Connection::from_database(&db)
    }

    /// Trigger a sync for a specific database (no-op in Local mode).
    pub fn sync(&self, db_name: &str) -> Result<()> {
        if matches!(self.mode, DatabaseMode::Local) {
            return Ok(());
        }
        let db = self.get_or_create_database(db_name)?;
        run_async(db.sync()).map_err(DbError::from)?;
        Ok(())
    }

    /// Trigger sync for all open databases.
    pub fn sync_all(&self) -> Result<()> {
        if matches!(self.mode, DatabaseMode::Local) {
            return Ok(());
        }
        for db_name in Self::discover_database_names(&self.db_dir)? {
            self.sync(&db_name)?;
        }
        Ok(())
    }

    pub fn discover_database_names(db_dir: &Path) -> Result<Vec<String>> {
        let mut db_names = BTreeSet::new();
        for db_name in CORE_DATABASE_NAMES {
            db_names.insert((*db_name).to_string());
        }

        if db_dir.exists() {
            for entry in std::fs::read_dir(db_dir).map_err(|err| {
                DbError::Custom(format!(
                    "Failed to read managed database directory `{}`: {err}",
                    db_dir.display()
                ))
            })? {
                let entry = entry.map_err(|err| {
                    DbError::Custom(format!(
                        "Failed to read managed database entry in `{}`: {err}",
                        db_dir.display()
                    ))
                })?;
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if file_name.ends_with(".db") {
                    db_names.insert(file_name.to_string());
                }
            }
        }

        Ok(db_names.into_iter().collect())
    }

    fn get_or_create_database(&self, db_name: &str) -> Result<std::sync::Arc<libsql::Database>> {
        // Fast path: read lock
        {
            let dbs = self.databases.read().unwrap();
            if let Some(db) = dbs.get(db_name) {
                return Ok(db.clone());
            }
        }
        // Slow path: write lock, create database
        let mut dbs = self.databases.write().unwrap();
        // Double-check after acquiring write lock
        if let Some(db) = dbs.get(db_name) {
            return Ok(db.clone());
        }

        let path = self.db_dir.join(db_name);
        let path_str = path
            .to_str()
            .ok_or_else(|| DbError::Custom("Invalid UTF-8 in database path".into()))?;

        let db = match &self.mode {
            DatabaseMode::Local => run_async(libsql::Builder::new_local(path_str).build())?,
            DatabaseMode::Synced { url, auth_token } => {
                let sync_url = sync_url_for_database(url, db_name)?;
                run_async(
                    libsql::Builder::new_synced_database(path_str, sync_url, auth_token.clone())
                        .build(),
                )?
            }
        };

        let arc = std::sync::Arc::new(db);
        dbs.insert(db_name.to_string(), arc.clone());
        Ok(arc)
    }

    /// Get the database directory path.
    pub fn db_dir(&self) -> &Path {
        &self.db_dir
    }

    /// Get the current database mode.
    pub fn mode(&self) -> &DatabaseMode {
        &self.mode
    }
}

struct ManagedDatabaseInner {
    manager: RwLock<DatabaseManager>,
}

/// Global/Tauri-managed wrapper around `DatabaseManager`.
///
/// `Connection::open(path)` automatically routes through this manager when the
/// target path is inside the managed DB directory, which lets the existing DB
/// modules keep using `Connection::open(...)` while transparently gaining
/// embedded-replica support.
#[derive(Clone)]
pub struct ManagedDatabaseState {
    inner: Arc<ManagedDatabaseInner>,
}

static GLOBAL_DATABASE_STATE: OnceLock<ManagedDatabaseState> = OnceLock::new();

impl ManagedDatabaseState {
    pub fn install(manager: DatabaseManager) -> Self {
        let state = GLOBAL_DATABASE_STATE
            .get_or_init(|| ManagedDatabaseState {
                inner: Arc::new(ManagedDatabaseInner {
                    manager: RwLock::new(DatabaseManager::new(
                        manager.db_dir().to_path_buf(),
                        manager.mode().clone(),
                    )),
                }),
            })
            .clone();
        state.replace(manager);
        state
    }

    pub fn global() -> Option<Self> {
        GLOBAL_DATABASE_STATE.get().cloned()
    }

    pub fn replace(&self, manager: DatabaseManager) {
        let mut guard = self.inner.manager.write().unwrap();
        *guard = manager;
    }

    pub fn connect(&self, db_name: &str) -> Result<Connection> {
        self.inner.manager.read().unwrap().connect(db_name)
    }

    pub fn sync_all(&self) -> Result<()> {
        self.inner.manager.read().unwrap().sync_all()
    }

    pub fn db_dir(&self) -> PathBuf {
        self.inner.manager.read().unwrap().db_dir().to_path_buf()
    }

    fn connect_for_path(&self, path: &Path) -> Option<Result<Connection>> {
        let db_name = path.file_name()?.to_str()?;
        let db_dir = self.db_dir();
        let parent = path.parent()?;
        if parent == db_dir.as_path() {
            Some(self.connect(db_name))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_connection_open_in_memory() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)", ()).unwrap();
        conn.execute("INSERT INTO test (id, name) VALUES (?1, ?2)", params![1, "alice"]).unwrap();

        let name: String = conn
            .query_row("SELECT name FROM test WHERE id = ?1", params![1], |row| row.get(0))
            .unwrap();
        assert_eq!(name, "alice");
    }

    #[test]
    fn test_statement_query_map() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)", ()).unwrap();
        conn.execute("INSERT INTO test (name) VALUES (?1)", params!["alice"]).unwrap();
        conn.execute("INSERT INTO test (name) VALUES (?1)", params!["bob"]).unwrap();

        let mut stmt = conn.prepare("SELECT id, name FROM test ORDER BY id").unwrap();
        let names: Vec<String> =
            stmt.query_map((), |row| row.get(1)).unwrap().collect::<Result<Vec<_>>>().unwrap();
        assert_eq!(names, vec!["alice", "bob"]);
    }

    #[test]
    fn test_optional_extension() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)", ()).unwrap();

        let result: Option<String> = conn
            .query_row("SELECT name FROM test WHERE id = ?1", params![999], |row| row.get(0))
            .optional()
            .unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_row_get_with_type_annotation() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER, name TEXT, score REAL)", ()).unwrap();
        conn.execute("INSERT INTO test VALUES (?1, ?2, ?3)", params![42, "alice", 99.5]).unwrap();

        conn.query_row("SELECT id, name, score FROM test", (), |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let score: f64 = row.get(2)?;
            assert_eq!(id, 42);
            assert_eq!(name, "alice");
            assert_eq!((score - 99.5).abs() < f64::EPSILON, true);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_execute_batch() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE a (id INTEGER);
             CREATE TABLE b (id INTEGER);
             INSERT INTO a VALUES (1);
             INSERT INTO b VALUES (2);",
        )
        .unwrap();

        let a_id: i64 = conn.query_row("SELECT id FROM a", (), |row| row.get(0)).unwrap();
        let b_id: i64 = conn.query_row("SELECT id FROM b", (), |row| row.get(0)).unwrap();
        assert_eq!(a_id, 1);
        assert_eq!(b_id, 2);
    }

    #[test]
    fn test_sync_url_for_database_uses_namespaces() {
        let sync_url = sync_url_for_database("https://sync.example.com/base", "system.db").unwrap();
        assert_eq!(sync_url, "https://sync.example.com/base/dev/system");
    }

    #[test]
    fn test_discover_database_names_includes_core_and_dynamic_databases() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("artifact-data-sample.db"), b"").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"").unwrap();

        let names = DatabaseManager::discover_database_names(dir.path()).unwrap();

        assert!(names.contains(&"system.db".to_string()));
        assert!(names.contains(&"artifact-data-sample.db".to_string()));
        assert!(!names.contains(&"notes.txt".to_string()));
    }

    #[test]
    fn test_u64_param_overflow_is_preserved_as_text() {
        assert_eq!(u64::MAX.into_db_param(), libsql::Value::Text(u64::MAX.to_string()));
    }
}
