import { Fragment, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight, KeyRound, TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { VaultEntryLabel, VaultUseDecision, VaultUseRequest } from "@/data/Vault";
import type { InlineInteractionItem } from "@/components/ConversationUI";

interface VaultUseCardProps {
    request: VaultUseRequest;
    onRespond: (requestId: string, decision: VaultUseDecision) => Promise<void>;
}

const PLACEHOLDER = /<vault:([a-z][a-z0-9_]{1,39})>/g;

function hostOf(origin: string): string {
    try {
        return new URL(origin).host;
    } catch {
        return origin;
    }
}

/** 占位符显示为带名称的标签，让用户看清每个值被放在哪里。 */
export function VaultPlaceholderText({ text, entries }: { text: string; entries: VaultEntryLabel[] }) {
    const parts: ReactNode[] = [];
    let last = 0;
    for (const match of text.matchAll(PLACEHOLDER)) {
        const index = match.index ?? 0;
        if (index > last) parts.push(text.slice(last, index));
        const name = entries.find((entry) => entry.key === match[1])?.name ?? match[1];
        parts.push(
            <span
                key={`${index}-${match[1]}`}
                className="mx-0.5 inline-flex items-center gap-0.5 rounded border border-border bg-muted px-1 font-sans text-[11px] text-foreground"
                title={match[0]}
            >
                <KeyRound className="h-3 w-3" />
                {name}
            </span>,
        );
        last = index + match[0].length;
    }
    if (last < text.length) parts.push(text.slice(last));
    return <>{parts.map((part, index) => <Fragment key={index}>{part}</Fragment>)}</>;
}

/**
 * 助手要在一次工具调用里使用保险箱的值。每次使用都由用户在这里批准；
 * 卡片写明谁在用、用哪些值、交给谁，并展示调用原文（只含占位符，不含值）。
 */
export function VaultUseCard({ request, onRespond }: VaultUseCardProps) {
    const local = request.destination.kind === "local";
    const [busy, setBusy] = useState(false);
    // 本机去向能直接读到值，默认展开调用原文，让用户先看清再决定。
    const [expanded, setExpanded] = useState(local);
    const decide = async (decision: VaultUseDecision) => {
        setBusy(true);
        await onRespond(request.requestId, decision);
        setBusy(false);
    };
    const who = request.assistantName ? `助手「${request.assistantName}」` : "助手";
    const origin = request.destination.origin;

    return (
        <div className="my-2 rounded-xl border border-border bg-card p-4 shadow-sm" role="group" aria-label="保险箱使用确认">
            <div className="flex items-start gap-3">
                <KeyRound className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                <div className="min-w-0 flex-1 space-y-2">
                    <p className="text-sm font-medium">{who}要使用保险箱</p>
                    <div className="flex flex-wrap gap-1.5">
                        {request.entries.map((entry) => (
                            <span key={entry.id} className="rounded-md border border-border bg-muted px-2 py-0.5 text-xs" title={`<vault:${entry.key}>`}>
                                {entry.name}
                            </span>
                        ))}
                    </div>
                    <p className="break-all text-xs">
                        <span className="text-muted-foreground">用在：</span>
                        {request.destination.label}
                    </p>
                    {request.destination.insecure && (
                        <p className="flex items-center gap-1 text-xs text-muted-foreground">
                            <TriangleAlert className="h-3.5 w-3.5 shrink-0" />
                            这是未加密的 http 地址，值会以明文经过网络。
                        </p>
                    )}
                    {local && (
                        <p className="flex items-start gap-1 text-xs text-muted-foreground">
                            <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                            本机程序能读到这个值。输出里的原值会被遮住，但程序改写后再输出的内容遮不住，不能保证助手拿不到它。
                        </p>
                    )}
                    <button
                        type="button"
                        className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
                        aria-expanded={expanded}
                        onClick={() => setExpanded((value) => !value)}
                    >
                        {expanded ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
                        调用详情
                    </button>
                    {expanded && (
                        <div className="space-y-1">
                            <p className="text-xs text-muted-foreground">{request.tool}</p>
                            <pre className="max-h-60 overflow-auto whitespace-pre-wrap break-all rounded-md bg-muted/60 p-2 font-mono text-xs">
                                <VaultPlaceholderText text={request.preview} entries={request.entries} />
                            </pre>
                        </div>
                    )}
                </div>
            </div>
            <div className="mt-3 flex flex-wrap justify-end gap-2">
                <Button variant="ghost" size="sm" disabled={busy} onClick={() => void decide("deny")}>拒绝</Button>
                <Button variant={request.canAlways ? "outline" : "default"} size="sm" disabled={busy} onClick={() => void decide("once")}>允许这一次</Button>
                {request.canAlways && origin && (
                    <Button size="sm" disabled={busy} onClick={() => void decide("always")}>总是允许用于 {hostOf(origin)}</Button>
                )}
            </div>
        </div>
    );
}

/** 转成聊天列表的内联交互项：有消息 id 时挂在发起调用的消息后面，否则放在末尾。 */
export function vaultInlineItems(
    requests: VaultUseRequest[],
    onRespond: (requestId: string, decision: VaultUseDecision) => Promise<void>,
): InlineInteractionItem[] {
    return requests.map((request) => ({
        key: `vault-use-${request.requestId}`,
        callId: null,
        messageId: request.messageId,
        content: <VaultUseCard request={request} onRespond={onRespond} />,
    }));
}
