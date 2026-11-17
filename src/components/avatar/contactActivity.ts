import type { ConversationRuntimeState } from "@/data/Conversation";
import type { AvatarState } from "./avatarDefinition";

/** 前端从会话事件里推断出的细节：当前执行的工具、是否在自动审核、正在流式输出的消息类型。 */
export interface ContactDetail {
    native?: ContactVisual;
    tool?: string;
    path?: string;
    reviewing?: boolean;
    stream?: "reasoning" | "response" | "plan";
}
export interface ContactActivity {
    assistant_id: number;
    conversation_id: number;
    status: string | null;
    waiting_peer: boolean;
    runtime: ConversationRuntimeState;
    permission: boolean;
    question: boolean;
    detail?: ContactDetail;
}
export interface ContactVisual { state: AvatarState; label: string }
export interface ContactFeedback { state: "finished" | "failed" | "notifying" | "celebrate" | "blocked"; until: number }
export const successful = (status: string | null) => status === "completed" || status === "succeeded";
export const terminal = (status: string | null) => successful(status) || ["failed", "cancelled", "interrupted"].includes(status ?? "");

/** 助手在干活时的状态（工具细分、思考、输出），它们之间切换需要最短停留，避免头像抽搐。 */
export const BUSY_STATES = new Set<AvatarState>([
    "listening", "thinking", "streaming", "working", "suspicious", "searching", "receiving", "reading",
    "writing", "coding", "running", "loading", "controlling", "sending", "spawning",
]);

const CODE_FILE = /\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|swift|c|cc|cpp|h|hpp|cs|rb|php|vue|svelte|sh|bash|ps1|sql|lua|dart|scala|zig)$/i;
const toolVisuals: [string[], ContactVisual][] = [
    [["search_web", "search", "search_own_history", "search_bot_history"], { state: "searching", label: "搜索中" }],
    [["fetch_url", "web_fetch"], { state: "receiving", label: "抓取网页" }],
    [["read_file", "list_directory", "read_work_context", "read_bot_message_context", "get_artifact_workspace",
        "superadmin_inspect", "superadmin_catalog", "superadmin_audit_query"], { state: "reading", label: "阅读中" }],
    [["todo_write", "schedule_task"], { state: "writing", label: "规划中" }],
    [["execute_bash", "get_bash_output"], { state: "running", label: "运行命令" }],
    [["load_skill", "load_mcp_server", "load_mcp_tool"], { state: "loading", label: "加载能力" }],
    [["superadmin_execute", "superadmin_batch", "superadmin_undo"], { state: "controlling", label: "操控中" }],
    [["message_bot", "spawn_task_conversation", "task_conversation_operation"], { state: "sending", label: "联络助手" }],
    [["show_artifact", "preview_file", "preview_code", "preview_message", "capture_artifact_screenshot"], { state: "spawning", label: "生成展示" }],
];
function writingVisual(hasCode: boolean): ContactVisual {
    return hasCode ? { state: "coding", label: "编程中" } : { state: "writing", label: "书写中" };
}

export function toolVisual(tool?: string, path?: string): ContactVisual {
    if (tool === "write_file" || tool === "edit_file") {
        return writingVisual(CODE_FILE.test(path ?? ""));
    }
    return toolVisuals.find(([names]) => tool && names.includes(tool))?.[1] ?? { state: "working", label: "执行中" };
}

export type NativeActivity = { item_id?: string; kind?: string; status?: string; input?: unknown; metadata?: unknown; title?: string | null };
export function updateNativeActivity(active: Map<string, ContactVisual>, event: NativeActivity) {
    if (!event.item_id) return;
    if (event.status !== "executing" && event.status !== "running" && event.status !== "pending") {
        active.delete(event.item_id);
        return;
    }
    const input = event.input as { changes?: unknown; path?: string; file_path?: string; tool_name?: string; parameters?: { path?: string; file_path?: string } } | null;
    const metadata = event.metadata as { tool?: string; display_tool?: string } | null;
    let visual: ContactVisual;
    switch (event.kind) {
        case "reasoning": visual = { state: "thinking", label: "思考中" }; break;
        case "command": visual = toolVisual("execute_bash"); break;
        case "file_read": visual = toolVisual("read_file"); break;
        case "file_search": visual = { state: "searching", label: "搜索文件" }; break;
        case "patch":
        case "file_write": {
            const hasCodeChange = Array.isArray(input?.changes) && input.changes.some(
                change => typeof change?.path === "string" && CODE_FILE.test(change.path),
            );
            visual = writingVisual(hasCodeChange || CODE_FILE.test(input?.path ?? input?.file_path ?? ""));
            break;
        }
        case "sub_agent": visual = { state: "spawning", label: "委派助手" }; break;
        default: {
            const name = metadata?.display_tool ?? metadata?.tool ?? event.title;
            const tool = typeof name === "string" ? name.replace(/^aipp_t\d+_/, "") : undefined;
            const dynamic = tool === "call_mcp_tool";
            const args = dynamic ? input?.parameters : input;
            visual = toolVisual(dynamic ? input?.tool_name : tool, args?.file_path ?? args?.path);
        }
    }
    active.set(event.item_id, visual);
}

const feedbackLabels: Record<ContactFeedback["state"], string> = {
    finished: "已完成", failed: "执行出错", notifying: "新消息", celebrate: "全部完成", blocked: "已中断",
};

export function contactVisual(rows: ContactActivity[], feedback: ContactFeedback | undefined, failed: boolean, now: number): ContactVisual {
    if (rows.some(r => r.permission)) return { state: "alerting", label: "待审批" };
    if (rows.some(r => r.question)) return { state: "waiting", label: "等你回答" };
    if (failed) return { state: feedback?.state === "failed" && feedback.until > now ? "failed" : "idle", label: "执行出错" };
    const active = rows.filter(r => r.runtime.is_running);
    if (active.some(r => r.detail?.reviewing)) return { state: "suspicious", label: "审核中" };
    const executing = active.find(r => r.runtime.phase === "mcp_executing");
    if (executing) return toolVisual(executing.detail?.tool, executing.detail?.path);
    const streaming = active.find(r => r.runtime.phase === "assistant_streaming");
    if (streaming) {
        if (streaming.detail?.native) return streaming.detail.native;
        if (streaming.detail?.stream === "reasoning") return { state: "thinking", label: "思考中" };
        if (streaming.detail?.stream === "plan") return { state: "writing", label: "制定计划" };
        return { state: "streaming", label: "回复中" };
    }
    // 还没有推理或回复 token：请求已发出，模型尚未开始输出。不叫思考。
    if (active.length) return { state: "sending", label: "投递中" };
    if (rows.some(r => r.waiting_peer)) return { state: "waiting", label: "待收协作答复" };
    if (rows.some(r => r.status === "running")) return { state: "working", label: "任务处理中" };
    if (rows.some(r => r.status === "accepted")) return { state: "receiving", label: "已接收任务" };
    if (feedback && feedback.until > now) return { state: feedback.state, label: feedbackLabels[feedback.state] };
    return { state: "idle", label: "" };
}
