import type { AgentActivityEvent, MCPToolCallUpdateEvent } from "@/data/Conversation";

/** Bridge activities and MCP history can both contain the same successful report. */
export function reportedProgressByMessage(conversation: number, activities: Iterable<AgentActivityEvent>,
    calls: Iterable<MCPToolCallUpdateEvent>): Map<number, string> {
    const lines = new Map<number, Set<string>>();
    const add = (owner: number, input: unknown) => {
        const text = (input as { text?: unknown } | null)?.text;
        if (typeof text !== "string" || !text.trim()) return;
        const reports = lines.get(owner) ?? new Set<string>();
        reports.add(text.trim());
        lines.set(owner, reports);
    };
    for (const activity of activities) {
        if (activity.conversation_id !== conversation || activity.kind !== "tool" || activity.status !== "success") continue;
        const meta = activity.metadata as { display_tool?: string; tool?: string } | null;
        const tool = meta?.display_tool ?? meta?.tool ?? activity.title ?? "";
        if (/^(?:mcp__aipp__)?(?:aipp_t\d+_)?report_progress$/.test(tool)) add(activity.response_message_id, activity.input);
    }
    for (const call of calls) {
        if (call.conversation_id !== conversation || call.status !== "success" || call.tool_name !== "report_progress"
            || !call.message_id || !call.parameters) continue;
        try { add(call.message_id, JSON.parse(call.parameters)); } catch { /* Incomplete historical parameters. */ }
    }
    return new Map(Array.from(lines, ([owner, reports]) => [owner, [...reports].join("\n")]));
}
