import React, { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Loader2, MessageSquareQuote } from "lucide-react";
import UnifiedMarkdown from "@/components/UnifiedMarkdown";
import { Button } from "@/components/ui/button";
import { getErrorMessage } from "@/utils/error";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";

/** 后端 `get_message_preview` 的返回（`api/message_preview.rs::MessagePreview`）。 */
export interface MessagePreviewData {
    message_id: number;
    conversation_id: number;
    conversation_kind: string;
    source: string;
    author: string;
    created_time: string;
    archived: boolean;
    content: string;
}

export interface MessagePreviewReference {
    messageId?: number;
    workRef?: string;
    title?: string;
}

/** 原文较长时先折叠，避免一张引用卡片占满整屏。 */
const COLLAPSE_CHARS = 1500;
const COLLAPSE_LINES = 30;

const parseJson = (raw?: string): unknown => {
    if (!raw?.trim()) return null;
    try {
        return JSON.parse(raw);
    } catch {
        return null;
    }
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
    Boolean(value) && typeof value === "object" && !Array.isArray(value);

const toMessageId = (value: unknown): number | undefined => {
    const id = typeof value === "string" ? Number(value.trim().replace(/^#/, "")) : value;
    return typeof id === "number" && Number.isInteger(id) && id > 0 ? id : undefined;
};

/** 回执里的 message_id：Agent 工具把回执 JSON 放在 text 内容里，也兼容 json 内容。 */
const receiptMessageId = (result?: string): number | undefined => {
    const visit = (node: unknown): number | undefined => {
        if (typeof node === "string") return visit(parseJson(node));
        if (Array.isArray(node)) {
            for (const item of node) {
                const found = visit(item);
                if (found) return found;
            }
            return undefined;
        }
        if (!isRecord(node)) return undefined;
        if (node.status === "preview_shown") return toMessageId(node.message_id);
        return visit(node.content) ?? visit(node.text) ?? visit(node.json);
    };
    return visit(parseJson(result));
};

/**
 * 卡片要展示哪一条：执行成功后以回执里解析好的 message_id 为准（work_ref 不再重新解析），
 * 否则用调用参数里的 message_id / work_ref。
 */
export function resolvePreviewReference(parameters?: string, result?: string): MessagePreviewReference | null {
    const args = parseJson(parameters);
    if (!isRecord(args)) return null;
    const title = typeof args.title === "string" && args.title.trim() ? args.title.trim() : undefined;
    const resolved = receiptMessageId(result);
    if (resolved) return { messageId: resolved, title };
    const messageId = toMessageId(args.message_id);
    const workRef = typeof args.work_ref === "string" && args.work_ref.trim() ? args.work_ref.trim() : undefined;
    if (!messageId && !workRef) return null;
    return { messageId, workRef: messageId ? undefined : workRef, title };
}

const formatTime = (value: string): string => {
    const date = new Date(value.includes("T") ? value : value.replace(" ", "T"));
    if (Number.isNaN(date.getTime())) return "";
    return date.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
};

const shouldCollapse = (content: string) =>
    content.length > COLLAPSE_CHARS || content.split("\n").length > COLLAPSE_LINES;

type LoadState =
    | { kind: "loading" }
    | { kind: "ready"; preview: MessagePreviewData }
    | { kind: "error"; message: string };

const MessagePreviewToolCall: React.FC<McpToolComponentProps> = (props) => {
    const status = props.currentToolCall?.status ?? props.status;
    const reference = useMemo(
        () => resolvePreviewReference(props.parameters, props.currentToolCall?.result),
        [props.parameters, props.currentToolCall?.result],
    );
    const viewer = props.conversationId;
    const waiting = status === "pending" || status === "executing" || props.isStreaming;
    const failed = status === "failed";
    const key = reference && viewer ? `${viewer}:${reference.messageId ?? reference.workRef}` : null;
    const [state, setState] = useState<{ key: string; value: LoadState } | null>(null);
    const [expanded, setExpanded] = useState(false);

    const messageId = reference?.messageId;
    const workRef = reference?.workRef;
    useEffect(() => {
        if (!key || viewer === undefined || waiting || failed) return;
        let active = true;
        setState({ key, value: { kind: "loading" } });
        invoke<MessagePreviewData>("get_message_preview", {
            viewerConversationId: viewer,
            messageId: messageId ?? null,
            workRef: workRef ?? null,
        })
            .then((preview) => {
                if (active) setState({ key, value: { kind: "ready", preview } });
            })
            .catch((error) => {
                if (active) setState({ key, value: { kind: "error", message: getErrorMessage(error) } });
            });
        return () => {
            active = false;
        };
    }, [key, viewer, waiting, failed, messageId, workRef]);

    const current = state?.key === key ? state.value : null;

    if (failed) {
        const reason = props.currentToolCall?.error ?? props.error ?? "工具执行失败";
        return <PreviewNotice>引用的消息无法展示：{reason}</PreviewNotice>;
    }
    if (waiting || !current || current.kind === "loading") {
        return (
            <PreviewNotice>
                <Loader2 className="h-3.5 w-3.5 animate-spin text-muted-foreground" />
                {waiting ? "正在展示引用的消息…" : "正在读取引用的消息…"}
            </PreviewNotice>
        );
    }
    if (current.kind === "error") {
        return <PreviewNotice>引用的消息无法展示：{current.message}</PreviewNotice>;
    }

    const { preview } = current;
    const collapsible = shouldCollapse(preview.content);
    const collapsed = collapsible && !expanded;
    const time = formatTime(preview.created_time);
    return (
        <figure
            className="my-2 w-full max-w-3xl overflow-hidden rounded-lg border border-border bg-muted/30"
            data-aipp-slot="message-preview"
            data-message-preview-id={preview.message_id}
        >
            <figcaption className="flex min-w-0 items-center gap-2 border-b border-border/60 px-3 py-2 text-xs text-muted-foreground">
                <MessageSquareQuote className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate font-medium text-foreground">
                    {reference?.title ?? `${preview.author} 的回复`}
                </span>
                <span className="truncate">{preview.source}</span>
                {time && <time className="ml-auto shrink-0" dateTime={preview.created_time}>{time}</time>}
            </figcaption>
            <div className="relative px-3 py-2 text-sm">
                <div className={collapsed ? "max-h-96 overflow-hidden" : undefined}>
                    <UnifiedMarkdown>{preview.content}</UnifiedMarkdown>
                </div>
                {collapsed && (
                    <div className="pointer-events-none absolute inset-x-0 bottom-0 h-16 bg-gradient-to-t from-background to-transparent" />
                )}
            </div>
            {collapsible && (
                <div className="flex justify-center border-t border-border/60 py-1">
                    <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => setExpanded((value) => !value)}>
                        {expanded ? "收起" : "展开全文"}
                    </Button>
                </div>
            )}
        </figure>
    );
};

function PreviewNotice({ children }: { children: React.ReactNode }) {
    return (
        <div
            className="my-2 flex w-full max-w-3xl items-center gap-2 rounded-lg border border-dashed border-border px-3 py-2 text-xs text-muted-foreground"
            data-aipp-slot="message-preview-notice"
        >
            {children}
        </div>
    );
}

export default MessagePreviewToolCall;
