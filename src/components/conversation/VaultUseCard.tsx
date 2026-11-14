import { useState } from "react";
import { ChevronDown, ChevronRight, KeyRound, TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { VaultUseDecision, VaultUseRequest } from "@/data/Vault";
import type { InlineInteractionItem } from "@/components/ConversationUI";

interface VaultUseCardProps {
    request: VaultUseRequest;
    onRespond: (requestId: string, decision: VaultUseDecision) => Promise<void>;
}

function hostOf(origin: string): string {
    try {
        return new URL(origin).host;
    } catch {
        return origin;
    }
}

/** 助手第一次用某条凭据访问某个网站时的确认卡。只展示请求信息，从不包含凭据值。 */
export function VaultUseCard({ request, onRespond }: VaultUseCardProps) {
    const [busy, setBusy] = useState(false);
    const [expanded, setExpanded] = useState(false);
    const decide = async (decision: VaultUseDecision) => {
        setBusy(true);
        await onRespond(request.requestId, decision);
        setBusy(false);
    };
    const who = request.assistantName ?? "助手";

    return (
        <div className="my-2 rounded-xl border border-border bg-card p-4 shadow-sm" role="group" aria-label="使用凭据确认">
            <div className="flex items-start gap-3">
                <KeyRound className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                <div className="min-w-0 flex-1 space-y-2">
                    <p className="text-sm">
                        <span className="font-medium">{who}</span> 想用「<span className="font-medium">{request.credentialName}</span>」访问一个新网站
                    </p>
                    <p className="break-all font-mono text-xs">
                        <span className="mr-2 font-semibold">{request.method}</span>
                        {request.url}
                    </p>
                    {request.insecure && (
                        <p className="flex items-center gap-1 text-xs text-muted-foreground">
                            <TriangleAlert className="h-3.5 w-3.5" />
                            这是未加密的 http 地址，凭据会以明文经过网络。
                        </p>
                    )}
                    <button
                        type="button"
                        className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
                        aria-expanded={expanded}
                        onClick={() => setExpanded((value) => !value)}
                    >
                        {expanded ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
                        请求详情
                    </button>
                    {expanded && (
                        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs text-muted-foreground">
                            <dt>凭据引用名</dt>
                            <dd className="font-mono">{request.credentialHandle}</dd>
                            <dt>附加请求头</dt>
                            <dd className="font-mono">{request.headerNames.length ? request.headerNames.join(", ") : "无"}</dd>
                            <dt>请求体</dt>
                            <dd>{request.hasBody ? "有" : "无"}</dd>
                            <dt>说明</dt>
                            <dd>凭据值由 AIPP 在发送时注入，助手看不到。</dd>
                        </dl>
                    )}
                </div>
            </div>
            <div className="mt-3 flex flex-wrap justify-end gap-2">
                <Button variant="ghost" size="sm" disabled={busy} onClick={() => void decide("deny")}>拒绝</Button>
                <Button variant="outline" size="sm" disabled={busy} onClick={() => void decide("once")}>允许一次</Button>
                <Button size="sm" disabled={busy} onClick={() => void decide("always")}>总是允许 {hostOf(request.origin)}</Button>
            </div>
        </div>
    );
}

/** 转成聊天列表的内联交互项（挂在消息流末尾）。 */
export function vaultInlineItems(
    requests: VaultUseRequest[],
    onRespond: (requestId: string, decision: VaultUseDecision) => Promise<void>,
): InlineInteractionItem[] {
    return requests.map((request) => ({
        key: `vault-use-${request.requestId}`,
        callId: null,
        messageId: null,
        content: <VaultUseCard request={request} onRespond={onRespond} />,
    }));
}
