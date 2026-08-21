import { useMemo, useState } from "react";
import { CheckCircle2, ChevronDown, ChevronRight, FileDiff, Loader2, Terminal, Wrench, XCircle } from "lucide-react";
import type { AgentActivityEvent } from "@/data/Conversation";

function formatValue(value: unknown): string {
    if (typeof value === "string") return value;
    if (value === null || value === undefined) return "";
    try { return JSON.stringify(value, null, 2); } catch { return String(value); }
}

function ActivityIcon({ activity }: { activity: AgentActivityEvent }) {
    if (activity.status === "executing" || activity.status === "pending") return <Loader2 className="h-4 w-4 animate-spin" />;
    if (activity.status === "failed") return <XCircle className="h-4 w-4" />;
    if (activity.status === "success") return <CheckCircle2 className="h-4 w-4" />;
    if (activity.kind === "patch") return <FileDiff className="h-4 w-4" />;
    if (activity.kind === "command") return <Terminal className="h-4 w-4" />;
    return <Wrench className="h-4 w-4" />;
}

function ActivityCard({ activity }: { activity: AgentActivityEvent }) {
    const [expanded, setExpanded] = useState(activity.status === "executing" || activity.status === "failed");
    const input = formatValue(activity.input);
    const output = activity.output || activity.error || "";
    return (
        <div className="rounded-lg border border-border bg-background text-foreground">
            <button type="button" className="flex w-full items-center gap-2 px-3 py-2 text-left text-sm" onClick={() => setExpanded((value) => !value)}>
                {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
                <ActivityIcon activity={activity} />
                <span className="min-w-0 flex-1 truncate">{activity.title || activity.kind}</span>
                <span className="text-xs text-muted-foreground">{activity.status}</span>
            </button>
            {expanded && (input || output) && (
                <div className="space-y-2 border-t border-border p-3">
                    {input && <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted p-2 text-xs">{input}</pre>}
                    {output && <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted p-2 text-xs">{output}</pre>}
                </div>
            )}
        </div>
    );
}

export function AgentActivityList({ activities }: { activities: Map<string, AgentActivityEvent> }) {
    const ordered = useMemo(() => Array.from(activities.values()).sort((a, b) => a.sequence - b.sequence), [activities]);
    if (ordered.length === 0) return null;
    return (
        <div className="flex w-full max-w-[65%] flex-col gap-2 self-start" data-agent-activity-list>
            {ordered.map((activity) => <ActivityCard key={`${activity.agent_kind}:${activity.session_id ?? ""}:${activity.item_id}`} activity={activity} />)}
        </div>
    );
}
