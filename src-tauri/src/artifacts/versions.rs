//! Artifact 版本：每次 `show_artifact` 把 `<workspace>/artifacts/<artifact_key>/` 整个目录固定为一个快照。
//!
//! - 存在会话库 `artifact_version` 表（随会话级联删除，不参与跨设备同步，与工作区文件一致）；
//! - 与上一版完全相同时不新增版本；每个 Artifact 只保留最近 [`MAX_VERSIONS`] 个；
//! - 快照沿用比选工程的规则（`choices::snapshot`：单版 20 MiB、2048 个文件，跳过依赖与缓存目录）；
//! - 入口引用了目录里的其他文件时标为多文件（`multi_file`），预览走工程运行时而不是单文件模板。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::db::conversation_db::ConversationDatabase;
use crate::mcp::builtin_mcp::choices::snapshot::{self, Project};

/// 每个 Artifact 保留的版本数。
pub const MAX_VERSIONS: i64 = 30;
/// 工作区为每个 Artifact 写的元数据文件，不属于用户工程。
const METADATA_FILE: &str = "artifact.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactVersionSummary {
    pub version: i64,
    pub title: String,
    pub language: String,
    pub preview_type: String,
    pub entry_file: String,
    pub multi_file: bool,
    pub file_count: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactVersionDetail {
    #[serde(flatten)]
    pub summary: ArtifactVersionSummary,
    pub artifact_key: String,
    pub db_id: Option<String>,
    pub assistant_id: Option<i64>,
    /// 文件名 → base64 原始字节（与比选工程快照同格式）。
    pub files: Project,
}

impl ArtifactVersionDetail {
    pub fn entry_code(&self) -> Result<String> {
        let encoded = self
            .files
            .get(&self.summary.entry_file)
            .with_context(|| format!("版本 {} 缺少入口文件 {}", self.summary.version, self.summary.entry_file))?;
        Ok(String::from_utf8(STANDARD.decode(encoded)?)?)
    }
}

/// 要记录的版本信息（来自刚写入的 manifest 条目）。
pub struct NewVersion<'a> {
    pub conversation_id: i64,
    pub artifact_key: &'a str,
    pub title: &'a str,
    pub language: &'a str,
    pub preview_type: &'a str,
    pub entry_file: &'a str,
    pub db_id: Option<&'a str>,
    pub assistant_id: Option<i64>,
}

pub(crate) fn schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS artifact_version (
            conversation_id INTEGER NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
            artifact_key TEXT NOT NULL,
            version INTEGER NOT NULL,
            title TEXT NOT NULL,
            language TEXT NOT NULL,
            preview_type TEXT NOT NULL,
            entry_file TEXT NOT NULL,
            multi_file INTEGER NOT NULL DEFAULT 0,
            file_count INTEGER NOT NULL DEFAULT 1,
            files TEXT NOT NULL,
            digest TEXT NOT NULL,
            db_id TEXT,
            assistant_id INTEGER,
            created_at TEXT NOT NULL,
            PRIMARY KEY (conversation_id, artifact_key, version));",
    )?;
    // 多文件版本用 Bun 构建后的静态输出（与比选工程的 build 同格式）；旧库补列
    let has_build: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('artifact_version') WHERE name='build'")?
        .exists([])?;
    if !has_build {
        conn.execute("ALTER TABLE artifact_version ADD COLUMN build TEXT", [])?;
    }
    Ok(())
}

/// 保存某个版本的构建输出（文件名 → base64）。版本不存在时返回 false。
pub(crate) fn set_build_in(
    conn: &Connection,
    conversation_id: i64,
    artifact_key: &str,
    version: i64,
    build: &Project,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE artifact_version SET build=?4 WHERE conversation_id=?1 AND artifact_key=?2 AND version=?3",
        params![conversation_id, artifact_key, version, serde_json::to_string(build)?],
    )?;
    Ok(changed > 0)
}

/// 某个版本的构建输出（JSON），没有构建过时为 None。
pub(crate) fn get_build_in(
    conn: &Connection,
    conversation_id: i64,
    artifact_key: &str,
    version: i64,
) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT build FROM artifact_version WHERE conversation_id=?1 AND artifact_key=?2 AND version=?3",
            params![conversation_id, artifact_key, version],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

pub fn set_build(app: &AppHandle, conversation_id: i64, artifact_key: &str, version: i64, build: &Project) -> Result<bool> {
    set_build_in(&db(app)?, conversation_id, artifact_key, version, build)
}

pub fn get_build(app: &AppHandle, conversation_id: i64, artifact_key: &str, version: i64) -> Result<Option<String>> {
    get_build_in(&db(app)?, conversation_id, artifact_key, version)
}

fn db(app: &AppHandle) -> Result<Connection> {
    let conn = ConversationDatabase::new(app)?.get_connection()?;
    schema(&conn)?;
    Ok(conn)
}

/// 固定 Artifact 目录为快照。超过快照上限时只保留入口文件，保证仍能回看该版本。
pub fn snapshot_dir(artifact_dir: &Path, entry_file: &str) -> Result<Project> {
    let root = artifact_dir.canonicalize().with_context(|| format!("读取 {}", artifact_dir.display()))?;
    let mut files = Project::new();
    let mut budget = snapshot::LIMIT;
    match snapshot::collect_tree(&root, &root, &mut files, &mut budget, 0) {
        Ok(()) => {
            files.remove(METADATA_FILE);
            Ok(files)
        }
        Err(e) => {
            tracing::warn!(error = %e, dir = %artifact_dir.display(), "Artifact snapshot fell back to entry file only");
            let bytes = std::fs::read(root.join(entry_file))?;
            Ok(BTreeMap::from([(entry_file.to_string(), STANDARD.encode(bytes))]))
        }
    }
}

fn digest(files: &Project, entry_file: &str, preview_type: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(entry_file.as_bytes());
    hasher.update([0]);
    hasher.update(preview_type.as_bytes());
    for (name, content) in files {
        hasher.update([0]);
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(content.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

static IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:\bfrom\s*|\bimport\s*\(?\s*|\brequire\s*\(\s*)["'](\.{1,2}/[^"']+)["']"#).unwrap()
});
static ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\b(?:src|href)\s*=\s*["']([^"']+)["']"#).unwrap());
static CSS_URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"url\(\s*["']?([^"')\s]+)["']?\s*\)"#).unwrap());

/// 把入口里的相对引用解析为快照内的文件名（与前端工程运行时的解析规则一致）。
fn resolve_reference(files: &Project, from: &str, spec: &str) -> Option<String> {
    if spec.is_empty()
        || spec.starts_with('#')
        || spec.starts_with("//")
        || spec.starts_with('/')
        || spec.contains(':')
    {
        return None;
    }
    let spec = spec.split(['?', '#']).next().unwrap_or(spec);
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for part in spec.split('/') {
        match part {
            ".." => {
                parts.pop()?;
            }
            "" | "." => {}
            other => parts.push(other),
        }
    }
    let name = parts.join("/");
    let suffixes = ["", ".ts", ".tsx", ".js", ".jsx", ".vue", ".json", ".css", "/index.ts", "/index.tsx", "/index.js"];
    suffixes
        .iter()
        .map(|suffix| format!("{name}{suffix}"))
        .chain(std::iter::once(format!("public/{name}")))
        .find(|candidate| candidate != from && files.contains_key(candidate))
}

/// 入口是否引用了快照里的其他文件（相对 import、src/href、CSS url）。
pub fn references_sibling_files(files: &Project, entry_file: &str, entry_code: &str) -> bool {
    let specs = IMPORT_RE
        .captures_iter(entry_code)
        .chain(ATTR_RE.captures_iter(entry_code))
        .chain(CSS_URL_RE.captures_iter(entry_code))
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()));
    for spec in specs {
        if resolve_reference(files, entry_file, &spec).is_some() {
            return true;
        }
    }
    false
}

/// 记录一个版本；与最新版本内容相同则直接返回最新版本号。
pub fn record_version(
    app: &AppHandle,
    artifact_dir: &Path,
    new: NewVersion<'_>,
) -> Result<ArtifactVersionSummary> {
    let files = snapshot_dir(artifact_dir, new.entry_file)?;
    let conn = db(app)?;
    record_version_in(&conn, files, new, &chrono::Utc::now().to_rfc3339())
}

pub(crate) fn record_version_in(
    conn: &Connection,
    files: Project,
    new: NewVersion<'_>,
    now: &str,
) -> Result<ArtifactVersionSummary> {
    let entry = files.get(new.entry_file).context("快照缺少入口文件")?;
    let entry_code = String::from_utf8(STANDARD.decode(entry)?).unwrap_or_default();
    let multi_file = references_sibling_files(&files, new.entry_file, &entry_code);
    let digest = digest(&files, new.entry_file, new.preview_type);

    let latest: Option<(i64, String)> = conn
        .query_row(
            "SELECT version, digest FROM artifact_version WHERE conversation_id=?1 AND artifact_key=?2
             ORDER BY version DESC LIMIT 1",
            params![new.conversation_id, new.artifact_key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((version, previous)) = &latest {
        if *previous == digest {
            // 内容没变（例如只改了标题）：沿用该版本，只更新展示信息。
            conn.execute(
                "UPDATE artifact_version SET title=?4, language=?5, db_id=?6, assistant_id=?7
                 WHERE conversation_id=?1 AND artifact_key=?2 AND version=?3",
                params![
                    new.conversation_id,
                    new.artifact_key,
                    version,
                    new.title,
                    new.language,
                    new.db_id,
                    new.assistant_id
                ],
            )?;
            return get_summary(conn, new.conversation_id, new.artifact_key, *version)?
                .context("版本已删除");
        }
    }
    let version = latest.map(|(v, _)| v + 1).unwrap_or(1);
    conn.execute(
        "INSERT INTO artifact_version (conversation_id, artifact_key, version, title, language, preview_type,
            entry_file, multi_file, file_count, files, digest, db_id, assistant_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            new.conversation_id,
            new.artifact_key,
            version,
            new.title,
            new.language,
            new.preview_type,
            new.entry_file,
            multi_file,
            files.len() as i64,
            serde_json::to_string(&files)?,
            digest,
            new.db_id,
            new.assistant_id,
            now
        ],
    )?;
    conn.execute(
        "DELETE FROM artifact_version WHERE conversation_id=?1 AND artifact_key=?2 AND version <= ?3",
        params![new.conversation_id, new.artifact_key, version - MAX_VERSIONS],
    )?;
    get_summary(conn, new.conversation_id, new.artifact_key, version)?.context("版本写入后读取失败")
}

const SUMMARY_COLUMNS: &str =
    "version, title, language, preview_type, entry_file, multi_file, file_count, created_at";

fn read_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArtifactVersionSummary> {
    Ok(ArtifactVersionSummary {
        version: row.get(0)?,
        title: row.get(1)?,
        language: row.get(2)?,
        preview_type: row.get(3)?,
        entry_file: row.get(4)?,
        multi_file: row.get(5)?,
        file_count: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn get_summary(
    conn: &Connection,
    conversation_id: i64,
    artifact_key: &str,
    version: i64,
) -> Result<Option<ArtifactVersionSummary>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {SUMMARY_COLUMNS} FROM artifact_version
                 WHERE conversation_id=?1 AND artifact_key=?2 AND version=?3"
            ),
            params![conversation_id, artifact_key, version],
            read_summary,
        )
        .optional()?)
}

pub(crate) fn list_versions_in(
    conn: &Connection,
    conversation_id: i64,
    artifact_key: &str,
) -> Result<Vec<ArtifactVersionSummary>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SUMMARY_COLUMNS} FROM artifact_version
         WHERE conversation_id=?1 AND artifact_key=?2 ORDER BY version DESC"
    ))?;
    let rows = stmt.query_map(params![conversation_id, artifact_key], read_summary)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 读取某个版本；`version` 为空时取最新。
pub(crate) fn get_version_in(
    conn: &Connection,
    conversation_id: i64,
    artifact_key: &str,
    version: Option<i64>,
) -> Result<Option<ArtifactVersionDetail>> {
    let row = conn
        .query_row(
            &format!(
                "SELECT {SUMMARY_COLUMNS}, files, db_id, assistant_id FROM artifact_version
                 WHERE conversation_id=?1 AND artifact_key=?2 AND (?3 IS NULL OR version=?3)
                 ORDER BY version DESC LIMIT 1"
            ),
            params![conversation_id, artifact_key, version],
            |r| Ok((read_summary(r)?, r.get::<_, String>(8)?, r.get(9)?, r.get(10)?)),
        )
        .optional()?;
    let Some((summary, files, db_id, assistant_id)) = row else { return Ok(None) };
    Ok(Some(ArtifactVersionDetail {
        summary,
        artifact_key: artifact_key.to_string(),
        db_id,
        assistant_id,
        files: serde_json::from_str(&files)?,
    }))
}

pub fn list_versions(
    app: &AppHandle,
    conversation_id: i64,
    artifact_key: &str,
) -> Result<Vec<ArtifactVersionSummary>> {
    list_versions_in(&db(app)?, conversation_id, artifact_key)
}

pub fn get_version(
    app: &AppHandle,
    conversation_id: i64,
    artifact_key: &str,
    version: Option<i64>,
) -> Result<Option<ArtifactVersionDetail>> {
    get_version_in(&db(app)?, conversation_id, artifact_key, version)
}

/// 某个 Artifact 的最新版本概况。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatestVersion {
    pub version: i64,
    pub count: i64,
    pub multi_file: bool,
}

/// 会话里每个 Artifact 的最新版本号、版本数与是否多文件。
pub fn latest_versions(app: &AppHandle, conversation_id: i64) -> Result<BTreeMap<String, LatestVersion>> {
    latest_versions_in(&db(app)?, conversation_id)
}

pub(crate) fn latest_versions_in(
    conn: &Connection,
    conversation_id: i64,
) -> Result<BTreeMap<String, LatestVersion>> {
    let mut stmt = conn.prepare(
        "SELECT v.artifact_key, v.version, v.multi_file, c.n FROM artifact_version v
         JOIN (SELECT artifact_key, MAX(version) AS latest, COUNT(*) AS n FROM artifact_version
               WHERE conversation_id=?1 GROUP BY artifact_key) c
           ON c.artifact_key = v.artifact_key AND c.latest = v.version
         WHERE v.conversation_id=?1",
    )?;
    let rows = stmt.query_map([conversation_id], |r| {
        Ok((r.get::<_, String>(0)?, LatestVersion { version: r.get(1)?, multi_file: r.get(2)?, count: r.get(3)? }))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 交给模型读取单个文件时的上限（字符数）。
const MAX_MODEL_FILE_CHARS: usize = 100_000;

/// `get_artifact` 工具：某版本的概况、文件清单，以及一个文件（默认入口）的文本内容。
pub fn describe_version_for_model(
    app: &AppHandle,
    conversation_id: i64,
    artifact_key: &str,
    version: Option<i64>,
    file: Option<&str>,
) -> Result<serde_json::Value> {
    let detail = get_version(app, conversation_id, artifact_key, version)?.with_context(|| match version {
        Some(v) => format!("Artifact {artifact_key} 没有版本 {v}"),
        None => format!("Artifact {artifact_key} 还没有版本记录（先调用 show_artifact 发布）"),
    })?;
    let versions = list_versions(app, conversation_id, artifact_key)?;
    let name = file.unwrap_or(&detail.summary.entry_file);
    let encoded = detail
        .files
        .get(name)
        .with_context(|| format!("版本 {} 中没有文件 {name}", detail.summary.version))?;
    let bytes = STANDARD.decode(encoded)?;
    let (content, note) = match String::from_utf8(bytes) {
        Ok(text) if text.chars().count() > MAX_MODEL_FILE_CHARS => (
            Some(text.chars().take(MAX_MODEL_FILE_CHARS).collect::<String>()),
            Some(format!("内容超过 {MAX_MODEL_FILE_CHARS} 字符，已截断")),
        ),
        Ok(text) => (Some(text), None),
        Err(_) => (None, Some("二进制文件，未返回内容".to_string())),
    };
    let files: Vec<serde_json::Value> = detail
        .files
        .iter()
        .map(|(name, encoded)| serde_json::json!({ "name": name, "bytes": encoded.len() * 3 / 4 }))
        .collect();
    Ok(serde_json::json!({
        "artifact_key": detail.artifact_key,
        "version": detail.summary.version,
        "latest_version": versions.first().map(|v| v.version),
        "versions": versions.iter().map(|v| serde_json::json!({"version": v.version, "created_at": v.created_at})).collect::<Vec<_>>(),
        "title": detail.summary.title,
        "preview_type": detail.summary.preview_type,
        "entry_file": detail.summary.entry_file,
        "multi_file": detail.summary.multi_file,
        "files": files,
        "file": name,
        "content": content,
        "note": note,
    }))
}

#[tauri::command]
pub fn list_artifact_versions(
    app_handle: AppHandle,
    conversation_id: i64,
    artifact_key: String,
) -> Result<Vec<ArtifactVersionSummary>, String> {
    list_versions(&app_handle, conversation_id, &artifact_key).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn get_artifact_version(
    app_handle: AppHandle,
    conversation_id: i64,
    artifact_key: String,
    version: Option<i64>,
) -> Result<Option<ArtifactVersionDetail>, String> {
    get_version(&app_handle, conversation_id, &artifact_key, version).map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
#[path = "versions_tests.rs"]
mod tests;
