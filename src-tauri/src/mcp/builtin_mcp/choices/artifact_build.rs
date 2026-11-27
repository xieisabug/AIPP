//! 会话里多文件 Artifact 版本的 Bun 构建：与比选工程同一条路径（应用内确认 → 临时副本安装依赖并构建 → 固定输出），
//! 构建输出存进该版本（`artifact_version.build`），之后卡片、全屏、独立窗口打开同一版本都直接用构建结果。

use super::build::{self, BuildView};
use super::snapshot::Candidate;
use crate::utils::consent::Consent;
use anyhow::{ensure, Context, Result};
use tauri::AppHandle;

async fn build_version(
    app: &AppHandle, conversation_id: i64, artifact_key: &str, version: i64, output: &str, confirmed: Option<&str>,
) -> Result<Consent<()>> {
    let detail = crate::artifacts::versions::get_version(app, conversation_id, artifact_key, Some(version))?
        .with_context(|| format!("找不到 {artifact_key} 的 v{version}"))?;
    ensure!(detail.summary.multi_file, "只有多文件版本可以构建");
    let candidate = Candidate {
        id: "artifact".into(),
        title: detail.summary.title.clone(),
        kind: "artifact".into(),
        summary: None,
        language: Some(detail.summary.language.clone()),
        content: Some(detail.entry_code()?),
        url: None,
        artifact_key: Some(artifact_key.to_string()),
        entry_file: Some(detail.summary.entry_file.clone()),
    };
    let output = match build::compile_project(app, &candidate, &detail.files, output, confirmed).await? {
        Consent::Done { value } => value,
        Consent::Confirm { prompt } => return Ok(Consent::Confirm { prompt }),
    };
    ensure!(
        crate::artifacts::versions::set_build(app, conversation_id, artifact_key, version, &output)?,
        "构建期间该版本已被删除"
    );
    Ok(Consent::Done { value: () })
}

/// 用 Bun 安装依赖并构建某个多文件版本（执行前须经应用内确认，见 `utils::consent`）。
#[tauri::command]
pub async fn build_artifact_version(
    app_handle: AppHandle,
    conversation_id: i64,
    artifact_key: String,
    version: i64,
    output_dir: String,
    confirmed_prompt: Option<String>,
) -> Result<Consent<()>, String> {
    build_version(&app_handle, conversation_id, &artifact_key, version, &output_dir, confirmed_prompt.as_deref())
        .await
        .map_err(|e| format!("构建 {artifact_key} v{version}：{e:#}"))
}

/// 打开某个版本的构建输出；没有构建过时返回 None。用完调用 `close_choice_build` 释放。
#[tauri::command]
pub async fn open_artifact_version_build(
    app_handle: AppHandle,
    window: tauri::Window,
    conversation_id: i64,
    artifact_key: String,
    version: i64,
) -> Result<Option<BuildView>, String> {
    use sha2::{Digest, Sha256};
    let data = crate::artifacts::versions::get_build(&app_handle, conversation_id, &artifact_key, version)
        .map_err(|e| format!("读取构建结果：{e:#}"))?;
    let Some(data) = data else { return Ok(None) };
    let key = (conversation_id, format!("artifact:{artifact_key}:v{version}"), format!("{:x}", Sha256::digest(data.as_bytes())));
    build::open_snapshot(window.label(), key, &data, || build::bun(&app_handle))
        .await
        .map(Some)
        .map_err(|e| format!("打开构建预览：{e:#}"))
}
