import React, { useMemo, useState } from "react";
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
                <span className="min-w-0 truncate text-[11px] text-muted-foreground" title={bucket.error}>
                    {bucket.error}
                </span>
            ) : null}
        </div>
    );
}

function AttemptCopyButton({ messages }: { messages: Message[] }) {
    const [copied, setCopied] = useState(false);
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

    if (!text.trim()) {
        return null;
    }

    return (
        <button
            type="button"
            className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
            aria-label="复制此尝试内容"
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
                {renderMessages(bucket.messages)}
            </CollapsibleContent>
        </Collapsible>
    );
}

/**
 * When stream retries leave partial answers, render them as attempt-grouped
 * collapsible sections instead of a flat list (LeaderAI #159 / UX-05).
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
    const showGrouping = shouldShowStreamRetryGrouping(buckets);

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
