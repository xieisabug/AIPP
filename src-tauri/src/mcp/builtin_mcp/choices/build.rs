use super::{
    db, project,
    snapshot::{self, Project},
};
use anyhow::{anyhow, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    time::timeout,
};

pub(super) fn bun(app: &AppHandle) -> Result<PathBuf> {
    crate::utils::bun_utils::BunUtils::get_bun_executable(app)
        .or_else(|_| which::which("bun"))
        .map_err(|e| anyhow!("未找到可运行的 Bun：{e}"))
}
fn command(exe: &Path) -> Command {
    let mut cmd = Command::new(exe);
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    cmd
}
async fn step(exe: &Path, dir: &Path, args: &[&str]) -> Result<()> {
    let mut log = tempfile::tempfile()?;
    let mut child = command(exe)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?)
        .spawn()
        .with_context(|| format!("启动 bun {}", args.join(" ")))?;
    let pid = child.id();
    let result = timeout(Duration::from_secs(600), child.wait()).await;
    if result.is_err() {
        if let Some(pid) = pid {
            let _ = tauri::async_runtime::spawn_blocking(move || {
                let _ = crate::artifacts::shared_components::kill_process_group_by_pid(pid);
            })
            .await;
        }
        let _ = child.kill().await;
        anyhow::bail!("bun {} 超过 10 分钟", args.join(" "));
    }
    let status = result??;
    if !status.success() {
        let len = log.metadata()?.len();
        log.seek(SeekFrom::Start(len.saturating_sub(8192)))?;
        let mut tail = Vec::new();
        log.read_to_end(&mut tail)?;
        anyhow::bail!(
            "bun {} 退出状态 {status}：{}",
            args.join(" "),
            String::from_utf8_lossy(&tail)
        );
    }
    Ok(())
}
async fn build(app: &AppHandle, cid: i64, id: &str, candidate: &str, output: &str) -> Result<()> {
    let (c, files) = project(app, cid, id, candidate)?;
    let result = compile_project(app, &c, &files, output).await?;
    db(app)?.execute("INSERT INTO choice_build(request_id,candidate_id,files) VALUES(?1,?2,?3) ON CONFLICT(request_id,candidate_id) DO NOTHING",
        params![id,candidate,serde_json::to_string(&result)?])?;
    Ok(())
}

pub(super) async fn compile_project(app: &AppHandle, c: &snapshot::Candidate, files: &Project, output: &str) -> Result<Project> {
    ensure!(cfg!(desktop), "移动端不支持本机构建");
    let output = snapshot::relative(output)?;
    let pkg = STANDARD.decode(files.get("package.json").context("工程缺少 package.json")?)?;
    let pkg: serde_json::Value = serde_json::from_slice(&pkg)?;
    let script = pkg["scripts"]["build"].as_str().context("package.json 缺少 scripts.build")?;
    let frozen = ["bun.lock", "bun.lockb"].iter().any(|key| files.contains_key(*key));
    let args = if frozen { vec!["install", "--frozen-lockfile"] } else { vec!["install"] };
    let prompt=format!("工程：{}\n\n将使用本机 Bun 在此版本的临时副本中执行：\nbun {}\nbun run build\n\nbuild 脚本：{}\n输出目录：{}\n\n安装及构建脚本使用当前用户权限，可访问本机文件和网络。",c.title,args.join(" "),script,output.display());
    let handle = app.clone();
    let approved = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .message(prompt)
            .title("确认安装依赖并构建工程")
            .buttons(MessageDialogButtons::OkCancelCustom("安装并构建".into(), "取消".into()))
            .blocking_show()
    })
    .await?;
    ensure!(approved, "用户取消了本次构建");
    let exe = bun(app)?;
    let work = tempfile::tempdir()?;
    for (name, bytes) in files {
        let path = work.path().join(snapshot::relative(name)?);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, STANDARD.decode(bytes)?)?;
    }
    step(&exe, work.path(), &args).await?;
    step(&exe, work.path(), &["run", "build"]).await?;
    let root = work.path().join(output).canonicalize().context("构建完成，但找不到指定输出目录")?;
    ensure!(root.starts_with(work.path().canonicalize()?), "输出目录指向工程以外");
    let mut result = Project::new();
    let mut budget = snapshot::LIMIT;
    snapshot::collect_tree(&root, &root, &mut result, &mut budget, 0)?;
    ensure!(result.contains_key("index.html"), "构建输出缺少 index.html");
    Ok(result)
}
#[tauri::command]
pub async fn build_choice_project(
    app_handle: AppHandle,
    conversation_id: i64,
    request_id: String,
    candidate_id: String,
    output_dir: String,
) -> Result<(), String> {
    build(&app_handle, conversation_id, &request_id, &candidate_id, &output_dir)
        .await
        .map_err(|e| format!("构建候选 {candidate_id}：{e:#}"))
}

#[derive(Serialize)]
pub struct BuildView {
    url: String,
    lease: String,
}
struct Server {
    url: String,
    child: Child,
    dir: tempfile::TempDir,
}
impl Server {
    async fn stop(mut self) {
        drop(self.child.stdin.take());
        if timeout(Duration::from_secs(2), self.child.wait()).await.is_err() {
            let _ = self.child.kill().await;
        }
        drop(self.dir);
    }
}

// The last view (including a window destroyed during startup) owns shutdown.
#[derive(Default)]
struct SharedServer(tokio::sync::Mutex<Option<Server>>);
impl Drop for SharedServer {
    fn drop(&mut self) {
        if let Some(server) = self.0.get_mut().take() {
            tauri::async_runtime::spawn(server.stop());
        }
    }
}

type BuildKey = (i64, String, String);
struct View {
    owner: String,
    key: BuildKey,
    server: Arc<SharedServer>,
}
static VIEWS: LazyLock<Mutex<HashMap<String, View>>> = LazyLock::new(Default::default);

fn reserve_view(owner: &str, key: BuildKey) -> (String, Arc<SharedServer>) {
    let mut views = VIEWS.lock().unwrap();
    let server = views.values().find(|v| v.key == key)
        .map(|v| Arc::clone(&v.server)).unwrap_or_default();
    let lease = uuid::Uuid::new_v4().simple().to_string();
    views.insert(lease.clone(), View { owner: owner.to_owned(), key, server: Arc::clone(&server) });
    (lease, server)
}

// Also release a reservation if the opening future is cancelled.
struct OpeningView(Option<String>);
impl Drop for OpeningView {
    fn drop(&mut self) {
        if let Some(lease) = self.0.take() {
            VIEWS.lock().unwrap().remove(&lease);
        }
    }
}
async fn open(
    app: &AppHandle,
    owner: &str,
    cid: i64,
    id: &str,
    candidate: &str,
) -> Result<Option<BuildView>> {
    let data: Option<String> = db(app)?
        .query_row(
            "SELECT b.files FROM choice_build b JOIN present_choices r USING(request_id)
        WHERE r.conversation_id=?1 AND r.request_id=?2 AND b.candidate_id=?3",
            params![cid, id, candidate],
            |r| r.get(0),
        )
        .optional()?;
    let Some(data) = data else { return Ok(None) };
    ensure!(cfg!(desktop), "移动端不启动桌面构建预览");
    let key = (cid, id.to_owned(), candidate.to_owned());
    Ok(Some(open_snapshot(owner, key, &data, || bun(app)).await?))
}

pub(super) async fn open_snapshot(
    owner: &str, key: BuildKey, data: &str, executable: impl FnOnce() -> Result<PathBuf>,
) -> Result<BuildView> {
    ensure!(cfg!(desktop), "移动端不启动桌面构建预览");
    let (lease, shared) = reserve_view(owner, key);
    let mut reservation = OpeningView(Some(lease.clone()));
    let mut slot = shared.0.lock().await;
    if let Some(server) = slot.as_mut() {
        if server.child.try_wait()?.is_some() {
            slot.take();
        }
    }
    if slot.is_none() {
        *slot = Some(start_server(&executable()?, data).await?);
    }
    ensure!(VIEWS.lock().unwrap().contains_key(&lease), "预览窗口已关闭");
    reservation.0.take();
    Ok(BuildView { url: slot.as_ref().unwrap().url.clone(), lease })
}

async fn start_server(exe: &Path, data: &str) -> Result<Server> {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("serve.mjs"), include_str!("serve-build.mjs"))?;
    fs::write(dir.path().join("frame-height.js"), super::document::FRAME_HEIGHT_SCRIPT)?;
    fs::write(dir.path().join("snapshot.json"), data)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut child = command(exe)
        .args(["serve.mjs", "snapshot.json", &token])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut lines = BufReader::new(child.stdout.take().context("预览进程没有 stdout")?).lines();
    let port =
        timeout(Duration::from_secs(10), lines.next_line()).await?.context("读取预览端口")?;
    let Some(port) = port else {
        let out = child.wait_with_output().await?;
        anyhow::bail!("预览进程退出 {}：{}", out.status, String::from_utf8_lossy(&out.stderr));
    };
    let port = port.parse::<u16>().context("无效预览端口")?;
    let url = format!("http://{token}.aipp-choice.localhost:{port}/");
    Ok(Server { url, child, dir })
}
#[tauri::command]
pub async fn open_choice_build(
    app_handle: AppHandle,
    window: tauri::Window,
    conversation_id: i64,
    request_id: String,
    candidate_id: String,
) -> Result<Option<BuildView>, String> {
    open(&app_handle, window.label(), conversation_id, &request_id, &candidate_id)
        .await
        .map_err(|e| format!("打开构建预览：{e:#}"))
}
#[tauri::command]
pub async fn close_choice_build(lease: String) {
    let view = VIEWS.lock().unwrap().remove(&lease);
    if let Some(mut shared) = view.and_then(|v| Arc::try_unwrap(v.server).ok()) {
        if let Some(server) = shared.0.get_mut().take() {
            server.stop().await;
        }
    }
}
pub fn close_window(label: &str) {
    VIEWS.lock().unwrap().retain(|_, view| view.owner != label);
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
