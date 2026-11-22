use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};
use tauri::{AppHandle, Manager};

pub const LIMIT: usize = 20 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub summary: Option<String>,
    pub language: Option<String>,
    pub content: Option<String>,
    pub url: Option<String>,
    pub artifact_key: Option<String>,
    pub entry_file: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Request {
    pub title: String,
    pub prompt: Option<String>,
    #[serde(default = "single")]
    pub selection_mode: String,
    #[serde(default = "tabs")]
    pub layout: String,
    #[serde(default = "yes")]
    pub allow_feedback: bool,
    pub candidates: Vec<Candidate>,
}
fn single() -> String {
    "single".into()
}
fn tabs() -> String {
    "tabs".into()
}
fn yes() -> bool {
    true
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.title.trim().is_empty() && (2..=6).contains(&self.candidates.len()),
            "比选需要标题及 2–6 个候选"
        );
        ensure!(
            matches!(self.selection_mode.as_str(), "single" | "multiple")
                && matches!(self.layout.as_str(), "tabs" | "grid" | "split"),
            "选择模式或布局无效"
        );
        let mut ids = HashSet::new();
        for c in &self.candidates {
            ensure!(
                !c.title.trim().is_empty() && !c.id.trim().is_empty() && ids.insert(&c.id),
                "候选标题、ID 必须非空且 ID 唯一"
            );
            match c.kind.as_str() {
                "artifact" => {
                    relative(c.artifact_key.as_deref().context("缺少工程根")?)?;
                    relative(c.entry_file.as_deref().context("缺少工程入口")?)?;
                    ensure!(c.content.is_none() && c.url.is_none(), "工程仅接受工作区引用");
                }
                "html" | "markdown" | "text" | "image" => ensure!(
                    (c.content.is_some() ^ c.url.is_some())
                        && c.artifact_key.is_none()
                        && c.entry_file.is_none(),
                    "候选 {} 内容来源冲突或缺失",
                    c.id
                ),
                _ => bail!("未知候选类型 {}", c.kind),
            }
        }
        Ok(())
    }
}

pub fn relative(raw: &str) -> Result<PathBuf> {
    let path = PathBuf::from(raw.replace('\\', "/"));
    ensure!(
        !raw.trim().is_empty()
            && !raw.contains(':')
            && path.components().all(|c| matches!(c, Component::Normal(_))),
        "无效工程相对路径 {raw}"
    );
    Ok(path)
}
pub type Project = BTreeMap<String, String>; // Original bytes, base64 encoded for binary-safe IPC.
fn read(path: &Path, budget: &mut usize) -> Result<Vec<u8>> {
    let file = fs::File::open(path).with_context(|| format!("读取 {}", path.display()))?;
    ensure!(
        file.metadata()?.is_file() && file.metadata()?.len() <= *budget as u64,
        "候选文件过大或不是普通文件：{}",
        path.display()
    );
    let mut bytes = Vec::new();
    file.take(*budget as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= *budget, "候选内容超过容量限制");
    *budget -= bytes.len();
    Ok(bytes)
}
pub(super) fn collect_tree(
    root: &Path,
    dir: &Path,
    files: &mut Project,
    budget: &mut usize,
    depth: usize,
) -> Result<()> {
    ensure!(depth < 64, "工程目录层级过深");
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir()
            && matches!(
                entry.file_name().to_str(),
                Some(".git" | "node_modules" | ".aipp" | ".vite" | "target")
            )
        {
            continue;
        }
        ensure!(
            kind.is_dir() || kind.is_file(),
            "工程包含不可快照的链接或设备：{}",
            path.display()
        );
        ensure!(path.canonicalize()?.starts_with(root), "工程文件超出根目录");
        if kind.is_dir() {
            collect_tree(root, &path, files, budget, depth + 1)?;
        } else {
            ensure!(files.len() < 2048, "工程超过 2048 个文件");
            let name = path
                .strip_prefix(root)?
                .to_str()
                .context("工程文件名不是 UTF-8")?
                .replace('\\', "/");
            relative(&name)?;
            files.insert(name, STANDARD.encode(read(&path, budget)?));
        }
    }
    Ok(())
}
pub fn to_content(kind: &str, bytes: &[u8], mime: &str) -> Result<String> {
    if kind == "image" {
        ensure!(mime.starts_with("image/"), "图片 MIME 类型无效：{mime}");
        Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
    } else {
        Ok(std::str::from_utf8(bytes).context("候选文本需要 UTF-8")?.to_owned())
    }
}
pub(super) fn workspace_root(app: &AppHandle, cid: i64, path: &Path) -> Result<PathBuf> {
    let expected = app
        .path()
        .app_data_dir()?
        .canonicalize()?
        .join("artifact_workspaces")
        .join(format!("conversation_{cid}"))
        .join("artifacts");
    let actual = path.canonicalize()?;
    ensure!(actual == expected, "工程工作区不能指向其他目录");
    Ok(actual)
}
pub fn prepare(
    app: &AppHandle,
    cid: i64,
    req: &mut Request,
) -> Result<(BTreeMap<String, Project>, usize)> {
    req.validate()?;
    let mut projects = BTreeMap::new();
    let mut budget = LIMIT;
    for c in &mut req.candidates {
        if c.kind == "artifact" {
            let ws = crate::artifacts::workspace::get_artifact_workspace(app, cid)
                .map_err(anyhow::Error::msg)?;
            let base = workspace_root(app, cid, Path::new(&ws.artifacts_dir))?;
            let root = base.join(relative(c.artifact_key.as_deref().unwrap())?).canonicalize()?;
            ensure!(root.starts_with(&base) && root != base, "工程不在本会话工作区");
            let entry =
                relative(c.entry_file.as_deref().unwrap())?.to_string_lossy().replace('\\', "/");
            let mut files = Project::new();
            collect_tree(&root, &root, &mut files, &mut budget, 0)?;
            let encoded = files.get(&entry).with_context(|| format!("快照缺少入口 {entry}"))?;
            c.content = Some(String::from_utf8(STANDARD.decode(encoded)?)?);
            c.entry_file = Some(entry);
            projects.insert(c.id.clone(), files);
        } else if let Some(url) = c.url.as_deref() {
            if url.starts_with("https://") || url.starts_with("http://") {
                continue;
            }
            let path = Path::new(url);
            ensure!(path.is_absolute(), "候选 {} 需要本地绝对路径或 HTTP(S) URL", c.id);
            c.content = Some(to_content(
                &c.kind,
                &read(path, &mut budget)?,
                mime_guess::from_path(path).first_or_octet_stream().essence_str(),
            )?);
            c.url = None;
        } else if let Some(content) = &mut c.content {
            budget = budget
                .checked_sub(content.len())
                .ok_or_else(|| anyhow!("候选内容总计超过 20 MiB"))?;
            if c.kind == "image" {
                let (mime, encoded) = if content.starts_with("data:") {
                    let (header, data) = content.split_once(',').context("无效的图片 data URL")?;
                    let mime = header
                        .strip_prefix("data:image/")
                        .and_then(|s| s.strip_suffix(";base64"))
                        .context("图片需要 base64 编码")?;
                    ensure!(
                        !mime.is_empty()
                            && mime
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b)),
                        "无效图片 MIME"
                    );
                    (Some(format!("image/{mime}")), data)
                } else {
                    (None, content.as_str())
                };
                let bytes =
                    STANDARD.decode(encoded).context("图片需要有效的 base64 或 data:image URL")?;
                let mime = mime.unwrap_or_else(|| {
                    {
                        if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                            "image/jpeg"
                        } else if bytes.starts_with(b"GIF8") {
                            "image/gif"
                        } else if bytes.get(8..12) == Some(b"WEBP") {
                            "image/webp"
                        } else {
                            "image/png"
                        }
                    }
                    .into()
                });
                *content = to_content("image", &bytes, &mime)?;
            }
        }
    }
    Ok((projects, budget))
}
