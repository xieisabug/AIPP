import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import {
    Check,
    ChevronDown,
    FileDiff,
    GitBranch,
    GitCommitHorizontal,
    Loader2,
    RefreshCw,
    Shrink,
    Sparkles,
    Upload,
} from "lucide-react";

import { cn } from "@/utils/utils";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import type {
    AgentGitBranch,
    AgentGitChangedFile,
    AgentGitCommitResult,
    AgentGitStatus,
} from "@/data/AgentWorkspace";
import { filterBranches, formatLineCount } from "@/utils/agentWorkspace";

export interface AgentWorkspacePanelProps {
    conversationId: string;
    /** 当前 Agent 是否支持手动压缩上下文 */
    compactSupported: boolean;
    /** Agent 正在回复：切换分支、提交、压缩都暂不可用 */
    busy: boolean;
    /** 发送压缩指令（开始一轮压缩）；失败时抛出带原因的错误 */
    onCompact: () => Promise<void>;
    /** 让当前 Agent 起草提交信息，返回提交信息文本；失败时抛出带原因的错误 */
    onGenerateCommitMessage: () => Promise<string>;
    mobile?: boolean;
}

const STATUS_LETTER: Record<string, string> = {
    modified: "M",
    added: "A",
    deleted: "D",
    renamed: "R",
    copied: "C",
    untracked: "U",
    conflicted: "!",
    type_changed: "T",
};

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

function splitPath(path: string): { name: string; dir: string } {
    const index = path.lastIndexOf("/");
    return index < 0 ? { name: path, dir: "" } : { name: path.slice(index + 1), dir: path.slice(0, index) };
}

/** +增 −删；为 0 的一侧省略（showZero 时两侧都显示，用于汇总行）。 */
const LineCounts: React.FC<{ insertions: number; deletions: number; showZero?: boolean }> = ({ insertions, deletions, showZero = false }) => (
    <span className="shrink-0 text-xs tabular-nums">
        {showZero || insertions > 0 ? (
            <span className="text-emerald-600 dark:text-emerald-400">+{formatLineCount(insertions)}</span>
        ) : null}
        {(showZero || insertions > 0) && (showZero || deletions > 0) ? " " : null}
        {showZero || deletions > 0 ? (
            <span className="text-red-600 dark:text-red-400">−{formatLineCount(deletions)}</span>
        ) : null}
    </span>
);

/**
 * 详情侧栏顶部的 Agent 工作区面板：当前分支（下拉筛选/切换）、提交（可让 Agent 起草提交信息）、
 * 改动统计、手动压缩上下文。Git 相关行只在工作目录是 Git 仓库时显示。
 */
const AgentWorkspacePanel: React.FC<AgentWorkspacePanelProps> = ({
    conversationId,
    compactSupported,
    busy,
    onCompact,
    onGenerateCommitMessage,
    mobile = false,
}) => {
    const conversationIdNum = Number(conversationId);
    const [status, setStatus] = useState<AgentGitStatus | null>(null);
    const [statusError, setStatusError] = useState<string | null>(null);
    const [refreshing, setRefreshing] = useState(false);
    const requestIdRef = useRef(0);

    const refreshStatus = useCallback(async () => {
        if (!conversationIdNum) return;
        const requestId = ++requestIdRef.current;
        setRefreshing(true);
        try {
            const next = await invoke<AgentGitStatus>("get_agent_git_status", { conversationId: conversationIdNum });
            if (requestId !== requestIdRef.current) return;
            setStatus(next);
            setStatusError(null);
        } catch (error) {
            if (requestId !== requestIdRef.current) return;
            setStatus(null);
            setStatusError(errorText(error));
        } finally {
            if (requestId === requestIdRef.current) setRefreshing(false);
        }
    }, [conversationIdNum]);

    // 切换会话：清空旧状态再拉取
    useEffect(() => {
        setStatus(null);
        setStatusError(null);
        void refreshStatus();
    }, [refreshStatus]);

    // Agent 一轮结束（可能改了文件、切了分支）后刷新；窗口回到前台也刷新
    const wasBusyRef = useRef(busy);
    useEffect(() => {
        if (wasBusyRef.current && !busy) void refreshStatus();
        wasBusyRef.current = busy;
    }, [busy, refreshStatus]);
    useEffect(() => {
        const onFocus = () => void refreshStatus();
        window.addEventListener("focus", onFocus);
        return () => window.removeEventListener("focus", onFocus);
    }, [refreshStatus]);

    // ---------------- 分支 ----------------
    const [branchOpen, setBranchOpen] = useState(false);
    const [branches, setBranches] = useState<AgentGitBranch[] | null>(null);
    const [branchError, setBranchError] = useState<string | null>(null);
    const [branchQuery, setBranchQuery] = useState("");
    const [switchingBranch, setSwitchingBranch] = useState<string | null>(null);

    const handleBranchOpenChange = useCallback((open: boolean) => {
        setBranchOpen(open);
        if (!open) {
            setBranchQuery("");
            return;
        }
        setBranches(null);
        setBranchError(null);
        invoke<AgentGitBranch[]>("list_agent_git_branches", { conversationId: conversationIdNum })
            .then(setBranches)
            .catch((error) => setBranchError(errorText(error)));
    }, [conversationIdNum]);

    const handleSwitchBranch = useCallback(async (branch: AgentGitBranch) => {
        if (branch.is_current) {
            setBranchOpen(false);
            return;
        }
        setSwitchingBranch(branch.name);
        try {
            const next = await invoke<AgentGitStatus>("switch_agent_git_branch", {
                conversationId: conversationIdNum,
                branch: branch.name,
                isRemote: branch.is_remote,
            });
            setStatus(next);
            setBranchOpen(false);
            toast.success(`已切换到 ${next.branch ?? branch.name}`);
        } catch (error) {
            toast.error("切换分支失败", { description: errorText(error) });
        } finally {
            setSwitchingBranch(null);
        }
    }, [conversationIdNum]);

    const visibleBranches = useMemo(() => filterBranches(branches ?? [], branchQuery), [branches, branchQuery]);
    const localBranches = visibleBranches.filter((branch) => !branch.is_remote);
    const remoteBranches = visibleBranches.filter((branch) => branch.is_remote);

    // ---------------- 提交 ----------------
    const [commitOpen, setCommitOpen] = useState(false);
    const [changes, setChanges] = useState<AgentGitChangedFile[] | null>(null);
    const [changesError, setChangesError] = useState<string | null>(null);
    const [message, setMessage] = useState("");
    const [generating, setGenerating] = useState(false);
    const [committing, setCommitting] = useState<"commit" | "push" | null>(null);
    const generationIdRef = useRef(0);

    const loadChanges = useCallback(() => {
        setChangesError(null);
        invoke<AgentGitChangedFile[]>("list_agent_git_changes", { conversationId: conversationIdNum })
            .then(setChanges)
            .catch((error) => setChangesError(errorText(error)));
    }, [conversationIdNum]);

    useEffect(() => {
        setCommitOpen(false);
        setChanges(null);
        setMessage("");
        setGenerating(false);
        generationIdRef.current += 1;
    }, [conversationIdNum]);

    // 展开时以及状态刷新后（Agent 又改了文件）同步文件列表
    useEffect(() => {
        if (commitOpen && status?.has_uncommitted_changes) loadChanges();
    }, [commitOpen, status, loadChanges]);

    const handleGenerateMessage = useCallback(async () => {
        const generationId = ++generationIdRef.current;
        setGenerating(true);
        try {
            const text = await onGenerateCommitMessage();
            if (generationId !== generationIdRef.current) return;
            if (!text.trim()) {
                toast.error("Agent 没有返回提交信息", { description: "回复为空，请重试或手动填写" });
                return;
            }
            setMessage(text);
        } catch (error) {
            if (generationId !== generationIdRef.current) return;
            toast.error("生成提交信息失败", { description: errorText(error) });
        } finally {
            if (generationId === generationIdRef.current) setGenerating(false);
        }
    }, [onGenerateCommitMessage]);

    const handleCancelGenerate = useCallback(() => {
        generationIdRef.current += 1;
        setGenerating(false);
    }, []);

    const handleCommit = useCallback(async (push: boolean) => {
        setCommitting(push ? "push" : "commit");
        try {
            const result = await invoke<AgentGitCommitResult>("commit_agent_git_changes", {
                conversationId: conversationIdNum,
                message,
                push,
            });
            setStatus(result.status);
            setMessage("");
            setCommitOpen(false);
            if (push && !result.pushed) {
                toast.error(`已提交 ${result.commit}，但推送失败`, { description: result.push_error ?? "没有返回失败原因" });
            } else {
                toast.success(`${result.pushed ? "已提交并推送" : "已提交"} ${result.commit}`, { description: result.summary });
            }
        } catch (error) {
            toast.error(push ? "提交并推送失败" : "提交失败", { description: errorText(error) });
        } finally {
            setCommitting(null);
        }
    }, [conversationIdNum, message]);

    const [pushing, setPushing] = useState(false);
    const handlePush = useCallback(async () => {
        setPushing(true);
        try {
            const next = await invoke<AgentGitStatus>("push_agent_git_branch", { conversationId: conversationIdNum });
            setStatus(next);
            toast.success(`已推送到 ${next.upstream ?? "远程仓库"}`);
        } catch (error) {
            toast.error("推送失败", { description: errorText(error) });
        } finally {
            setPushing(false);
        }
    }, [conversationIdNum]);

    // ---------------- 压缩 ----------------
    const [compactPending, setCompactPending] = useState(false);
    useEffect(() => {
        if (!busy) setCompactPending(false);
    }, [busy]);
    const handleCompact = useCallback(async () => {
        setCompactPending(true);
        try {
            await onCompact();
        } catch (error) {
            setCompactPending(false);
            toast.error("压缩上下文失败", { description: errorText(error) });
        }
    }, [onCompact]);

    // ---------------- 渲染 ----------------
    const rowClass = cn(
        "flex w-full min-w-0 items-center gap-2 px-3 text-left text-sm transition-colors",
        mobile ? "min-h-11" : "h-8",
    );
    const interactiveRowClass = cn(rowClass, "hover:bg-muted/60 disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:bg-transparent");
    const iconClass = "h-4 w-4 shrink-0 text-muted-foreground";
    const busyTitle = "Agent 回复中，结束后再操作";

    const isRepo = Boolean(status?.is_repo);
    const hasChanges = Boolean(status?.has_uncommitted_changes);
    const ahead = status?.ahead ?? 0;
    const behind = status?.behind ?? 0;
    const diff = status?.diff;
    const branchLabel = status?.detached ? "分离 HEAD" : status?.branch ?? "（无分支）";

    const renderBranchItem = (branch: AgentGitBranch) => (
        <CommandItem
            key={branch.full_ref}
            value={branch.full_ref}
            onSelect={() => void handleSwitchBranch(branch)}
            disabled={Boolean(switchingBranch)}
            className="gap-2"
        >
            {switchingBranch === branch.name ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
                <Check className={cn("h-3.5 w-3.5", branch.is_current ? "opacity-100" : "opacity-0")} />
            )}
            <span className="min-w-0 flex-1 truncate">{branch.name}</span>
        </CommandItem>
    );

    return (
        <section
            className={cn("shrink-0 overflow-hidden rounded-xl border bg-background", mobile ? "mx-1 mb-2" : "mx-2 mt-2")}
            data-aipp-slot="chat-sidebar-agent-workspace"
            aria-label="Agent 工作区"
        >
            {statusError ? (
                <div className={cn(rowClass, "text-xs text-muted-foreground")} title={statusError}>
                    <GitBranch className={iconClass} />
                    <span className="min-w-0 flex-1 truncate">Git 状态读取失败：{statusError}</span>
                    <button
                        type="button"
                        className="shrink-0 rounded p-0.5 hover:bg-muted"
                        onClick={() => void refreshStatus()}
                        aria-label="重试读取 Git 状态"
                    >
                        <RefreshCw className={cn("h-3.5 w-3.5", refreshing && "animate-spin")} />
                    </button>
                </div>
            ) : null}

            {isRepo && status ? (
                <>
                    {/* 分支 */}
                    <Popover open={branchOpen} onOpenChange={handleBranchOpenChange}>
                        <PopoverTrigger asChild>
                            <button
                                type="button"
                                className={interactiveRowClass}
                                disabled={busy}
                                title={busy ? busyTitle : status.upstream ? `跟踪 ${status.upstream}` : "切换分支"}
                                data-aipp-slot="chat-sidebar-agent-branch"
                            >
                                <GitBranch className={iconClass} />
                                <span className="min-w-0 flex-1 truncate">{branchLabel}</span>
                                {ahead > 0 || behind > 0 ? (
                                    <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
                                        {ahead > 0 ? `↑${ahead}` : ""}{ahead > 0 && behind > 0 ? " " : ""}{behind > 0 ? `↓${behind}` : ""}
                                    </span>
                                ) : null}
                                <ChevronDown className={iconClass} />
                            </button>
                        </PopoverTrigger>
                        <PopoverContent align="start" side={mobile ? "bottom" : "left"} className="w-72 p-0">
                            <Command shouldFilter={false}>
                                <CommandInput
                                    placeholder="筛选分支…"
                                    value={branchQuery}
                                    onValueChange={setBranchQuery}
                                />
                                <CommandList>
                                    {branchError ? (
                                        <div className="px-3 py-4 text-xs text-muted-foreground break-all">读取分支失败：{branchError}</div>
                                    ) : branches === null ? (
                                        <div className="flex items-center justify-center gap-2 py-6 text-xs text-muted-foreground">
                                            <Loader2 className="h-3.5 w-3.5 animate-spin" />读取分支…
                                        </div>
                                    ) : (
                                        <>
                                            <CommandEmpty>没有匹配的分支</CommandEmpty>
                                            {localBranches.length > 0 ? (
                                                <CommandGroup heading="本地">{localBranches.map(renderBranchItem)}</CommandGroup>
                                            ) : null}
                                            {remoteBranches.length > 0 ? (
                                                <CommandGroup heading="远程">{remoteBranches.map(renderBranchItem)}</CommandGroup>
                                            ) : null}
                                        </>
                                    )}
                                </CommandList>
                            </Command>
                        </PopoverContent>
                    </Popover>

                    {/* 提交 / 推送 */}
                    {hasChanges ? (
                        <button
                            type="button"
                            className={interactiveRowClass}
                            onClick={() => setCommitOpen((open) => !open)}
                            aria-expanded={commitOpen}
                            data-aipp-slot="chat-sidebar-agent-commit"
                        >
                            <GitCommitHorizontal className={iconClass} />
                            <span className="min-w-0 flex-1 truncate">{status.has_remote ? "提交与推送" : "提交"}</span>
                            <ChevronDown className={cn(iconClass, "transition-transform", commitOpen && "rotate-180")} />
                        </button>
                    ) : ahead > 0 && status.has_remote ? (
                        <button
                            type="button"
                            className={interactiveRowClass}
                            onClick={() => void handlePush()}
                            disabled={pushing || busy}
                            title={busy ? busyTitle : `推送 ${ahead} 个提交到 ${status.upstream ?? "远程仓库"}`}
                            data-aipp-slot="chat-sidebar-agent-commit"
                        >
                            {pushing ? <Loader2 className={cn(iconClass, "animate-spin")} /> : <Upload className={iconClass} />}
                            <span className="min-w-0 flex-1 truncate">推送 {ahead} 个提交</span>
                        </button>
                    ) : (
                        <div className={cn(rowClass, "text-muted-foreground")} data-aipp-slot="chat-sidebar-agent-commit">
                            <GitCommitHorizontal className={iconClass} />
                            <span className="min-w-0 flex-1 truncate">没有待提交的改动</span>
                        </div>
                    )}

                    {hasChanges && commitOpen ? (
                        <div className="space-y-2 border-y bg-muted/20 px-3 py-2" data-aipp-slot="chat-sidebar-agent-commit-form">
                            <div className="flex min-w-0 items-center justify-between gap-2 text-xs text-muted-foreground">
                                <span className="min-w-0 truncate">
                                    {changes ? `${changes.length} 个文件` : "待提交的改动"}
                                </span>
                                {generating ? (
                                    <span className="flex shrink-0 items-center gap-1.5">
                                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                        Agent 正在起草…
                                        <button type="button" className="underline-offset-2 hover:underline" onClick={handleCancelGenerate}>
                                            不等了
                                        </button>
                                    </span>
                                ) : (
                                    <Button
                                        type="button"
                                        variant="ghost"
                                        size="sm"
                                        className="-mr-2 h-6 shrink-0 px-2 text-xs"
                                        onClick={() => void handleGenerateMessage()}
                                        disabled={busy || Boolean(committing)}
                                        title={busy ? busyTitle : "在对话里让 Agent 根据改动起草提交信息"}
                                    >
                                        <Sparkles className="h-3.5 w-3.5" />让 Agent 生成
                                    </Button>
                                )}
                            </div>
                            {changesError ? (
                                <div className="text-xs text-muted-foreground break-all">读取改动失败：{changesError}</div>
                            ) : changes === null ? (
                                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                    <Loader2 className="h-3.5 w-3.5 animate-spin" />读取改动…
                                </div>
                            ) : (
                                <ul className="max-h-36 space-y-0.5 overflow-y-auto text-xs" aria-label="待提交的文件">
                                    {changes.map((file) => {
                                        const { name, dir } = splitPath(file.path);
                                        return (
                                            <li key={file.path} className="flex min-w-0 items-center gap-1.5" title={file.original_path ? `${file.original_path} → ${file.path}` : file.path}>
                                                <span className="w-3 shrink-0 text-center font-mono text-muted-foreground">{STATUS_LETTER[file.status] ?? "M"}</span>
                                                <span className="shrink-0 truncate">{name}</span>
                                                <span className="min-w-0 flex-1 truncate text-muted-foreground">{dir}</span>
                                                {file.binary ? (
                                                    <span className="shrink-0 text-muted-foreground">二进制</span>
                                                ) : (
                                                    <LineCounts insertions={file.insertions} deletions={file.deletions} />
                                                )}
                                            </li>
                                        );
                                    })}
                                </ul>
                            )}
                            <Textarea
                                value={message}
                                onChange={(event) => setMessage(event.target.value)}
                                placeholder="提交信息"
                                aria-label="提交信息"
                                rows={3}
                                className="min-h-16 resize-none bg-background text-sm"
                                disabled={generating}
                            />
                            <div className="flex items-center justify-end gap-1.5">
                                <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    className="h-7 px-2.5 text-xs"
                                    onClick={() => void handleCommit(false)}
                                    disabled={!message.trim() || busy || generating || Boolean(committing)}
                                >
                                    {committing === "commit" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}提交
                                </Button>
                                {status.has_remote ? (
                                    <Button
                                        type="button"
                                        size="sm"
                                        className="h-7 px-2.5 text-xs"
                                        onClick={() => void handleCommit(true)}
                                        disabled={!message.trim() || busy || generating || Boolean(committing)}
                                    >
                                        {committing === "push" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}提交并推送
                                    </Button>
                                ) : null}
                            </div>
                        </div>
                    ) : null}

                    {/* 改动统计 */}
                    {diff ? (
                        <div
                            className={rowClass}
                            title={`${diff.base ? `对比 ${diff.base}` : "全部文件（还没有提交）"}：${diff.files_changed} 个文件，+${diff.insertions} −${diff.deletions}${diff.untracked_files > 0 ? `（含 ${diff.untracked_files} 个未跟踪文件）` : ""}`}
                            data-aipp-slot="chat-sidebar-agent-changes"
                        >
                            <FileDiff className={iconClass} />
                            <span className="shrink-0">改动</span>
                            <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
                                {diff.base ? `对比 ${diff.base}` : "全部文件"}
                            </span>
                            <LineCounts insertions={diff.insertions} deletions={diff.deletions} showZero />
                        </div>
                    ) : null}
                </>
            ) : null}

            {/* 压缩上下文 */}
            <button
                type="button"
                className={cn(interactiveRowClass, isRepo && "border-t")}
                onClick={() => void handleCompact()}
                disabled={!compactSupported || busy || compactPending}
                title={
                    !compactSupported
                        ? "当前 Agent 暂不支持手动压缩"
                        : busy
                            ? busyTitle
                            : "让 Agent 压缩当前会话的上下文，释放上下文窗口"
                }
                data-aipp-slot="chat-sidebar-agent-compact"
            >
                {compactPending ? <Loader2 className={cn(iconClass, "animate-spin")} /> : <Shrink className={iconClass} />}
                <span className="min-w-0 flex-1 truncate">{compactPending ? "正在压缩上下文…" : "压缩上下文"}</span>
                {!compactSupported ? <span className="shrink-0 text-xs text-muted-foreground">不支持</span> : null}
            </button>
        </section>
    );
};

export default React.memo(AgentWorkspacePanel);
