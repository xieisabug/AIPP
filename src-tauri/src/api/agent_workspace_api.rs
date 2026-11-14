//! Agent 会话工作目录的 Git 操作，服务于聊天侧栏的“工作区”面板：
//! 分支选择/切换、提交（可选推送）、推送、改动统计与改动文件列表。
//!
//! - 工作目录只在服务端按 会话 → 助手 → Agent 工作目录 解析，前端只传会话 ID，不接受路径。
//! - 所有 git 调用都带 `-C <目录>`、`GIT_TERMINAL_PROMPT=0`（不会卡在凭据提示上）并有超时；
//!   只读查询额外带 `GIT_OPTIONAL_LOCKS=0`，不与 Agent 自己的 git 操作抢锁。
//! - 仓库内的操作统一在仓库根目录执行：工作目录可能是仓库子目录，而 `git add -A`
//!   与提交作用于整个仓库，统计和改动列表也按整个仓库计算，二者保持一致。
//! - 按目录执行的核心函数（`*_for_dir`）和输出解析都不依赖 `AppHandle`，可直接在临时仓库上测试。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tracing::warn;

use crate::api::assistant_api::get_acp_working_directory;
use crate::db::assistant_db::AssistantDatabase;
use crate::db::conversation_db::{ConversationDatabase, Repository};

/// 本地只读查询（status/diff/ls-files/for-each-ref 等）的超时。
const LOCAL_GIT_TIMEOUT: Duration = Duration::from_secs(30);
/// 会改动仓库的本地命令（add/commit/switch）的超时：可能触发 pre-commit / post-checkout 钩子。
const MUTATING_GIT_TIMEOUT: Duration = Duration::from_secs(120);
/// 推送涉及网络与远端钩子。
const PUSH_GIT_TIMEOUT: Duration = Duration::from_secs(180);
/// 空树对象（SHA-1），HEAD 还没有任何提交时作为 diff 基准。
pub(crate) const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/// 最多为多少个未跟踪文件统计行数（超出的仍计入文件数）。
const UNTRACKED_MEASURE_LIMIT: usize = 1000;
/// 超过该大小的未跟踪文件不统计行数。
const UNTRACKED_MAX_BYTES: u64 = 1024 * 1024;
/// 与 git 一致：前 8000 字节含 NUL 视为二进制文件。
const BINARY_SNIFF_BYTES: usize = 8000;
/// for-each-ref 输出格式：字段之间用 NUL 分隔，每个引用一行。
const BRANCH_REF_FORMAT: &str =
    "--format=%(refname)%00%(upstream)%00%(committerdate:unix)%00%(HEAD)%00%(symref)";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AgentGitDiffStat {
    /// 统计基准：上游分支名（如 `origin/main`）或 `HEAD`；还没有任何提交时为空（与空树比较）。
    pub base: Option<String>,
    pub files_changed: u32,
    pub insertions: u64,
    pub deletions: u64,
    pub untracked_files: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AgentGitStatus {
    pub working_directory: String,
    pub is_repo: bool,
    pub branch: Option<String>,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub has_remote: bool,
    pub has_uncommitted_changes: bool,
    pub diff: AgentGitDiffStat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentGitChangedFile {
    pub path: String,
    pub original_path: Option<String>,
    /// "modified" | "added" | "deleted" | "renamed" | "copied" | "untracked" | "conflicted" | "type_changed"
    pub status: String,
    pub insertions: u64,
    pub deletions: u64,
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentGitBranch {
    /// 短名，如 `main` 或 `origin/main`。
    pub name: String,
    pub full_ref: String,
    pub is_remote: bool,
    pub is_current: bool,
    pub upstream: Option<String>,
    /// 最近提交时间（Unix 秒）。
    pub committed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentGitCommitResult {
    /// 短提交号。
    pub commit: String,
    /// 提交说明首行。
    pub summary: String,
    pub pushed: bool,
    pub push_error: Option<String>,
    pub status: AgentGitStatus,
}

// ---------------------------------------------------------------------------
// Tauri 命令
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_agent_git_status(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<AgentGitStatus, String> {
    let directory = resolve_conversation_working_directory(&app_handle, conversation_id)?;
    git_status_for_dir(&directory).await.map_err(in_conversation(conversation_id))
}

#[tauri::command]
pub async fn list_agent_git_changes(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<Vec<AgentGitChangedFile>, String> {
    let directory = resolve_conversation_working_directory(&app_handle, conversation_id)?;
    list_changes_for_dir(&directory).await.map_err(in_conversation(conversation_id))
}

#[tauri::command]
pub async fn list_agent_git_branches(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<Vec<AgentGitBranch>, String> {
    let directory = resolve_conversation_working_directory(&app_handle, conversation_id)?;
    list_branches_for_dir(&directory).await.map_err(in_conversation(conversation_id))
}

#[tauri::command]
pub async fn switch_agent_git_branch(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    branch: String,
    is_remote: bool,
) -> Result<AgentGitStatus, String> {
    let directory = resolve_conversation_working_directory(&app_handle, conversation_id)?;
    switch_branch_for_dir(&directory, &branch, is_remote).await.map_err(|error| {
        warn!(conversation_id, branch = %branch, is_remote, error = %error, "agent workspace branch switch failed");
        in_conversation(conversation_id)(error)
    })
}

#[tauri::command]
pub async fn commit_agent_git_changes(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    message: String,
    push: bool,
) -> Result<AgentGitCommitResult, String> {
    let directory = resolve_conversation_working_directory(&app_handle, conversation_id)?;
    let result = commit_for_dir(&directory, &message, push).await.map_err(|error| {
        warn!(conversation_id, error = %error, "agent workspace commit failed");
        in_conversation(conversation_id)(error)
    })?;
    if let Some(push_error) = &result.push_error {
        warn!(conversation_id, commit = %result.commit, error = %push_error, "agent workspace push after commit failed");
    }
    Ok(result)
}

#[tauri::command]
pub async fn push_agent_git_branch(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<AgentGitStatus, String> {
    let directory = resolve_conversation_working_directory(&app_handle, conversation_id)?;
    push_for_dir(&directory).await.map_err(|error| {
        warn!(conversation_id, error = %error, "agent workspace push failed");
        in_conversation(conversation_id)(error)
    })
}

fn in_conversation(conversation_id: i64) -> impl Fn(String) -> String {
    move |error| format!("会话 {conversation_id}：{error}")
}

/// 会话 → 助手 → Agent 工作目录。只在服务端解析，不接受前端传入的路径。
fn resolve_conversation_working_directory(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
) -> Result<PathBuf, String> {
    let conversation = ConversationDatabase::new(app_handle)
        .map_err(|e| format!("打开会话数据库失败（会话 {conversation_id}）：{e}"))?
        .conversation_repo()
        .map_err(|e| format!("读取会话 {conversation_id} 失败：{e}"))?
        .read(conversation_id)
        .map_err(|e| format!("读取会话 {conversation_id} 失败：{e}"))?
        .ok_or_else(|| format!("会话 {conversation_id} 不存在"))?;
    let assistant_id = conversation
        .assistant_id
        .ok_or_else(|| format!("会话 {conversation_id} 没有关联助手，无法确定 Agent 工作目录"))?;
    let assistant = AssistantDatabase::new(app_handle)
        .map_err(|e| {
            format!("打开助手数据库失败（会话 {conversation_id}，助手 {assistant_id}）：{e}")
        })?
        .get_assistant(assistant_id)
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                format!("会话 {conversation_id} 关联的助手 {assistant_id} 不存在")
            }
            other => format!("读取会话 {conversation_id} 关联的助手 {assistant_id} 失败：{other}"),
        })?;
    if assistant.assistant_type != Some(4) {
        return Err(format!(
            "会话 {conversation_id} 关联的助手 {assistant_id} 不是 Agent 助手，没有工作目录"
        ));
    }
    let directory = get_acp_working_directory(app_handle.clone(), assistant_id).map_err(|e| {
        format!("解析会话 {conversation_id}（助手 {assistant_id}）的 Agent 工作目录失败：{e}")
    })?;
    if directory.trim().is_empty() {
        return Err(format!(
            "会话 {conversation_id}（助手 {assistant_id}）的 Agent 工作目录为空，请检查助手配置"
        ));
    }
    Ok(PathBuf::from(directory))
}

// ---------------------------------------------------------------------------
// 按目录执行的核心逻辑（不依赖 AppHandle）
// ---------------------------------------------------------------------------

/// 工作目录的 Git 状态。目录不是 Git 仓库时返回 `is_repo = false`，不报错；目录不存在时报错。
pub(crate) async fn git_status_for_dir(directory: &Path) -> Result<AgentGitStatus, String> {
    ensure_working_directory(directory)?;
    let working_directory = directory.display().to_string();
    match find_repo_root(directory).await? {
        Some(root) => collect_status(&root, working_directory).await,
        None => Ok(AgentGitStatus { working_directory, ..Default::default() }),
    }
}

/// 未提交的改动（相对 HEAD）：即“全部提交”会包含的内容——已暂存、未暂存与未跟踪文件。
pub(crate) async fn list_changes_for_dir(
    directory: &Path,
) -> Result<Vec<AgentGitChangedFile>, String> {
    let root = require_repo_root(directory).await?;
    let porcelain = read_porcelain_status(&root).await?;
    let base = if porcelain.unborn { EMPTY_TREE_SHA } else { "HEAD" };
    let (name_status, numstat, untracked) =
        tokio::join!(read_name_status(&root, base), read_numstat(&root, base), read_untracked(&root));
    Ok(merge_changed_files(&name_status?, &numstat?, &untracked?))
}

pub(crate) async fn list_branches_for_dir(directory: &Path) -> Result<Vec<AgentGitBranch>, String> {
    let root = require_repo_root(directory).await?;
    read_branches(&root).await
}

pub(crate) async fn switch_branch_for_dir(
    directory: &Path,
    branch: &str,
    is_remote: bool,
) -> Result<AgentGitStatus, String> {
    let root = require_repo_root(directory).await?;
    let (branches, remotes) = tokio::join!(read_branches(&root), list_remotes(&root));
    let args = plan_switch_args(&branches?, &remotes?, branch, is_remote)?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run_git(&root, &args, GitKind::Write).await?;
    collect_status(&root, directory.display().to_string())
        .await
        .map_err(|e| format!("已切换到分支 {branch}，但刷新 Git 状态失败：{e}"))
}

pub(crate) async fn commit_for_dir(
    directory: &Path,
    message: &str,
    push: bool,
) -> Result<AgentGitCommitResult, String> {
    validate_commit_message(message)?;
    let root = require_repo_root(directory).await?;

    let pending_args = ["status", "--porcelain=v2", "-z", "--untracked-files=normal"];
    let pending = run_git(&root, &pending_args, GitKind::Read).await?;
    let pending = parse_porcelain_v2(&pending)
        .map_err(|e| format!("解析 git status 输出失败（目录 {}）：{e}", root.display()))?;
    if pending.entries.is_empty() {
        return Err(format!("工作目录 {} 没有需要提交的改动", directory.display()));
    }

    run_git(&root, &["add", "-A"], GitKind::Write).await?;
    let commit_args = ["commit", "-F", "-"];
    let output = exec_git(&root, &commit_args, GitKind::Write, Some(message.as_bytes())).await?;
    if !output.status.success() {
        return Err(git_failure(&root, &commit_args, &output));
    }

    let log_args = ["log", "-1", "--no-show-signature", "--format=%h%x00%B", "HEAD"];
    let (commit, summary) = run_git(&root, &log_args, GitKind::Read)
        .await
        .map(|text| parse_commit_log(&text))
        .map_err(|e| format!("提交已完成，但读取提交信息失败：{e}"))?;

    let (pushed, push_error) = if push {
        match push_current_branch(&root).await {
            Ok(()) => (true, None),
            Err(error) => (false, Some(error)),
        }
    } else {
        (false, None)
    };

    let status = collect_status(&root, directory.display().to_string())
        .await
        .map_err(|e| format!("提交 {commit} 已完成，但刷新 Git 状态失败：{e}"))?;
    Ok(AgentGitCommitResult { commit, summary, pushed, push_error, status })
}

pub(crate) async fn push_for_dir(directory: &Path) -> Result<AgentGitStatus, String> {
    let root = require_repo_root(directory).await?;
    push_current_branch(&root).await?;
    collect_status(&root, directory.display().to_string())
        .await
        .map_err(|e| format!("推送已完成，但刷新 Git 状态失败：{e}"))
}

async fn push_current_branch(root: &Path) -> Result<(), String> {
    let (porcelain, remotes) = tokio::join!(read_porcelain_status(root), list_remotes(root));
    let args = plan_push_args(&porcelain?, &remotes?)
        .map_err(|e| format!("{e}（目录 {}）", root.display()))?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run_git(root, &args, GitKind::Push).await.map(|_| ())
}

async fn collect_status(root: &Path, working_directory: String) -> Result<AgentGitStatus, String> {
    let porcelain = read_porcelain_status(root).await?;
    let base = diff_base_for(&porcelain);
    let (numstat, untracked, remotes) =
        tokio::join!(read_numstat(root, base.arg), read_untracked(root), list_remotes(root));
    let (numstat, untracked, remotes) = (numstat?, untracked?, remotes?);
    let (ahead, behind) = porcelain.ahead_behind.unwrap_or((0, 0));
    Ok(AgentGitStatus {
        working_directory,
        is_repo: true,
        has_uncommitted_changes: !porcelain.entries.is_empty() || !untracked.is_empty(),
        diff: summarize_diff(base.label, &numstat, &untracked),
        branch: porcelain.branch,
        detached: porcelain.detached,
        upstream: porcelain.upstream,
        ahead,
        behind,
        has_remote: !remotes.is_empty(),
    })
}

fn ensure_working_directory(directory: &Path) -> Result<(), String> {
    match std::fs::metadata(directory) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(format!("工作目录 {} 不是文件夹", directory.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(format!(
            "工作目录 {} 不存在，请检查 Agent 助手的工作目录配置",
            directory.display()
        )),
        Err(error) => Err(format!("无法访问工作目录 {}：{error}", directory.display())),
    }
}

/// 仓库根目录；不是 Git 仓库时返回 `None`。其它失败（如 safe.directory 拒绝）原样报错。
async fn find_repo_root(directory: &Path) -> Result<Option<PathBuf>, String> {
    let args = ["rev-parse", "--show-toplevel"];
    let output = exec_git(directory, &args, GitKind::Probe, None).await?;
    if output.status.success() {
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if root.is_empty() {
            return Err(format!(
                "git rev-parse --show-toplevel 没有返回仓库根目录（目录 {}）",
                directory.display()
            ));
        }
        return Ok(Some(PathBuf::from(root)));
    }
    if is_not_a_repository_error(&String::from_utf8_lossy(&output.stderr)) {
        return Ok(None);
    }
    Err(git_failure(directory, &args, &output))
}

async fn require_repo_root(directory: &Path) -> Result<PathBuf, String> {
    ensure_working_directory(directory)?;
    find_repo_root(directory)
        .await?
        .ok_or_else(|| format!("工作目录 {} 不是 Git 仓库", directory.display()))
}

async fn read_porcelain_status(root: &Path) -> Result<PorcelainStatus, String> {
    let args = ["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=no"];
    let output = run_git(root, &args, GitKind::Read).await?;
    parse_porcelain_v2(&output)
        .map_err(|e| format!("解析 git status 输出失败（目录 {}）：{e}", root.display()))
}

async fn read_numstat(root: &Path, base: &str) -> Result<Vec<NumstatEntry>, String> {
    let args = [
        "diff",
        "--numstat",
        "-z",
        "--find-renames",
        "--no-ext-diff",
        "--no-textconv",
        base,
        "--",
    ];
    let output = run_git(root, &args, GitKind::Read).await?;
    parse_numstat_z(&output)
        .map_err(|e| format!("解析 git diff --numstat 输出失败（目录 {}）：{e}", root.display()))
}

async fn read_name_status(root: &Path, base: &str) -> Result<Vec<NameStatusEntry>, String> {
    let args = ["diff", "--name-status", "-z", "--find-renames", "--no-ext-diff", base, "--"];
    let output = run_git(root, &args, GitKind::Read).await?;
    parse_name_status_z(&output)
        .map_err(|e| format!("解析 git diff --name-status 输出失败（目录 {}）：{e}", root.display()))
}

async fn read_untracked(root: &Path) -> Result<Vec<UntrackedFile>, String> {
    let args = ["ls-files", "--others", "--exclude-standard", "-z"];
    let output = run_git(root, &args, GitKind::Read).await?;
    let paths: Vec<String> =
        output.split('\0').filter(|path| !path.is_empty()).map(str::to_string).collect();
    let measure_root = root.to_path_buf();
    tokio::task::spawn_blocking(move || measure_untracked_files(&measure_root, paths))
        .await
        .map_err(|e| format!("统计未跟踪文件行数的后台任务失败（目录 {}）：{e}", root.display()))
}

async fn list_remotes(root: &Path) -> Result<Vec<String>, String> {
    let output = run_git(root, &["remote"], GitKind::Read).await?;
    Ok(output.lines().map(str::trim).filter(|name| !name.is_empty()).map(String::from).collect())
}

async fn read_branches(root: &Path) -> Result<Vec<AgentGitBranch>, String> {
    let args = ["for-each-ref", BRANCH_REF_FORMAT, "refs/heads", "refs/remotes"];
    let output = run_git(root, &args, GitKind::Read).await?;
    let mut branches = parse_branch_refs(&output)
        .map_err(|e| format!("解析 git for-each-ref 输出失败（目录 {}）：{e}", root.display()))?;
    sort_branches(&mut branches);
    Ok(branches)
}

fn measure_untracked_files(root: &Path, paths: Vec<String>) -> Vec<UntrackedFile> {
    paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let (lines, binary) = if index < UNTRACKED_MEASURE_LIMIT {
                measure_file(&root.join(&path))
            } else {
                (0, false)
            };
            UntrackedFile { path, lines, binary }
        })
        .collect()
}

/// 统计单个未跟踪文件。非普通文件（目录形式的嵌套仓库、符号链接）、过大或读取失败的文件只计数不计行。
fn measure_file(path: &Path) -> (u64, bool) {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= UNTRACKED_MAX_BYTES => {
            std::fs::read(path).map(|bytes| count_text_lines(&bytes)).unwrap_or((0, false))
        }
        _ => (0, false),
    }
}

// ---------------------------------------------------------------------------
// git 进程
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitKind {
    /// 只读探测，强制英文输出以便识别“不是 Git 仓库”。
    Probe,
    Read,
    Write,
    Push,
}

impl GitKind {
    fn timeout(self) -> Duration {
        match self {
            GitKind::Probe | GitKind::Read => LOCAL_GIT_TIMEOUT,
            GitKind::Write => MUTATING_GIT_TIMEOUT,
            GitKind::Push => PUSH_GIT_TIMEOUT,
        }
    }

    fn read_only(self) -> bool {
        matches!(self, GitKind::Probe | GitKind::Read)
    }
}

fn describe_git(args: &[&str]) -> String {
    format!("git {}", args.join(" "))
}

/// 运行 git 并在成功时返回 stdout。
async fn run_git(directory: &Path, args: &[&str], kind: GitKind) -> Result<String, String> {
    let output = exec_git(directory, args, kind, None).await?;
    if !output.status.success() {
        return Err(git_failure(directory, args, &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 启动 git 并等待结束。只有启动失败、写入 stdin 失败或超时才返回 Err；退出码由调用方判断。
async fn exec_git(
    directory: &Path,
    args: &[&str],
    kind: GitKind,
    stdin: Option<&[u8]>,
) -> Result<Output, String> {
    let label = describe_git(args);
    let mut command = tokio::process::Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if kind.read_only() {
        command.env("GIT_OPTIONAL_LOCKS", "0");
    }
    if kind == GitKind::Probe {
        command.env("LC_ALL", "C").env_remove("LANGUAGE");
    }
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!(
                "未找到 git 可执行文件，无法执行 {label}（目录 {}）。请先安装 Git，并确保 git 在 PATH 中",
                directory.display()
            )
        } else {
            format!("启动 {label} 失败（目录 {}）：{error}", directory.display())
        }
    })?;

    let run = async {
        let write_result = match (stdin, child.stdin.take()) {
            (Some(input), Some(mut pipe)) => {
                let result = pipe.write_all(input).await;
                drop(pipe);
                result
            }
            _ => Ok(()),
        };
        let output = child.wait_with_output().await.map_err(|error| {
            format!("等待 {label} 结束失败（目录 {}）：{error}", directory.display())
        })?;
        if let Err(error) = write_result {
            // git 提前退出时写入会失败；优先返回 git 自己的错误输出。
            if output.status.success() {
                return Err(format!(
                    "向 {label} 写入标准输入失败（目录 {}）：{error}",
                    directory.display()
                ));
            }
        }
        Ok(output)
    };

    match tokio::time::timeout(kind.timeout(), run).await {
        Ok(result) => result,
        // 超时后 future 被丢弃，kill_on_drop 会结束 git 进程。
        Err(_) => Err(format!(
            "{label} 超时（目录 {}，{} 秒内没有完成），已终止",
            directory.display(),
            kind.timeout().as_secs()
        )),
    }
}

fn git_failure(directory: &Path, args: &[&str], output: &Output) -> String {
    let code = output
        .status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "无（进程被信号终止）".to_string());
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else {
        // 例如 `git commit` 的 “nothing to commit” 写在 stdout。
        let stdout = String::from_utf8_lossy(&output.stdout).replace('\0', " ").trim().to_string();
        if stdout.is_empty() { "git 没有输出任何错误信息".to_string() } else { stdout }
    };
    format!("{} 失败（目录 {}，退出码 {code}）：{detail}", describe_git(args), directory.display())
}

pub(crate) fn is_not_a_repository_error(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("not a git repository") || stderr.contains("不是 git 仓库")
}

// ---------------------------------------------------------------------------
// 纯函数：输出解析与操作规划
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PorcelainEntryKind {
    Ordinary,
    RenamedOrCopied,
    Unmerged,
    Untracked,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PorcelainEntry {
    pub kind: PorcelainEntryKind,
    pub xy: String,
    pub path: String,
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PorcelainStatus {
    /// HEAD 提交；还没有任何提交时为 `None`（同时 `unborn = true`）。
    pub oid: Option<String>,
    pub unborn: bool,
    pub branch: Option<String>,
    pub detached: bool,
    pub upstream: Option<String>,
    /// 只有上游引用存在时 git 才输出 `# branch.ab`；上游已被删除（gone）时为 `None`。
    pub ahead_behind: Option<(u32, u32)>,
    pub entries: Vec<PorcelainEntry>,
}

/// 解析 `git status --porcelain=v2 [--branch] -z`。
pub(crate) fn parse_porcelain_v2(output: &str) -> Result<PorcelainStatus, String> {
    let mut status = PorcelainStatus::default();
    let mut records = output.split('\0').filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            apply_branch_header(&mut status, header)?;
            continue;
        }
        let entry = match record.as_bytes()[0] {
            b'1' => {
                let fields: Vec<&str> = record.splitn(9, ' ').collect();
                if fields.len() != 9 {
                    return Err(format!("普通改动记录字段不足：{record:?}"));
                }
                PorcelainEntry {
                    kind: PorcelainEntryKind::Ordinary,
                    xy: fields[1].to_string(),
                    path: fields[8].to_string(),
                    original_path: None,
                }
            }
            b'2' => {
                let fields: Vec<&str> = record.splitn(10, ' ').collect();
                if fields.len() != 10 {
                    return Err(format!("重命名/复制记录字段不足：{record:?}"));
                }
                let original = records
                    .next()
                    .ok_or_else(|| format!("重命名/复制记录缺少原路径：{record:?}"))?;
                PorcelainEntry {
                    kind: PorcelainEntryKind::RenamedOrCopied,
                    xy: fields[1].to_string(),
                    path: fields[9].to_string(),
                    original_path: Some(original.to_string()),
                }
            }
            b'u' => {
                let fields: Vec<&str> = record.splitn(11, ' ').collect();
                if fields.len() != 11 {
                    return Err(format!("冲突记录字段不足：{record:?}"));
                }
                PorcelainEntry {
                    kind: PorcelainEntryKind::Unmerged,
                    xy: fields[1].to_string(),
                    path: fields[10].to_string(),
                    original_path: None,
                }
            }
            kind @ (b'?' | b'!') => {
                let path = record
                    .get(2..)
                    .filter(|path| !path.is_empty() && record.as_bytes().get(1) == Some(&b' '))
                    .ok_or_else(|| format!("未跟踪/忽略记录缺少路径：{record:?}"))?;
                PorcelainEntry {
                    kind: if kind == b'?' {
                        PorcelainEntryKind::Untracked
                    } else {
                        PorcelainEntryKind::Ignored
                    },
                    xy: (kind as char).to_string().repeat(2),
                    path: path.to_string(),
                    original_path: None,
                }
            }
            // 新版本 git 可能增加记录类型；不认识的直接跳过。
            _ => continue,
        };
        status.entries.push(entry);
    }
    Ok(status)
}

fn apply_branch_header(status: &mut PorcelainStatus, header: &str) -> Result<(), String> {
    if let Some(oid) = header.strip_prefix("branch.oid ") {
        status.unborn = oid == "(initial)";
        status.oid = (!status.unborn).then(|| oid.to_string());
    } else if let Some(head) = header.strip_prefix("branch.head ") {
        status.detached = head == "(detached)";
        status.branch = (!status.detached).then(|| head.to_string());
    } else if let Some(upstream) = header.strip_prefix("branch.upstream ") {
        status.upstream = Some(upstream.to_string());
    } else if let Some(ab) = header.strip_prefix("branch.ab ") {
        status.ahead_behind = Some(parse_ahead_behind(ab)?);
    }
    Ok(())
}

/// `+A -B`；`?`（未计算）按 0 处理。
fn parse_ahead_behind(value: &str) -> Result<(u32, u32), String> {
    let mut parts = value.split_whitespace();
    let mut count = |prefix: char| -> Result<u32, String> {
        let part = parts
            .next()
            .and_then(|part| part.strip_prefix(prefix))
            .ok_or_else(|| format!("无法解析 branch.ab：{value:?}"))?;
        if part == "?" {
            return Ok(0);
        }
        part.parse::<u32>().map_err(|e| format!("无法解析 branch.ab {value:?}：{e}"))
    };
    let ahead = count('+')?;
    let behind = count('-')?;
    Ok((ahead, behind))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NumstatEntry {
    pub path: String,
    pub original_path: Option<String>,
    /// 二进制文件 git 输出 `-`，此时为 `None`。
    pub insertions: Option<u64>,
    pub deletions: Option<u64>,
}

impl NumstatEntry {
    fn binary(&self) -> bool {
        self.insertions.is_none() || self.deletions.is_none()
    }
}

/// 解析 `git diff --numstat -z`：`增\t删\t路径\0`，重命名为 `增\t删\t\0原路径\0新路径\0`。
pub(crate) fn parse_numstat_z(output: &str) -> Result<Vec<NumstatEntry>, String> {
    let mut tokens = output.split('\0');
    let mut entries = Vec::new();
    while let Some(token) = tokens.next() {
        if token.is_empty() {
            continue;
        }
        let mut fields = token.splitn(3, '\t');
        let (Some(insertions), Some(deletions), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return Err(format!("numstat 记录格式不正确：{token:?}"));
        };
        let insertions = parse_numstat_count(insertions, token)?;
        let deletions = parse_numstat_count(deletions, token)?;
        let (path, original_path) = if path.is_empty() {
            let original = tokens
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("numstat 重命名记录缺少原路径：{token:?}"))?;
            let renamed = tokens
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("numstat 重命名记录缺少新路径：{token:?}"))?;
            (renamed.to_string(), Some(original.to_string()))
        } else {
            (path.to_string(), None)
        };
        entries.push(NumstatEntry { path, original_path, insertions, deletions });
    }
    Ok(entries)
}

fn parse_numstat_count(value: &str, record: &str) -> Result<Option<u64>, String> {
    if value == "-" {
        return Ok(None);
    }
    value.parse::<u64>().map(Some).map_err(|e| format!("numstat 行数 {value:?} 无效（{record:?}）：{e}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameStatusEntry {
    /// 状态字母：A/M/D/R/C/T/U/X。
    pub status: char,
    pub path: String,
    pub original_path: Option<String>,
}

/// 解析 `git diff --name-status -z`：`M\0路径\0`，重命名/复制为 `R100\0原路径\0新路径\0`。
pub(crate) fn parse_name_status_z(output: &str) -> Result<Vec<NameStatusEntry>, String> {
    let mut tokens = output.split('\0').filter(|token| !token.is_empty());
    let mut entries = Vec::new();
    while let Some(code) = tokens.next() {
        let status = code.chars().next().unwrap_or('M');
        let first = tokens.next().ok_or_else(|| format!("name-status 记录 {code:?} 缺少路径"))?;
        let (path, original_path) = if matches!(status, 'R' | 'C') {
            let second =
                tokens.next().ok_or_else(|| format!("name-status 记录 {code:?} 缺少新路径"))?;
            (second.to_string(), Some(first.to_string()))
        } else {
            (first.to_string(), None)
        };
        entries.push(NameStatusEntry { status, path, original_path });
    }
    Ok(entries)
}

pub(crate) fn change_status_label(code: char) -> &'static str {
    match code {
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        'C' => "copied",
        'T' => "type_changed",
        'U' => "conflicted",
        _ => "modified",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UntrackedFile {
    pub path: String,
    pub lines: u64,
    pub binary: bool,
}

/// 按 git 的方式数行：换行数，末尾没有换行的最后一行也算一行；前 8000 字节含 NUL 视为二进制。
pub(crate) fn count_text_lines(bytes: &[u8]) -> (u64, bool) {
    if bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
        return (0, true);
    }
    let newlines = bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
    let unterminated = u64::from(bytes.last().is_some_and(|byte| *byte != b'\n'));
    (newlines + unterminated, false)
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(crate) struct DiffBase {
    /// 传给 `git diff` 的基准。
    pub arg: &'static str,
    /// 展示给前端的基准名。
    pub label: Option<String>,
}

/// 有可用上游 → 与上游比较（含未推送的提交）；否则与 HEAD 比较；还没有提交 → 与空树比较。
pub(crate) fn diff_base_for(status: &PorcelainStatus) -> DiffBase {
    if status.unborn {
        return DiffBase { arg: EMPTY_TREE_SHA, label: None };
    }
    match (&status.upstream, status.ahead_behind) {
        (Some(upstream), Some(_)) => DiffBase { arg: "@{upstream}", label: Some(upstream.clone()) },
        _ => DiffBase { arg: "HEAD", label: Some("HEAD".to_string()) },
    }
}

pub(crate) fn summarize_diff(
    base: Option<String>,
    numstat: &[NumstatEntry],
    untracked: &[UntrackedFile],
) -> AgentGitDiffStat {
    AgentGitDiffStat {
        base,
        files_changed: saturating_u32(numstat.len() + untracked.len()),
        insertions: numstat.iter().filter_map(|entry| entry.insertions).sum::<u64>()
            + untracked.iter().map(|file| file.lines).sum::<u64>(),
        deletions: numstat.iter().filter_map(|entry| entry.deletions).sum(),
        untracked_files: saturating_u32(untracked.len()),
    }
}

/// 合并 name-status（状态）、numstat（行数）与未跟踪文件，按路径排序。
pub(crate) fn merge_changed_files(
    name_status: &[NameStatusEntry],
    numstat: &[NumstatEntry],
    untracked: &[UntrackedFile],
) -> Vec<AgentGitChangedFile> {
    let counts: HashMap<&str, &NumstatEntry> =
        numstat.iter().map(|entry| (entry.path.as_str(), entry)).collect();
    let mut files: Vec<AgentGitChangedFile> = Vec::new();
    let mut index_by_path: HashMap<String, usize> = HashMap::new();
    for entry in name_status {
        if let Some(&existing) = index_by_path.get(&entry.path) {
            // 冲突文件可能同时出现 U 与其它记录，冲突优先。
            if entry.status == 'U' {
                files[existing].status = change_status_label('U').to_string();
            }
            continue;
        }
        let count = counts.get(entry.path.as_str());
        index_by_path.insert(entry.path.clone(), files.len());
        files.push(AgentGitChangedFile {
            path: entry.path.clone(),
            original_path: entry.original_path.clone(),
            status: change_status_label(entry.status).to_string(),
            insertions: count.and_then(|count| count.insertions).unwrap_or(0),
            deletions: count.and_then(|count| count.deletions).unwrap_or(0),
            binary: count.is_some_and(|count| count.binary()),
        });
    }
    for file in untracked {
        if index_by_path.contains_key(&file.path) {
            continue;
        }
        files.push(AgentGitChangedFile {
            path: file.path.clone(),
            original_path: None,
            status: "untracked".to_string(),
            insertions: file.lines,
            deletions: 0,
            binary: file.binary,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// 解析 `for-each-ref --format=%(refname)%00%(upstream)%00%(committerdate:unix)%00%(HEAD)%00%(symref)`。
/// 跳过远程的符号引用（`refs/remotes/<remote>/HEAD`）。
pub(crate) fn parse_branch_refs(output: &str) -> Result<Vec<AgentGitBranch>, String> {
    let mut branches = Vec::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split('\0').collect();
        if fields.len() != 5 {
            return Err(format!("for-each-ref 输出行字段数为 {}，应为 5：{line:?}", fields.len()));
        }
        let full_ref = fields[0];
        let (name, is_remote) = if let Some(name) = full_ref.strip_prefix("refs/heads/") {
            (name, false)
        } else if let Some(name) = full_ref.strip_prefix("refs/remotes/") {
            (name, true)
        } else {
            continue;
        };
        if name.is_empty() || (is_remote && (!fields[4].is_empty() || name.ends_with("/HEAD"))) {
            continue;
        }
        branches.push(AgentGitBranch {
            name: name.to_string(),
            full_ref: full_ref.to_string(),
            is_remote,
            is_current: !is_remote && fields[3].trim() == "*",
            upstream: short_ref_name(fields[1]),
            committed_at: fields[2].trim().parse::<i64>().ok(),
        });
    }
    Ok(branches)
}

fn short_ref_name(full_ref: &str) -> Option<String> {
    let full_ref = full_ref.trim();
    if full_ref.is_empty() {
        return None;
    }
    let short = full_ref
        .strip_prefix("refs/heads/")
        .or_else(|| full_ref.strip_prefix("refs/remotes/"))
        .unwrap_or(full_ref);
    Some(short.to_string())
}

/// 当前分支在最前，其次本地分支、远程分支，各自按最近提交时间倒序。
pub(crate) fn sort_branches(branches: &mut [AgentGitBranch]) {
    branches.sort_by(|a, b| {
        b.is_current
            .cmp(&a.is_current)
            .then(a.is_remote.cmp(&b.is_remote))
            .then(b.committed_at.cmp(&a.committed_at))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// 把 `<remote>/<branch>` 拆成远程名与分支名；远程名本身可能带 `/`，取最长匹配。
pub(crate) fn split_remote_branch(name: &str, remotes: &[String]) -> Option<(String, String)> {
    remotes
        .iter()
        .filter_map(|remote| {
            name.strip_prefix(remote.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
                .filter(|rest| !rest.is_empty())
                .map(|rest| (remote, rest))
        })
        .max_by_key(|(remote, _)| remote.len())
        .map(|(remote, rest)| (remote.clone(), rest.to_string()))
}

/// 用户传入的分支名只做最基本的形状检查；真正的校验是必须出现在 for-each-ref 列表里。
pub(crate) fn validate_branch_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("分支名不能为空".to_string());
    }
    if name.starts_with('-') {
        return Err(format!("分支名 {name:?} 不合法：不能以 - 开头"));
    }
    if name.chars().any(char::is_control) {
        return Err(format!("分支名 {name:?} 不合法：包含控制字符"));
    }
    Ok(())
}

/// 计算切换分支要执行的 git 参数。分支必须存在于列表中（防止参数注入）。
pub(crate) fn plan_switch_args(
    branches: &[AgentGitBranch],
    remotes: &[String],
    branch: &str,
    is_remote: bool,
) -> Result<Vec<String>, String> {
    validate_branch_name(branch)?;
    let target = branches
        .iter()
        .find(|candidate| candidate.is_remote == is_remote && candidate.name == branch)
        .ok_or_else(|| {
            format!("{}分支 {branch} 不存在", if is_remote { "远程" } else { "本地" })
        })?;
    if !is_remote {
        return Ok(vec!["switch".to_string(), target.name.clone()]);
    }
    let (_, local_name) = split_remote_branch(&target.name, remotes).ok_or_else(|| {
        format!(
            "无法从远程分支 {} 识别远程仓库（已配置的远程：{}）",
            target.name,
            if remotes.is_empty() { "无".to_string() } else { remotes.join("、") }
        )
    })?;
    validate_branch_name(&local_name)?;
    if branches.iter().any(|candidate| !candidate.is_remote && candidate.name == local_name) {
        Ok(vec!["switch".to_string(), local_name])
    } else {
        Ok(vec![
            "switch".to_string(),
            "-c".to_string(),
            local_name,
            "--track".to_string(),
            target.full_ref.clone(),
        ])
    }
}

/// 有上游 → `git push`；否则推到 origin / 唯一远程并设置上游。
pub(crate) fn plan_push_args(
    status: &PorcelainStatus,
    remotes: &[String],
) -> Result<Vec<String>, String> {
    if status.detached {
        return Err("当前处于分离 HEAD（detached HEAD）状态，没有可推送的分支，请先切换到分支".to_string());
    }
    if status.unborn {
        return Err(format!(
            "当前分支 {} 还没有任何提交，无法推送",
            status.branch.as_deref().unwrap_or("（未知）")
        ));
    }
    if status.upstream.is_some() {
        return Ok(vec!["push".to_string()]);
    }
    let remote = choose_push_remote(remotes)?;
    Ok(vec!["push".to_string(), "-u".to_string(), remote, "HEAD".to_string()])
}

pub(crate) fn choose_push_remote(remotes: &[String]) -> Result<String, String> {
    let remote = if remotes.iter().any(|remote| remote == "origin") {
        "origin".to_string()
    } else {
        match remotes {
            [] => return Err("没有可推送的远程仓库，请先添加远程仓库（git remote add）".to_string()),
            [only] => only.clone(),
            many => {
                return Err(format!(
                    "存在多个远程仓库（{}）且没有 origin，无法确定推送目标",
                    many.join("、")
                ))
            }
        }
    };
    if remote.starts_with('-') {
        return Err(format!("远程仓库名 {remote:?} 不合法：不能以 - 开头"));
    }
    Ok(remote)
}

pub(crate) fn validate_commit_message(message: &str) -> Result<(), String> {
    if message.trim().is_empty() {
        return Err("提交说明不能为空".to_string());
    }
    if message.contains('\0') {
        return Err("提交说明不能包含 NUL 字符".to_string());
    }
    Ok(())
}

/// 解析 `git log -1 --format=%h%x00%B`：短提交号与提交说明首行（git 实际保存的说明）。
pub(crate) fn parse_commit_log(output: &str) -> (String, String) {
    let (hash, body) = output.split_once('\0').unwrap_or((output, ""));
    let summary = body.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    (hash.trim().to_string(), summary.to_string())
}

#[cfg(test)]
#[path = "agent_workspace_api_tests.rs"]
mod tests;
