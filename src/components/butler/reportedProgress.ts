import type { AgentActivityEvent, MCPToolCallUpdateEvent } from "@/data/Conversation";
import { parseDatabaseTime } from "@/utils/messageOrdering";
import type { InteractionRow } from "./butlerInteractions";

export interface ProgressReport { text: string; time?: number; order: number }

export function reportedProgressByMessage(conversation: number, activities: Iterable<AgentActivityEvent>,
    calls: Iterable<MCPToolCallUpdateEvent>, saved: readonly InteractionRow[] = []): Map<number, ProgressReport[]> {
    const result = new Map<number, ProgressReport[]>();
    const add = (owner: number, report: ProgressReport) => {
        const reports = result.get(owner) ?? [];
        reports.push(report);
        result.set(owner, reports);
    };
    const mirrored = new Map<number, string[]>();
    for (const activity of activities) {
        if (activity.conversation_id !== conversation || activity.kind !== "tool" || activity.status !== "success") continue;
        const meta = activity.metadata as { display_tool?: string; tool?: string; startedAtMs?: number } | null;
        const tool = meta?.display_tool ?? meta?.tool ?? activity.title ?? "";
        if (!/^(?:mcp__aipp__)?(?:aipp_t\d+_)?report_progress$/.test(tool)) continue;
        const text = (activity.input as { text?: string } | null)?.text;
        if (typeof text !== "string" || !text.trim()) continue;
        const owner = activity.response_message_id;
        add(owner, { text: text.trim(), order: activity.order ?? activity.sequence,
            time: activity.started_at ? Date.parse(activity.started_at) : meta?.startedAtMs });
        const copies = mirrored.get(owner) ?? [];
        copies.push(text.trim());
        mirrored.set(owner, copies);
    }
    for (const call of calls) {
        if (call.conversation_id !== conversation || call.status !== "success" || call.tool_name !== "report_progress"
            || !call.message_id || !call.parameters) continue;
        try {
            const text = JSON.parse(call.parameters)?.text;
            if (typeof text !== "string" || !text.trim()) continue;
            // One MCP call mirrors at most one native activity. Identical separate calls are separate bubbles.
            const copies = mirrored.get(call.message_id);
            const index = copies?.indexOf(text.trim()) ?? -1;
            if (copies && index >= 0) { copies.splice(index, 1); continue; }
            add(call.message_id, { text: text.trim(), order: call.call_id,
                time: call.started_time ? parseDatabaseTime(call.started_time).getTime() : undefined });
        } catch { /* Incomplete historical parameters. */ }
    }
    // Recent calls carry a tool-owned record even if the native driver omitted the activity.
    // Replace one legacy observation per occurrence, keeping its siblings when text repeats.
    const recorded = new Map<number, ProgressReport[]>();
    for (const row of saved) {
        if (row.kind !== "report" || row.anchor == null) continue;
        const text = row.request.text.trim();
        const legacy = result.get(row.anchor);
        const index = legacy?.findIndex(report => report.text === text) ?? -1;
        if (legacy && index >= 0) legacy.splice(index, 1);
        const list = recorded.get(row.anchor) ?? [];
        list.push({ text, time:row.occurred_ms, order:row.request.call_id ?? row.id });
        recorded.set(row.anchor, list);
    }
    for (const [owner, list] of recorded) result.set(owner, [...(result.get(owner) ?? []), ...list]);
    for (const reports of result.values()) reports.sort((a, b) => (a.time ?? 0) - (b.time ?? 0) || a.order - b.order);
    return result;
}
