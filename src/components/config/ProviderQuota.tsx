import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useDataRefresh } from "@/hooks/useDataRefresh";

interface QuotaWindow {
    label: string;
    remainingPercent: number;
    resetsAt: string | null;
}

type Reading =
    | { status: "loading" }
    | { status: "error"; message: string }
    | { status: "ready"; windows: QuotaWindow[]; checkedAt: Date };

export function ProviderQuota({ providerId }: { providerId: number }) {
    const [reading, setReading] = useState<Reading>({ status: "loading" });
    const refresh = useDataRefresh({
        identity: providerId,
        events: [`cursor-account-changed-${providerId}`],
        onInvalidate: () => setReading({ status: "loading" }),
        onError: error => setReading({ status: "error", message: String(error) }),
        refresh: async isCurrent => {
            const windows = await invoke<QuotaWindow[]>("get_provider_quota", { providerId });
            if (isCurrent()) setReading({ status: "ready", windows, checkedAt: new Date() });
        },
    });
    useEffect(() => {
        // Let autosaved credential edits settle before contacting the provider.
        const timer = setTimeout(refresh, 700);
        return () => clearTimeout(timer);
    }, [refresh, providerId]);

    return <section aria-label="订阅额度" className="space-y-2 rounded-md border p-3 text-sm">
        <div className="flex items-center justify-between gap-2">
            <span className="text-xs text-muted-foreground" role="status">
                {reading.status === "ready" ? `更新于 ${reading.checkedAt.toLocaleTimeString()}` :
                    reading.status === "loading" ? "正在查询额度…" : "额度查询失败"}
            </span>
            <Button type="button" size="icon" variant="ghost" iconLabel="刷新订阅额度"
                disabled={reading.status === "loading"} onClick={refresh}>
                <RefreshCw className="h-4 w-4" />
            </Button>
        </div>
        {reading.status === "error" && <p role="alert" className="whitespace-pre-wrap break-words text-xs text-destructive">{reading.message}</p>}
        {reading.status === "ready" && reading.windows.map((window, index) =>
            <div key={`${window.label}-${index}`} className="space-y-1">
                <div className="flex flex-wrap justify-between gap-x-3">
                    <span>{window.label}</span>
                    <span className="tabular-nums">剩余 {window.remainingPercent.toLocaleString(undefined, { maximumFractionDigits: 2 })}%</span>
                </div>
                <div className="h-1 overflow-hidden rounded bg-muted">
                    <div className="h-full bg-primary" style={{ width: `${window.remainingPercent}%` }} />
                </div>
                <p className="text-xs text-muted-foreground">{window.resetsAt
                    ? <>重置于 <time dateTime={window.resetsAt}>{new Date(window.resetsAt).toLocaleString()}</time></>
                    : "服务未提供重置时间"}</p>
            </div>)}
    </section>;
}
