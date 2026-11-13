/** Agent 会话工作目录的 Git 状态（后端 `src-tauri/src/api/agent_workspace_api.rs`）。 */
export interface AgentGitDiffStat {
    /** 对比基准：上游分支名（如 origin/main）或 HEAD；还没有任何提交时为 null */
    base: string | null;
    files_changed: number;
    insertions: number;
    deletions: number;
    untracked_files: number;
}

export interface AgentGitStatus {
    working_directory: string;
    is_repo: boolean;
    branch: string | null;
    detached: boolean;
    upstream: string | null;
    ahead: number;
    behind: number;
    has_remote: boolean;
    has_uncommitted_changes: boolean;
    diff: AgentGitDiffStat;
}

export interface AgentGitBranch {
    /** 短名，如 main、origin/main */
    name: string;
    full_ref: string;
    is_remote: boolean;
    is_current: boolean;
    upstream: string | null;
    committed_at: number | null;
}

export interface AgentGitChangedFile {
    path: string;
    original_path: string | null;
    /** modified | added | deleted | renamed | copied | untracked | conflicted | type_changed */
    status: string;
    insertions: number;
    deletions: number;
    binary: boolean;
}

export interface AgentGitCommitResult {
    commit: string;
    summary: string;
    pushed: boolean;
    push_error: string | null;
    status: AgentGitStatus;
}
