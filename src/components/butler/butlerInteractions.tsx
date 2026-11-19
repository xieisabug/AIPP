import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AskUserQuestionCard, PreviewFileCard, type AskUserQuestionRequest, type PreviewFileRequest } from "@/components/InlineInteractionCards";
import type { InlineInteractionItem } from "@/components/ConversationUI";
import type { MCPToolCallUpdateEvent } from "@/data/Conversation";
import ButlerInteractionResult from "./ButlerInteractionResult";
import InlineCodePreviewCard from "@/components/InlineCodePreviewCard";

type Position = { id: number; request_id: string; anchor: number | null; occurred_ms: number };
export type InteractionRow = Position & (
    { kind: "question"; request: AskUserQuestionRequest; answers: Record<string, string> | null }
    | { kind: "file" }
    | { kind: "code"; request: { parameters: string; status?: string } }
    | { kind: "task" }
    | { kind: "report"; request: { text: string; call_id?: number } }
);

export function useButlerInteractions(conversationId: number, enabled: boolean) {
    const [snapshot, setSnapshot] = useState<{ id: number; rows: InteractionRow[] }>();
    useEffect(() => {
        if (!enabled || !conversationId) return;
        let active = true;
        let revision = 0;
        async function refresh() {
            const ticket = ++revision;
            try {
                const rows = await invoke<InteractionRow[]>("list_butler_interactions", { conversationId });
                if (active && ticket === revision) setSnapshot({ id: conversationId, rows });
            } catch (error) {
                if (active) console.error("读取管家交互记录失败", conversationId, error);
            }
        }
        const subscriptions = [
            listen<number>("butler_interactions_changed", e => { if (e.payload === conversationId) void refresh(); }),
            ...["ask-user-question-request", "preview-file-request", "preview-code-request"].map(event =>
                listen<{ conversation_id?: number }>(event, e => { if (e.payload.conversation_id === conversationId) void refresh(); })),
        ];
        void Promise.all(subscriptions).then(refresh).catch(console.error);
        return () => { active = false; subscriptions.forEach(p => void p.then(off => off()).catch(console.error)); };
    }, [conversationId, enabled]);
    return enabled && snapshot?.id === conversationId ? snapshot.rows : undefined;
}

function HistoricalFile({ requestId, conversationId }: { requestId: string; conversationId: number }) {
    const [file, setFile] = useState<PreviewFileRequest>();
    const [error, setError] = useState("");
    useEffect(() => {
        let active = true;
        invoke<PreviewFileRequest>("prepare_butler_file_preview", { conversationId, requestId })
            .then(result => { if (active) setFile(result); })
            .catch(reason => { if (active) setError(String(reason)); });
        return () => { active = false; };
    }, [conversationId, requestId]);
    if (error) return <div role="alert">{error}</div>;
    return file ? <PreviewFileCard request={file} isOpen /> : null;
}

export function interactionContent(row: InteractionRow, live: InlineInteractionItem | undefined, conversationId: number) {
    if (row.kind === "question" && row.answers) {
        return <AskUserQuestionCard request={row.request} isOpen readOnly viewMode="summary"
            completedAnswers={row.answers} onSubmit={() => {}} onCancel={() => {}} />;
    }
    if (live) return live.content;
    if (row.kind === "file") return <HistoricalFile key={row.request_id} requestId={row.request_id} conversationId={conversationId} />;
    if (row.kind === "code" && row.request.status !== "failed") return <InlineCodePreviewCard key={row.request_id}
        conversationId={conversationId} messageId={row.anchor ?? undefined} parameters={row.request.parameters}
        sourceRequestId={row.request_id} savedStatus={row.request.status} isLastMessage={!row.request.status} />;
    return null;
}

export function storedCallContent(call: MCPToolCallUpdateEvent) {
    return <ButlerInteractionResult toolName={call.tool_name} callId={call.call_id} status={call.status}
        conversationId={call.conversation_id} messageId={call.message_id} llmCallId={call.llm_call_id}
        parameters={call.parameters} isLastMessage={call.status === "executing"} />;
}
