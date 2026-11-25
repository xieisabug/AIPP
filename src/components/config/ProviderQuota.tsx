import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useDataRefresh } from "@/hooks/useDataRefresh";

interface QuotaWindow {
    group?: string;
    label: string;
    remainingPercent: number | null;
    remainingAmount?: number;
    resetsAt: string | null;
}

interface QuotaReading {
    windows: QuotaWindow[];
    checkedAt: number;
    expiresAt: number;
}

function WindowReading({ window }: { window: QuotaWindow }) {
    const percent = window.remainingPercent;
    let remaining = "未提供剩余额度";
    if (percent != null) remaining = `剩余 ${percent.toLocaleString(undefined, { maximumFractionDigits: 2 })}%`;
    else if (window.remainingAmount != null) remaining = `剩余 ${window.remainingAmount.toLocaleString()}`;

    return <div className="space-y-1">
        <div className="flex flex-wrap justify-between gap-x-3">
            <span>{window.label}</span>
            <span className="tabular-nums">{remaining}</span>
        </div>
        {percent != null && <div className="h-1 overflow-hidden rounded bg-muted">
            <div className="h-full bg-primary" style={{ width: `${percent}%` }} />
        </div>}
        <p className="text-xs text-muted-foreground">{window.resetsAt
            ? <>重置于 <time dateTime={window.resetsAt}>{new Date(window.resetsAt).toLocaleString()}</time></>
            : "服务未提供重置时间"}</p>
    </div>;
}

function QuotaWindows({ windows }: { windows: QuotaWindow[] }) {
    const groups = new Map<string | undefined, QuotaWindow[]>();
    for (const window of windows) {
        const existing = groups.get(window.group);
        if (existing) existing.push(window);
        else groups.set(window.group, [window]);
    }
    return Array.from(groups, ([name, items]) => {
        const rows = items.map((item, index) => <WindowReading key={`${item.label}-${index}`} window={item} />);
        return name ? <section key={name} aria-label={name} className="space-y-2">
            <h4 className="font-medium">{name}</h4>{rows}
        </section> : <div key="" className="space-y-2">{rows}</div>;
    });
}

export function ProviderQuota({ providerId, label = "订阅额度" }: { providerId: number; label?: string }) {
    const [reading, setReading] = useState<QuotaReading | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState("");
    const intent = useRef<"manual" | "expiry" | null>(null);
    const refresh = useDataRefresh({
        identity: providerId,
        events: [`cursor-account-changed-${providerId}`],
        onInvalidate: fromEvent => {
            if (fromEvent || !intent.current) setReading(null);
            setLoading(true);
            setError("");
        },
        onError: cause => { setLoading(false); setError(String(cause)); },
        refresh: async isCurrent => {
            const force = intent.current === "manual";
            intent.current = null;
            const result = await invoke<QuotaReading>("get_provider_quota", { providerId, force });
            if (isCurrent()) { setReading(result); setLoading(false); }
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
        const timer = setTimeout(() => { intent.current = "expiry"; refresh(); }, delay);
        return () => clearTimeout(timer);
    }, [refresh, reading, loading, error]);

    return <section aria-label={label} className="space-y-2 rounded-md border p-3 text-sm">
        <div className="flex items-center justify-between gap-2">
            <span className="text-xs text-muted-foreground" role="status">
                {reading ? `更新于 ${new Date(reading.checkedAt).toLocaleTimeString()}` :
                    loading ? "正在查询额度…" : "额度查询失败"}
            </span>
            <Button type="button" size="icon" variant="ghost" iconLabel={`刷新${label}`}
                disabled={loading} onClick={() => { intent.current = "manual"; refresh(); }}>
                <RefreshCw className="h-4 w-4" />
            </Button>
        </div>
        {error && <p role="alert" className="whitespace-pre-wrap break-words text-xs text-destructive">刷新额度失败：{error}</p>}
        {reading && <QuotaWindows windows={reading.windows} />}
    </section>;
}
