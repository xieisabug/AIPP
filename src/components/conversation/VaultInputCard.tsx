import { useState } from "react";
import { KeyRound } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { PasswordInput } from "@/components/ui/password-input";
import type { VaultInputRequest } from "@/data/Vault";
import type { InlineInteractionItem } from "@/components/ConversationUI";

interface VaultInputCardProps {
    request: VaultInputRequest;
    onSubmit: (requestId: string, values: Record<string, string>, sync: boolean) => Promise<void>;
    onDecline: (requestId: string) => Promise<void>;
}

/**
 * 助手请用户填写它没有的值。填写的内容直接存进保险箱，不进入聊天；
 * 助手只拿到条目的名称和占位符。
 */
export function VaultInputCard({ request, onSubmit, onDecline }: VaultInputCardProps) {
    const [values, setValues] = useState<Record<string, string>>({});
    const [sync, setSync] = useState(true);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const who = request.assistantName ? `助手「${request.assistantName}」` : "助手";
    const complete = request.fields.every((field) => (values[field.key] ?? "").trim().length > 0);

    const submit = async () => {
        setBusy(true);
        setError(null);
        try {
            await onSubmit(request.requestId, values, sync);
            setValues({});
            toast.success("已保存到保险箱");
        } catch (cause) {
            // 卡片保持打开，用户可以修改后重试。
            setError(String(cause));
        } finally {
            setBusy(false);
        }
    };

    const decline = async () => {
        setBusy(true);
        try {
            await onDecline(request.requestId);
            setValues({});
        } catch (cause) {
            toast.error(`处理填写请求失败：${String(cause)}`);
        } finally {
            setBusy(false);
        }
    };

    return (
        <form
            className="my-2 rounded-xl border border-border bg-card p-4 shadow-sm"
            aria-label="填写保险箱"
            onSubmit={(event) => {
                event.preventDefault();
                if (complete && !busy) void submit();
            }}
        >
            <div className="flex items-start gap-3">
                <KeyRound className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                <div className="min-w-0 flex-1 space-y-3">
                    <div className="space-y-1">
                        <p className="text-sm font-medium">{who}请你填写</p>
                        <p className="text-xs text-muted-foreground">用途：{request.purpose}</p>
                    </div>
                    {request.fields.map((field) => (
                        <div key={field.key} className="space-y-1">
                            <div className="flex items-baseline justify-between gap-2">
                                <label htmlFor={`vault-input-${request.requestId}-${field.key}`} className="text-sm">{field.name}</label>
                                <span className="font-mono text-[11px] text-muted-foreground">{field.key}</span>
                            </div>
                            <PasswordInput
                                id={`vault-input-${request.requestId}-${field.key}`}
                                autoComplete="off"
                                disabled={busy}
                                value={values[field.key] ?? ""}
                                onChange={(event) => {
                                    const value = event.target.value;
                                    setValues((current) => ({ ...current, [field.key]: value }));
                                }}
                            />
                            {field.description && <p className="text-xs text-muted-foreground">{field.description}</p>}
                        </div>
                    ))}
                    <label className="flex items-center gap-2 text-xs">
                        <Checkbox checked={sync} disabled={busy} onCheckedChange={(checked) => setSync(checked === true)} />
                        同步到我的其他设备
                    </label>
                    <p className="text-xs text-muted-foreground">填写的内容只存进保险箱，不会出现在聊天里；助手只拿到占位符。</p>
                    {error && <p className="text-xs text-destructive" role="alert">{error}</p>}
                </div>
            </div>
            <div className="mt-3 flex justify-end gap-2">
                <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void decline()}>拒绝</Button>
                <Button type="submit" size="sm" disabled={busy || !complete}>保存并继续</Button>
            </div>
        </form>
    );
}

export function vaultInputInlineItems(
    requests: VaultInputRequest[],
    onSubmit: VaultInputCardProps["onSubmit"],
    onDecline: VaultInputCardProps["onDecline"],
): InlineInteractionItem[] {
    return requests.map((request) => ({
        key: `vault-input-${request.requestId}`,
        callId: null,
        messageId: request.messageId,
        content: <VaultInputCard request={request} onSubmit={onSubmit} onDecline={onDecline} />,
    }));
}
