import { ButlerResultsOnlyContext } from "../butler/ButlerPresentationContext";
import React, { useContext, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Copy, Check } from "lucide-react";

import type { Message } from "@/data/Conversation";
import { Badge } from "@/components/ui/badge";
import {
    Collapsible,
    CollapsibleContent,
    CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
    groupMessagesByStreamRetryAttempt,
    isInterruptedStreamRetryBucket,
    shouldShowStreamRetryGrouping,
    type StreamRetryAttemptBucket,
} from "@/utils/streamRetryAttempts";
import { cn } from "@/utils/utils";

export interface StreamRetryAttemptSectionsProps {
    messages: Message[];
    renderMessages: (messages: Message[]) => React.ReactNode;
    className?: string;
}

function attemptTitle(bucket: StreamRetryAttemptBucket): string {
    if (bucket.attempt == null) {
        return "输出";
    }
    return `尝试 ${bucket.attempt}`;
}

function AttemptHeader({
    bucket,
    open,
}: {
    bucket: StreamRetryAttemptBucket;
    open: boolean;
}) {
    const interrupted = isInterruptedStreamRetryBucket(bucket);
    return (
        <div className="flex min-w-0 flex-1 items-center gap-2 text-left text-xs text-muted-foreground">
            {open ? (
                <ChevronDown className="h-3.5 w-3.5 shrink-0 text-icon" />
            ) : (
                <ChevronRight className="h-3.5 w-3.5 shrink-0 text-icon" />
            )}
            <span className="font-medium text-foreground">{attemptTitle(bucket)}</span>
            {interrupted ? (
                <Badge
                    variant="outline"
                    className="border-amber-300 bg-amber-50 text-amber-800 dark:border-amber-800 dark:bg-amber-950/40 dark:text-amber-200"
                >
                    已中断
                </Badge>
            ) : bucket.status === "completed" ? (
                <Badge variant="secondary">完成</Badge>
            ) : null}
            {bucket.error ? (
                // 标题行只放一行摘要；全文在展开区诊断块里，触控端不依赖 hover/title。
                <span className="min-w-0 truncate text-[11px] text-muted-foreground">
                    {bucket.error}
                </span>
            ) : null}
        </div>
    );
}

function CopyTextButton({ text, label }: { text: string; label: string }) {
    const [copied, setCopied] = useState(false);
    if (!text.trim()) {
        return null;
    }
    return (
        <button
            type="button"
            className="inline-flex h-11 w-11 min-h-11 min-w-11 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
            aria-label={label}
            onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                void navigator.clipboard.writeText(text).then(() => {
                    setCopied(true);
                    window.setTimeout(() => setCopied(false), 1500);
                });
            }}
        >
            {copied ? <Check className="h-3.5 w-3.5 text-icon" /> : <Copy className="h-3.5 w-3.5 text-icon" />}
        </button>
    );
}

/** 中断诊断全文：可换行、可选中，并能单独复制，不和回答内容混在一起。 */
function AttemptErrorDiagnostic({ error }: { error: string }) {
    return (
        <div
            className="mb-2 flex items-start gap-1 rounded-md border border-border/70 bg-background/60 pl-2"
            data-stream-retry-error
        >
            <pre className="min-w-0 flex-1 select-text whitespace-pre-wrap break-all py-2 font-mono text-[11px] leading-relaxed text-muted-foreground">
                {error}
            </pre>
            <CopyTextButton text={error} label="复制中断原因" />
        </div>
    );
}

function AttemptCopyButton({ messages }: { messages: Message[] }) {
    const text = useMemo(() => {
        const responses = messages
            .filter((message) => message.message_type === "response")
            .map((message) => message.content)
            .filter(Boolean);
        if (responses.length > 0) {
            return responses.join("\n");
        }
        return messages.map((message) => message.content).filter(Boolean).join("\n");
    }, [messages]);

    return <CopyTextButton text={text} label="复制此尝试内容" />;
}

function AttemptSection({
    bucket,
    defaultOpen,
    renderMessages,
}: {
    bucket: StreamRetryAttemptBucket;
    defaultOpen: boolean;
    renderMessages: (messages: Message[]) => React.ReactNode;
}) {
    const [open, setOpen] = useState(defaultOpen);
    const interrupted = isInterruptedStreamRetryBucket(bucket);
    const showChrome = bucket.attempt != null;

    if (!showChrome) {
        return <>{renderMessages(bucket.messages)}</>;
    }

    return (
        <Collapsible open={open} onOpenChange={setOpen} className="rounded-lg border border-border/70 bg-muted/20">
            <div className="flex items-center gap-1 px-2 py-1.5">
                <CollapsibleTrigger asChild>
                    <button
                        type="button"
                        className="flex min-h-11 min-w-0 flex-1 items-center rounded-md px-1 py-1 hover:bg-muted/60"
                        data-stream-retry-attempt={bucket.attempt ?? undefined}
                        data-stream-retry-status={bucket.status ?? undefined}
                    >
                        <AttemptHeader bucket={bucket} open={open} />
                    </button>
                </CollapsibleTrigger>
                <AttemptCopyButton messages={bucket.messages} />
            </div>
            <CollapsibleContent className={cn("px-2 pb-2", interrupted && "opacity-95")}>
                {bucket.error ? <AttemptErrorDiagnostic error={bucket.error} /> : null}
                {renderMessages(bucket.messages)}
            </CollapsibleContent>
        </Collapsible>
    );
}

/**
 * When stream retries leave partial answers, render them as attempt-grouped
 * collapsible sections instead of a flat list (LeaderAI #159 / UX-05).
 *
 * 适用范围：
 * - 普通会话（合并与非合并模式）：有 stream_retry 元数据且需要分组时，按尝试分组；
 *   非合并模式仍保留每条消息自身的气泡和操作，只共享尝试标题与诊断；
 * - Butler resultsOnly：只看最终结果，保持平铺、不显示诊断；
 * - 没有元数据的旧历史：attempt 为空，原样平铺，不伪造尝试序号。
 */
export function StreamRetryAttemptSections({
    messages,
    renderMessages,
    className,
}: StreamRetryAttemptSectionsProps) {
    const buckets = useMemo(
        () => groupMessagesByStreamRetryAttempt(messages),
        [messages],
    );
    const resultsOnly = useContext(ButlerResultsOnlyContext);
    const showGrouping = !resultsOnly && shouldShowStreamRetryGrouping(buckets);

    if (!showGrouping) {
        return <>{renderMessages(messages)}</>;
    }

    const lastTaggedIndex = (() => {
        for (let index = buckets.length - 1; index >= 0; index -= 1) {
            if (buckets[index].attempt != null) {
                return index;
            }
        }
        return -1;
    })();

    return (
        <div className={cn("flex flex-col gap-2", className)} data-stream-retry-groups>
            {buckets.map((bucket, index) => {
                const isLatestTagged = index === lastTaggedIndex;
                const defaultOpen = isLatestTagged || !isInterruptedStreamRetryBucket(bucket);
                return (
                    <AttemptSection
                        key={`stream-retry-${bucket.sessionId ?? "none"}-${bucket.attempt ?? "plain"}-${bucket.messages[0]?.id ?? index}`}
                        bucket={bucket}
                        defaultOpen={defaultOpen}
                        renderMessages={renderMessages}
                    />
                );
            })}
        </div>
    );
}

export default StreamRetryAttemptSections;
