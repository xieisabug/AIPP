import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import { cn } from "@/utils/utils";
import {
    classifyQuotaDisplay,
    QuotaDetailsList,
    QuotaRingsChart,
    type QuotaReading,
} from "./quotaVisuals";

export interface ProviderQuotaProps {
    providerId: number;
    label?: string;
    showInChat?: boolean;
    onShowInChatChange?: (enabled: boolean) => void;
}

export function ProviderQuota({
    providerId,
    label = "订阅额度",
    showInChat,
    onShowInChatChange,
}: ProviderQuotaProps) {
    const [reading, setReading] = useState<QuotaReading | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState("");
    const intent = useRef<"manual" | "expiry" | null>(null);
    const refresh = useDataRefresh({
        identity: providerId,
        events: [`cursor-account-changed-${providerId}`],
        onInvalidate: (fromEvent) => {
            if (fromEvent || !intent.current) setReading(null);
            setLoading(true);
            setError("");
        },
        onError: (cause) => {
            setLoading(false);
            setError(String(cause));
        },
        refresh: async (isCurrent) => {
            const force = intent.current === "manual";
            intent.current = null;
            const result = await invoke<QuotaReading>("get_provider_quota", { providerId, force });
            if (isCurrent()) {
                setReading(result);
                setLoading(false);
            }
        },
    });

    useEffect(() => {
        // Let autosaved credential edits settle before contacting the provider.
        const timer = setTimeout(refresh, 700);
        return () => clearTimeout(timer);
    }, [refresh, providerId]);

    useEffect(() => {
        if (loading || (!reading && !error)) return;
        const delay = error ? 15 * 60_000 : Math.max(0, reading!.expiresAt - Date.now());
        const timer = setTimeout(() => {
            intent.current = "expiry";
            refresh();
        }, delay);
        return () => clearTimeout(timer);
    }, [refresh, reading, loading, error]);

    const display = useMemo(
        () => (reading ? classifyQuotaDisplay(reading.windows) : null),
        [reading],
    );

    const handleManualRefresh = () => {
        if (loading) return;
        intent.current = "manual";
        refresh();
    };

    return (
        <section
            aria-label={label}
            className="overflow-hidden rounded-lg border border-border bg-card text-sm"
            data-aipp-slot="provider-quota-card"
        >
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border/70 bg-muted/30 px-3.5 py-2">
                <div className="flex min-w-0 items-center gap-2.5">
                    {display?.kind === "rings" && (
                        <button
                            type="button"
                            onClick={handleManualRefresh}
                            disabled={loading}
                            title={`点击刷新${label}`}
                            aria-label={`点击刷新${label}`}
                            className="inline-flex shrink-0 cursor-pointer items-center rounded-md transition-opacity hover:opacity-80 disabled:cursor-wait disabled:opacity-60"
                        >
                            <QuotaRingsChart clusters={display.clusters} size={24} />
                        </button>
                    )}
                    <span className="truncate text-xs text-muted-foreground" role="status">
                        {reading
                            ? `更新于 ${new Date(reading.checkedAt).toLocaleTimeString()}`
                            : loading
                                ? "正在查询额度…"
                                : "额度查询失败"}
                    </span>
                </div>
                <div className="flex shrink-0 items-center gap-2.5">
                    {onShowInChatChange && (
                        <label className="flex cursor-pointer items-center gap-1.5 text-xs text-muted-foreground select-none">
                            <span>聊天窗口展示</span>
                            <Switch
                                aria-label={`在聊天窗口展示${label}`}
                                checked={!!showInChat}
                                onCheckedChange={onShowInChatChange}
                            />
                        </label>
                    )}
                    <Button
                        type="button"
                        size="icon"
                        variant="ghost"
                        className="h-7 w-7 text-muted-foreground"
                        iconLabel={`刷新${label}`}
                        disabled={loading}
                        onClick={handleManualRefresh}
                    >
                        <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
                    </Button>
                </div>
            </div>
            {(error || reading) && (
                <div className="space-y-3 p-3.5">
                    {error && (
                        <p
                            role="alert"
                            className="whitespace-pre-wrap break-words rounded-md bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive"
                        >
                            刷新额度失败：{error}
                        </p>
                    )}
                    {reading && display && (
                        <QuotaDetailsList
                            windows={reading.windows}
                            colorByIndex={display.colorByIndex}
                        />
                    )}
                </div>
            )}
        </section>
    );
}
