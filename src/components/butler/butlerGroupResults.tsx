import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { MCPToolCall } from "@/data/MCPToolCall";
import { parseGroupTime, type ButlerGroupInteractionTurn, type ButlerGroupMessage } from "@/data/ButlerGroup";
import { parsePreviewFileRequestId } from "@/hooks/useInlineInteraction";
import { interactionContent, type InteractionRow } from "./butlerInteractions";
import { isButlerInteractionTool } from "./ButlerPresentationContext";
import ButlerInteractionResult from "./ButlerInteractionResult";
import ArtifactToolCall from "@/components/mcp-tool-components/ArtifactToolCall";
import { ReportBubble } from "./ButlerProgressRow";

interface GroupResult {
    key: string;
    conversation: number;
    owner: number;
    time: number;
    content: ReactNode;
    report?: string;
    questionCall?: number;
    liveQuestion?: boolean;
}

export interface GroupQuestion {
    key: string;
    call?: number;
    owner?: number;
    content: ReactNode;
    time: number;
    answered: boolean;
}

/** 成员执行会话对用户不可见，发布的 Artifact 只能靠群里的成果卡打开。 */
const isGroupResultCard = (tool?: string) => tool === "show_artifact";

function resultCard(conversation: number, owner: number, call: MCPToolCall): ReactNode {
    if (isGroupResultCard(call.tool_name)) {
        return <ArtifactToolCall toolName={call.tool_name} status={call.status} conversationId={conversation} messageId={owner}
            currentToolCall={{ call_id: call.id, conversation_id: call.conversation_id, status: call.status,
                tool_name: call.tool_name, parameters: call.parameters, result: call.result, error: call.error }} />;
    }
    return <ButlerInteractionResult toolName={call.tool_name} callId={call.id} status={call.status} conversationId={conversation}
        messageId={owner} parameters={call.parameters} isLastMessage={call.status === "executing"} />;
}

function groupResults(conversation: number, calls: MCPToolCall[], rows: InteractionRow[]): GroupResult[] {
    const events: GroupResult[] = [];
    const requests = new Set<string>();
    const represented = new Set<number>();
    for (const row of rows) {
        if (row.anchor == null || row.kind === "task") continue;
        const report = row.kind === "report" ? row.request.text : undefined;
        const content = report ? <ReportBubble text={report} /> : interactionContent(row, undefined, conversation);
        if (!content && row.kind !== "question") continue;
        requests.add(row.request_id);
        if ((row.kind === "question" || row.kind === "report") && row.request.call_id) represented.add(row.request.call_id);
        events.push({ key: row.request_id, conversation, owner: row.anchor, time: row.occurred_ms, report, content,
            questionCall: row.kind === "question" ? row.request.call_id ?? undefined : undefined });
    }
    for (const call of calls) {
        const owner = call.assistant_message_id ?? call.message_id;
        if (call.conversation_id !== conversation || !owner || represented.has(call.id)) continue;
        const pendingQuestion = call.tool_name === "ask_user_question" && call.status === "executing";
        if (call.status !== "success" && !(call.tool_name === "preview_code" && call.status === "executing") && !pendingQuestion) continue;
        let report: string | undefined;
        if (call.tool_name === "report_progress") {
            try { report = JSON.parse(call.parameters)?.text; } catch { continue; }
            if (typeof report !== "string" || !report.trim()) continue;
        } else if (!isButlerInteractionTool(call.tool_name) && !isGroupResultCard(call.tool_name)) continue;
        if (call.tool_name === "preview_file" && requests.has(parsePreviewFileRequestId(call.result) ?? "")) continue;
        const stamp = call.started_time || call.created_time;
        events.push({ key: `call-${call.id}`, conversation, owner, time: stamp ? parseGroupTime(stamp).getTime() : NaN, report,
            questionCall: call.tool_name === "ask_user_question" ? call.id : undefined,
            content: pendingQuestion ? null : report ? <ReportBubble text={report} /> : resultCard(conversation, owner, call) });
    }
    return events;
}

export function useGroupResults(conversations: string) {
    const [events, setEvents] = useState<Map<number, GroupResult[]>>(new Map());
    useEffect(() => {
        let active = true;
        setEvents(new Map());
        const subscriptions: Promise<() => void>[] = [];
        for (const id of conversations.split(",").filter(Boolean)) {
            const conversationId = Number(id);
            let revision = 0;
            async function refresh() {
                const current = ++revision;
                try {
                    const [calls, rows] = await Promise.all([
                        invoke<MCPToolCall[]>("get_mcp_tool_calls_by_conversation", { conversationId }),
                        invoke<InteractionRow[]>("list_butler_interactions", { conversationId }),
                    ]);
                    if (active && current === revision) {
                        const results = groupResults(conversationId, calls, rows);
                        setEvents(previous => new Map(previous).set(conversationId, results));
                    }
                } catch (error) { console.error("读取群成员交互记录失败", conversationId, error); }
            }
            const changed = listen<number>("butler_interactions_changed", e => { if (e.payload === conversationId) void refresh(); });
            const previews = ["preview-file-request", "preview-code-request"].map(event => listen<{ conversation_id?: number }>(event, e => {
                if (e.payload.conversation_id === conversationId) void refresh();
            }));
            const updates = listen<{ type: string; data?: { tool_name?: string } }>(`conversation_event_${id}`, ({ payload }) => {
                const tool = payload.data?.tool_name;
                if (payload.type === "mcp_tool_call_update" && (!tool || tool === "report_progress" || isButlerInteractionTool(tool) || isGroupResultCard(tool))) void refresh();
            });
            subscriptions.push(changed, ...previews, updates);
            void Promise.all([changed, ...previews, updates]).then(() => { if (active) void refresh(); }).catch(console.error);
        }
        return () => { active = false; subscriptions.forEach(p => void p.then(off => off()).catch(console.error)); };
    }, [conversations]);
    return events;
}

export function positionGroupResults(events: ReadonlyMap<number, GroupResult[]>, turns: ButlerGroupInteractionTurn[],
    messages: ButlerGroupMessage[], questions: ReadonlyMap<number, GroupQuestion>) {
    const before = new Map<number, GroupResult[]>();
    const tail: GroupResult[] = [];
    const currentReports = new Map<number, string[]>();
    const bySeq = new Map(messages.map(message => [message.seq, message]));
    const ordered = [...events.values()].flat();
    for (const [conversation, question] of questions) {
        const index = ordered.findIndex(e => e.conversation === conversation &&
            (e.key === question.key || (question.call != null && e.questionCall === question.call)));
        if (index === -1) {
            ordered.push({ key: question.key, conversation, owner: question.owner ?? 0, time: question.time,
                content: question.content, liveQuestion: !question.answered });
        } else if (!question.answered || !ordered[index].content) {
            ordered[index] = { ...ordered[index], content: question.content, liveQuestion: !question.answered };
        }
    }
    ordered.sort((a, b) => a.time - b.time || a.owner - b.owner);
    for (const event of ordered) {
        if (!event.content) continue;
        // The backend returns turns newest first. Only events preceding a stop belong in chat.
        const turn = turns.find(t => t.conversation_id === event.conversation && t.input_id <= event.owner);
        if (!turn) {
            if (event.liveQuestion) tail.push(event);
            continue;
        }
        const reply = turn.reply_seq === null ? undefined : bySeq.get(turn.reply_seq);
        if (turn.reply_seq !== null && (!reply?.source_message_id || event.owner > reply.source_message_id)) continue;
        if (!reply && turn.ended_at && (event.liveQuestion || !Number.isFinite(event.time) || event.time > parseGroupTime(turn.ended_at).getTime())) continue;
        if (!reply && !turn.ended_at && event.report) {
            const reports = currentReports.get(event.conversation) ?? [];
            reports.push(event.report);
            currentReports.set(event.conversation, reports);
        }
        const next = messages.find(message => message.seq > turn.trigger_seq &&
            (message.seq === reply?.seq || parseGroupTime(message.created_at).getTime() >= event.time));
        if (next) {
            const items = before.get(next.seq) ?? [];
            items.push(event);
            before.set(next.seq, items);
        } else tail.push(event);
    }
    return { before, tail, currentReports };
}
