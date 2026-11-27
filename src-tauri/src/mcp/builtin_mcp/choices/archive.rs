use super::{card, db, snapshot::{Candidate, Project}};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use crate::utils::consent::Consent;

pub const PREFIX: &str = "AIPP_CHOICE_ARCHIVE_V1\n";
pub const REFERENCE: &str = "AIPP_CHOICE_PREVIEW_V1\n";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reference {
    conversation_id: i64,
    request_id: String,
    candidate_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct Archive {
    pub candidate: Candidate,
    pub files: Option<Project>,
    pub build: Option<Project>,
}

impl Archive {
    pub fn encode(&self) -> Result<String> {
        // Also keep the payload inert in older HTML viewers.
        Ok(format!("{PREFIX}{}", serde_json::to_string(self)?.replace('<', "\\u003c")))
    }
    pub fn decode(code: &str) -> Result<Self> {
        let archive: Self = serde_json::from_str(code.strip_prefix(PREFIX).context("收藏不是比选快照")?)?;
        ensure!(archive.candidate.content.is_some(), "收藏缺少候选内容");
        if archive.candidate.kind == "artifact" {
            let entry = archive.candidate.entry_file.as_ref().context("收藏缺少工程入口")?;
            ensure!(archive.files.as_ref().is_some_and(|files| files.contains_key(entry)), "收藏缺少入口文件 {entry}");
        }
        Ok(archive)
    }
}

pub fn capture(app: &AppHandle, code: &str) -> Result<String> {
    let conn = db(app)?;
    capture_from(&conn, code)
}

fn capture_from(conn: &Connection, code: &str) -> Result<String> {
    let r: Reference = serde_json::from_str(code.strip_prefix(REFERENCE).context("缺少候选引用")?)?;
    let request = card(conn, r.conversation_id, Some(&r.request_id), None)?
        .context("原会话的比选快照已不存在")?.request;
    let candidate = request.candidates.into_iter().find(|c| c.id == r.candidate_id)
        .context("原快照中没有该候选")?;
    ensure!(candidate.content.is_some(), "请先加载候选内容再收藏");
    let mut archive = Archive { candidate, files: None, build: None };
    if archive.candidate.kind == "artifact" {
        let (files, build): (String, Option<String>) = conn.query_row(
            "SELECT p.files,b.files FROM choice_project p LEFT JOIN choice_build b USING(request_id,candidate_id)
             WHERE p.request_id=?1 AND p.candidate_id=?2", params![r.request_id,r.candidate_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?.context("原工程快照缺失")?;
        archive.files = Some(serde_json::from_str(&files)?);
        archive.build = build.map(|json| serde_json::from_str(&json)).transpose()?;
    }
    archive.encode()
}

fn stored(app: &AppHandle, id: i64) -> Result<(String, Archive)> {
    let db = crate::artifacts::artifacts_db::ArtifactsDatabase::new(app)?;
    let raw: String = db.conn.query_row(
        "SELECT code FROM artifacts_collection WHERE id=?1 AND artifact_type='html'",
        [id], |row| row.get(0),
    ).context("找不到该收藏")?;
    let archive = Archive::decode(&raw)?;
    Ok((raw, archive))
}

#[tauri::command]
pub fn get_collected_choice(app_handle: AppHandle, collection_id: i64) -> Result<serde_json::Value, String> {
    let (_, archive) = stored(&app_handle, collection_id).map_err(|e| format!("读取收藏 {collection_id}：{e:#}"))?;
    Ok(serde_json::json!({"candidate":archive.candidate, "files":archive.files}))
}

#[tauri::command]
pub async fn build_collected_choice(
    app_handle: AppHandle, collection_id: i64, output_dir: String, confirmed_prompt: Option<String>,
) -> Result<Consent<()>, String> {
    async fn run(app: &AppHandle, id: i64, output: &str, confirmed: Option<&str>) -> Result<Consent<()>> {
        let (raw, mut archive) = stored(app, id)?;
        if archive.build.is_some() { return Ok(Consent::Done { value: () }) }
        let files = archive.files.as_ref().context("收藏没有工程文件")?;
        archive.build = Some(match super::build::compile_project(app, &archive.candidate, files, output, confirmed).await? {
            Consent::Done { value } => value,
            Consent::Confirm { prompt } => return Ok(Consent::Confirm { prompt }),
        });
        let db = crate::artifacts::artifacts_db::ArtifactsDatabase::new(app)?;
        let updated = db.conn.execute("UPDATE artifacts_collection SET code=?1 WHERE id=?2 AND code=?3",
            params![archive.encode()?, id, raw])?;
        if updated == 0 {
            ensure!(stored(app, id)?.1.build.is_some(), "构建期间收藏已变更，请重新打开");
        }
        crate::sync::schedule_sync_after_local_change(app);
        Ok(Consent::Done { value: () })
    }
    run(&app_handle, collection_id, &output_dir, confirmed_prompt.as_deref()).await.map_err(|e| format!("构建收藏 {collection_id}：{e:#}"))
}

#[tauri::command]
pub async fn open_collected_choice_build(
    app_handle: AppHandle, window: tauri::Window, collection_id: i64,
) -> Result<Option<super::build::BuildView>, String> {
    use sha2::{Digest, Sha256};
    let (_, archive) = stored(&app_handle, collection_id).map_err(|e| format!("读取收藏 {collection_id}：{e:#}"))?;
    let Some(build) = archive.build else { return Ok(None) };
    let data = serde_json::to_string(&build).map_err(|e| e.to_string())?;
    let key = (collection_id, "collection".into(), format!("{:x}", Sha256::digest(data.as_bytes())));
    super::build::open_snapshot(window.label(), key, &data, || super::build::bun(&app_handle))
        .await.map(Some).map_err(|e| format!("打开收藏构建预览：{e:#}"))
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
