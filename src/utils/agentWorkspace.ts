import type { AcpAvailableCommand } from "@/data/Conversation";
import type { AgentGitBranch } from "@/data/AgentWorkspace";

/** 手动压缩上下文的指令；Claude Code / Codex 驱动识别后走各自原生的压缩通道。 */
export const MANUAL_COMPACT_PROMPT = "/compact";

/** 让当前 Agent 起草提交信息的指令（作为一轮普通对话发送）。 */
export const COMMIT_MESSAGE_PROMPT = [
    "请为工作目录里当前未提交的改动写一条 git 提交信息（可以查看 git status / git diff）。",
    "只回复提交信息本身：第一行是简短摘要，必要时空一行后列出要点。",
    "不要执行提交，也不要修改任何文件。",
].join("\n");

/**
 * 当前 Agent 是否支持手动压缩上下文：
 * Claude Code（CLI 自带 /compact）、Codex（thread/compact/start），以及在可用命令里声明了 compact 的 ACP Agent。
 */
export function agentSupportsManualCompact(
    agentKind: string | null | undefined,
    availableCommands: AcpAvailableCommand[] = [],
): boolean {
    if (agentKind === "claude_sdk" || agentKind === "codex_app_server") return true;
    return availableCommands.some((command) => command.name.replace(/^\//, "").toLowerCase() === "compact");
}

/** 1331 → "1,331" */
export function formatLineCount(value: number): string {
    return Math.max(0, Math.round(value)).toLocaleString("en-US");
}

/**
 * 从 Agent 回复里取出提交信息：优先取第一个代码块的内容，否则取全文；
 * 去掉“提交信息：”之类的前缀和首尾空白。
 */
export function extractCommitMessage(reply: string): string {
    const fenced = reply.match(/```[^\n]*\n([\s\S]*?)```/);
    const text = (fenced ? fenced[1] : reply).trim();
    return text.replace(/^(提交信息|commit message)\s*[:：]\s*/i, "").trim();
}

/** 按名称过滤分支（忽略大小写，空格分隔的多个词都要命中）。 */
export function filterBranches(branches: AgentGitBranch[], query: string): AgentGitBranch[] {
    const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (terms.length === 0) return branches;
    return branches.filter((branch) => {
        const name = branch.name.toLowerCase();
        return terms.every((term) => name.includes(term));
    });
}
