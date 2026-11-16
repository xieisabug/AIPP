import { useContext, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import InlineCodePreviewCard from "@/components/InlineCodePreviewCard";
import { AskUserQuestionCard, PreviewFileCard, type PreviewFileRequest } from "@/components/InlineInteractionCards";
import type { MCPToolCall } from "@/data/MCPToolCall";
import { parsePreviewFileRequest, toAskUserPersistedState } from "@/hooks/useInlineInteraction";
import { loadMcpToolCall } from "@/services/conversationResourceCache";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";
import { parsePreviewCodeStreamingState } from "@/utils/previewCode";
import { ButlerLiveCardsContext } from "./ButlerPresentationContext";

function StoredResult({ callId, status, conversationId }: Pick<McpToolComponentProps, "callId" | "status" | "conversationId">) {
    const [file, setFile] = useState<PreviewFileRequest | null>(null);
    const [question, setQuestion] = useState<ReturnType<typeof toAskUserPersistedState>>(null);
    const [error, setError] = useState("");
    useEffect(() => {
        let active = true;
        if (!callId || !conversationId) return;
        setError("");
        async function restore(call: MCPToolCall) {
            if (!active || call.status !== "success" || call.conversation_id !== conversationId) return;
            if (call.tool_name === "ask_user_question") {
                setQuestion(toAskUserPersistedState(call, conversationId));
            } else if (call.tool_name === "preview_file") {
                const request = parsePreviewFileRequest(call.parameters, conversationId, `persisted-preview-${call.id}`);
                if (!request) return;
                const prepared = await invoke<PreviewFileRequest>("prepare_preview_file_request_for_ui", { conversationId, request });
                if (active) setFile(prepared);
            }
        }
        loadMcpToolCall(callId).then(restore).catch(reason => {
            if (active) setError(String(reason));
        });
        return () => { active = false; };
    }, [callId, conversationId, status]);
    if (error) return <div role="alert" className="text-xs text-destructive">读取交互结果失败：{error}</div>;
    if (file) return <PreviewFileCard request={file} isOpen />;
    if (question?.answers) return <AskUserQuestionCard request={question.request} isOpen readOnly viewMode="summary"
        completedAnswers={question.answers} onSubmit={() => {}} onCancel={() => {}} />;
    return null;
}

export default function ButlerInteractionResult(props: McpToolComponentProps) {
    const live = useContext(ButlerLiveCardsContext);
    const state = props.callId ? props.mcpToolCallStates?.get(props.callId) : undefined;
    const status = state?.status ?? props.status;
    if (props.toolName === "preview_code") {
        if (status === "failed") return null;
        return <InlineCodePreviewCard conversationId={props.conversationId} messageId={props.messageId} callId={props.callId}
            llmCallId={props.llmCallId} mcpToolCallStates={props.mcpToolCallStates} isLastMessage={props.isLastMessage} isStreaming={props.isStreaming}
            parameters={state?.parameters ?? props.parameters ?? "{}"}
            streamingPreviewState={parsePreviewCodeStreamingState(props.streamingPreviewState) ?? undefined} />;
    }
    if (!props.callId || props.callId === (props.toolName === "preview_file" ? live.file : live.question)) return null;
    return <StoredResult key={props.callId} callId={props.callId} status={status} conversationId={props.conversationId} />;
}
