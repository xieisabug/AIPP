use super::*;

fn branch(name: &str, is_remote: bool, is_current: bool, committed_at: Option<i64>) -> AgentGitBranch {
    AgentGitBranch {
        name: name.to_string(),
        full_ref: if is_remote { format!("refs/remotes/{name}") } else { format!("refs/heads/{name}") },
        is_remote,
        is_current,
        upstream: None,
        committed_at,
    }
}

// ---------------------------------------------------------------------------
// 纯函数
// ---------------------------------------------------------------------------

#[test]
fn test_parse_porcelain_v2_reads_branch_header_and_entries() {
    let output = [
        "# branch.oid 0123456789abcdef",
        "# branch.head feature/x",
        "# branch.upstream origin/feature/x",
        "# branch.ab +2 -1",
        "1 .M N... 100644 100644 100644 aaa bbb src/main.rs",
        "2 R. N... 100644 100644 100644 aaa bbb R100 new name.rs",
        "old name.rs",
        "u UU N... 100644 100644 100644 100644 aaa bbb ccc conflict.rs",
        "? notes.txt",
        "",
    ]
    .join("\0");
    let status = parse_porcelain_v2(&output).unwrap();
    assert_eq!(status.branch.as_deref(), Some("feature/x"));
    assert_eq!(status.upstream.as_deref(), Some("origin/feature/x"));
    assert_eq!(status.ahead_behind, Some((2, 1)));
    assert!(!status.detached);
    assert!(!status.unborn);
    assert_eq!(status.entries.len(), 4);
    assert_eq!(status.entries[0].path, "src/main.rs");
    assert_eq!(status.entries[1].kind, PorcelainEntryKind::RenamedOrCopied);
    assert_eq!(status.entries[1].path, "new name.rs");
    assert_eq!(status.entries[1].original_path.as_deref(), Some("old name.rs"));
    assert_eq!(status.entries[2].kind, PorcelainEntryKind::Unmerged);
    assert_eq!(status.entries[2].path, "conflict.rs");
    assert_eq!(status.entries[3].kind, PorcelainEntryKind::Untracked);
    assert_eq!(status.entries[3].path, "notes.txt");
}

#[test]
fn test_parse_porcelain_v2_detached_and_unborn() {
    let detached = parse_porcelain_v2("# branch.oid abc\0# branch.head (detached)\0").unwrap();
    assert!(detached.detached);
    assert_eq!(detached.branch, None);

    let unborn = parse_porcelain_v2("# branch.oid (initial)\0# branch.head main\0").unwrap();
    assert!(unborn.unborn);
    assert_eq!(unborn.oid, None);
    assert_eq!(unborn.branch.as_deref(), Some("main"));
}

#[test]
fn test_parse_porcelain_v2_rejects_truncated_records() {
    assert!(parse_porcelain_v2("1 .M N... 100644").is_err());
    assert!(parse_porcelain_v2("2 R. N... 100644 100644 100644 aaa bbb R100 new.rs").is_err());
    assert!(parse_porcelain_v2("# branch.ab nonsense").is_err());
}

#[test]
fn test_parse_numstat_z_counts_binary_and_renames() {
    let output = "3\t1\tsrc/lib.rs\0-\t-\tlogo.png\05\t0\t\0old.rs\0new.rs\0";
    let entries = parse_numstat_z(output).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!((entries[0].insertions, entries[0].deletions), (Some(3), Some(1)));
    assert!(entries[1].binary());
    assert_eq!(entries[2].path, "new.rs");
    assert_eq!(entries[2].original_path.as_deref(), Some("old.rs"));
    assert!(parse_numstat_z("x\t1\tbad.rs\0").is_err());
}

#[test]
fn test_parse_name_status_z_handles_renames() {
    let entries = parse_name_status_z("M\0a.rs\0R087\0old.rs\0new.rs\0D\0gone.rs\0").unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[1].status, 'R');
    assert_eq!(entries[1].path, "new.rs");
    assert_eq!(entries[1].original_path.as_deref(), Some("old.rs"));
    assert_eq!(change_status_label(entries[2].status), "deleted");
}

#[test]
fn test_count_text_lines_matches_git_rules() {
    assert_eq!(count_text_lines(b""), (0, false));
    assert_eq!(count_text_lines(b"a\nb\n"), (2, false));
    assert_eq!(count_text_lines(b"a\nb"), (2, false));
    assert_eq!(count_text_lines(b"a\0b\n"), (0, true));
}

#[test]
fn test_diff_base_prefers_live_upstream() {
    let mut status = PorcelainStatus { upstream: Some("origin/main".into()), ahead_behind: Some((1, 0)), ..Default::default() };
    let base = diff_base_for(&status);
    assert_eq!((base.arg, base.label.as_deref()), ("@{upstream}", Some("origin/main")));

    // 上游已删除（git 不输出 branch.ab）→ 与 HEAD 比较
    status.ahead_behind = None;
    let base = diff_base_for(&status);
    assert_eq!((base.arg, base.label.as_deref()), ("HEAD", Some("HEAD")));

    let unborn = PorcelainStatus { unborn: true, ..Default::default() };
    let base = diff_base_for(&unborn);
    assert_eq!((base.arg, base.label), (EMPTY_TREE_SHA, None));
}

#[test]
fn test_summarize_diff_includes_untracked_lines() {
    let numstat = vec![
        NumstatEntry { path: "a.rs".into(), original_path: None, insertions: Some(10), deletions: Some(2) },
        NumstatEntry { path: "img.png".into(), original_path: None, insertions: None, deletions: None },
    ];
    let untracked = vec![UntrackedFile { path: "new.rs".into(), lines: 5, binary: false }];
    let diff = summarize_diff(Some("HEAD".into()), &numstat, &untracked);
    assert_eq!(diff.files_changed, 3);
    assert_eq!(diff.insertions, 15);
    assert_eq!(diff.deletions, 2);
    assert_eq!(diff.untracked_files, 1);
}

#[test]
fn test_merge_changed_files_combines_sources_and_sorts() {
    let name_status = vec![
        NameStatusEntry { status: 'M', path: "b.rs".into(), original_path: None },
        NameStatusEntry { status: 'A', path: "a.rs".into(), original_path: None },
    ];
    let numstat = vec![NumstatEntry { path: "b.rs".into(), original_path: None, insertions: Some(4), deletions: Some(1) }];
    let untracked = vec![UntrackedFile { path: "c.txt".into(), lines: 2, binary: false }];
    let files = merge_changed_files(&name_status, &numstat, &untracked);
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["a.rs", "b.rs", "c.txt"]);
    assert_eq!(files[1].status, "modified");
    assert_eq!((files[1].insertions, files[1].deletions), (4, 1));
    assert_eq!(files[2].status, "untracked");
    assert_eq!(files[2].insertions, 2);
}

#[test]
fn test_parse_branch_refs_skips_remote_head_and_marks_current() {
    let output = [
        "refs/heads/main\0refs/remotes/origin/main\01700000000\0*\0",
        "refs/heads/dev\0\01700000100\0 \0",
        "refs/remotes/origin/HEAD\0\01700000000\0 \0refs/remotes/origin/main",
        "refs/remotes/origin/main\0\01700000000\0 \0",
    ]
    .join("\n");
    let branches = parse_branch_refs(&output).unwrap();
    assert_eq!(branches.len(), 3);
    assert!(branches[0].is_current);
    assert_eq!(branches[0].upstream.as_deref(), Some("origin/main"));
    assert_eq!(branches[1].committed_at, Some(1700000100));
    assert!(branches[2].is_remote);
    assert_eq!(branches[2].name, "origin/main");
    assert!(parse_branch_refs("refs/heads/main\0only-two").is_err());
}

#[test]
fn test_sort_branches_current_then_local_then_remote_by_recency() {
    let mut branches = vec![
        branch("origin/new", true, false, Some(300)),
        branch("old", false, false, Some(100)),
        branch("main", false, true, Some(50)),
        branch("recent", false, false, Some(200)),
    ];
    sort_branches(&mut branches);
    let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["main", "recent", "old", "origin/new"]);
}

#[test]
fn test_split_remote_branch_prefers_longest_remote() {
    let remotes = vec!["origin".to_string(), "origin/mirror".to_string()];
    assert_eq!(split_remote_branch("origin/feature/x", &remotes), Some(("origin".into(), "feature/x".into())));
    assert_eq!(split_remote_branch("origin/mirror/dev", &remotes), Some(("origin/mirror".into(), "dev".into())));
    assert_eq!(split_remote_branch("upstream/dev", &remotes), None);
}

#[test]
fn test_plan_switch_args_validates_and_tracks_remote() {
    let branches = vec![
        branch("main", false, true, None),
        branch("origin/main", true, false, None),
        branch("origin/feature", true, false, None),
    ];
    let remotes = vec!["origin".to_string()];
    assert_eq!(plan_switch_args(&branches, &remotes, "main", false).unwrap(), ["switch", "main"]);
    // 远程分支已有同名本地分支 → 直接切换本地分支
    assert_eq!(plan_switch_args(&branches, &remotes, "origin/main", true).unwrap(), ["switch", "main"]);
    assert_eq!(
        plan_switch_args(&branches, &remotes, "origin/feature", true).unwrap(),
        ["switch", "-c", "feature", "--track", "refs/remotes/origin/feature"]
    );
    assert!(plan_switch_args(&branches, &remotes, "--orphan", false).is_err());
    assert!(plan_switch_args(&branches, &remotes, "missing", false).is_err());
    assert!(plan_switch_args(&branches, &remotes, "main", true).is_err());
}

#[test]
fn test_plan_push_args_by_upstream_and_remotes() {
    let tracked = PorcelainStatus { branch: Some("main".into()), upstream: Some("origin/main".into()), ..Default::default() };
    assert_eq!(plan_push_args(&tracked, &["origin".into()]).unwrap(), ["push"]);

    let untracked = PorcelainStatus { branch: Some("dev".into()), ..Default::default() };
    assert_eq!(plan_push_args(&untracked, &["origin".into(), "fork".into()]).unwrap(), ["push", "-u", "origin", "HEAD"]);
    assert_eq!(plan_push_args(&untracked, &["fork".into()]).unwrap(), ["push", "-u", "fork", "HEAD"]);
    assert!(plan_push_args(&untracked, &[]).is_err());
    assert!(plan_push_args(&untracked, &["a".into(), "b".into()]).is_err());

    let detached = PorcelainStatus { detached: true, ..Default::default() };
    assert!(plan_push_args(&detached, &["origin".into()]).is_err());
    let unborn = PorcelainStatus { unborn: true, branch: Some("main".into()), ..Default::default() };
    assert!(plan_push_args(&unborn, &["origin".into()]).is_err());
}

#[test]
fn test_validate_commit_message_and_parse_log() {
    assert!(validate_commit_message("   ").is_err());
    assert!(validate_commit_message("a\0b").is_err());
    assert!(validate_commit_message("feat: x").is_ok());
    assert_eq!(parse_commit_log("abc1234\0feat: add panel\n\n- rows\n"), ("abc1234".into(), "feat: add panel".into()));
}

#[test]
fn test_is_not_a_repository_error_detects_english_and_chinese() {
    assert!(is_not_a_repository_error("fatal: not a git repository (or any of the parent directories): .git"));
    assert!(is_not_a_repository_error("致命错误：不是 git 仓库（或者任何父目录）：.git"));
    assert!(!is_not_a_repository_error("fatal: detected dubious ownership"));
}

// ---------------------------------------------------------------------------
// 临时仓库上的集成测试（没有安装 git 时跳过）
// ---------------------------------------------------------------------------

fn git_available() -> bool {
    std::process::Command::new("git").arg("--version").output().map(|output| output.status.success()).unwrap_or(false)
}

fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().expect("spawn git");
    assert!(output.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&output.stderr));
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "AIPP Test"]);
    git(dir.path(), &["config", "user.email", "aipp@example.com"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(dir.path(), &["config", "core.autocrlf", "false"]);
    dir
}

fn write(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write file");
}

#[tokio::test]
async fn test_git_status_for_dir_non_repo_is_not_an_error() {
    if !git_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let status = git_status_for_dir(dir.path()).await.unwrap();
    assert!(!status.is_repo);
    assert_eq!(status.branch, None);
}

#[tokio::test]
async fn test_git_status_for_dir_missing_directory_errors() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist");
    let error = git_status_for_dir(&missing).await.unwrap_err();
    assert!(error.contains("不存在"), "{error}");
}

#[tokio::test]
async fn test_git_status_for_dir_unborn_repo_counts_untracked_lines() {
    if !git_available() {
        return;
    }
    let repo = init_repo();
    write(repo.path(), "a.txt", "one\ntwo\n");
    let status = git_status_for_dir(repo.path()).await.unwrap();
    assert!(status.is_repo);
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.diff.base, None);
    assert_eq!(status.diff.untracked_files, 1);
    assert_eq!(status.diff.insertions, 2);
    assert!(status.has_uncommitted_changes);
}

#[tokio::test]
async fn test_git_status_commit_and_changes_round_trip() {
    if !git_available() {
        return;
    }
    let repo = init_repo();
    write(repo.path(), "a.txt", "one\ntwo\n");
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "init"]);

    write(repo.path(), "a.txt", "one\nTWO\nthree\n");
    write(repo.path(), "b.txt", "new\n");
    let status = git_status_for_dir(repo.path()).await.unwrap();
    assert_eq!(status.diff.base.as_deref(), Some("HEAD"));
    assert_eq!(status.diff.files_changed, 2);
    assert_eq!(status.diff.insertions, 3); // a.txt +2, b.txt +1
    assert_eq!(status.diff.deletions, 1);
    assert!(!status.has_remote);

    let changes = list_changes_for_dir(repo.path()).await.unwrap();
    let summary: Vec<(&str, &str)> = changes.iter().map(|file| (file.path.as_str(), file.status.as_str())).collect();
    assert_eq!(summary, [("a.txt", "modified"), ("b.txt", "untracked")]);

    let result = commit_for_dir(repo.path(), "feat: update files\n\n- details", false).await.unwrap();
    assert_eq!(result.summary, "feat: update files");
    assert!(!result.pushed);
    assert!(!result.status.has_uncommitted_changes);
    assert_eq!(result.status.diff.files_changed, 0);

    let error = commit_for_dir(repo.path(), "again", false).await.unwrap_err();
    assert!(error.contains("没有需要提交的改动"), "{error}");
}

#[tokio::test]
async fn test_switch_branch_for_dir_switches_local_branch() {
    if !git_available() {
        return;
    }
    let repo = init_repo();
    write(repo.path(), "a.txt", "one\n");
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "init"]);
    git(repo.path(), &["branch", "dev"]);

    let branches = list_branches_for_dir(repo.path()).await.unwrap();
    assert_eq!(branches[0].name, "main");
    assert!(branches[0].is_current);
    assert!(branches.iter().any(|b| b.name == "dev" && !b.is_remote));

    let status = switch_branch_for_dir(repo.path(), "dev", false).await.unwrap();
    assert_eq!(status.branch.as_deref(), Some("dev"));
    assert!(switch_branch_for_dir(repo.path(), "missing", false).await.is_err());
}

#[tokio::test]
async fn test_push_for_dir_without_remote_reports_reason() {
    if !git_available() {
        return;
    }
    let repo = init_repo();
    write(repo.path(), "a.txt", "one\n");
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "init"]);
    let error = push_for_dir(repo.path()).await.unwrap_err();
    assert!(error.contains("没有可推送的远程仓库"), "{error}");
}

#[tokio::test]
async fn test_commit_and_push_to_local_bare_remote() {
    if !git_available() {
        return;
    }
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    let repo = init_repo();
    git(repo.path(), &["remote", "add", "origin", remote.path().to_str().unwrap()]);
    write(repo.path(), "a.txt", "one\n");

    let result = commit_for_dir(repo.path(), "init", true).await.unwrap();
    assert!(result.pushed, "push error: {:?}", result.push_error);
    assert_eq!(result.status.upstream.as_deref(), Some("origin/main"));
    assert_eq!((result.status.ahead, result.status.behind), (0, 0));
    assert_eq!(result.status.diff.base.as_deref(), Some("origin/main"));
}
