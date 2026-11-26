import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { PluginInstallProgressPayload } from "@/services/officePreviewPlugin";
import { invoke } from "@tauri-apps/api/core";
import { AlertCircle, CheckCircle2, Circle, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import { toast } from "sonner";

export const setupNames = {
    skills: "AIPP Skills 和 find-skills",
    office: "Office 插件",
    bun: "组件预览环境（bun）",
    uv: "Python 环境（uv）",
};
export type SetupId = keyof typeof setupNames;
export type SetupItem = {
    id: SetupId;
    state: "pending" | "running" | "done" | "skipped" | "failed";
    message: string;
    acknowledged?: boolean;
};
const states = { pending: "等待中", running: "正在准备", done: "已完成", skipped: "已就绪", failed: "未完成" };

export function useSetupItems() {
    const [items, setItems] = useState<SetupItem[]>([]);
    useDataRefresh({
        events: ["onboarding-setup-progress"],
        initial: true,
        refresh: async (current) => {
            const result = await invoke<SetupItem[]>("get_onboarding_setup_status");
            if (current()) setItems(Array.isArray(result) ? result : []);
        },
    });
    const officeState = items.find((item) => item.id === "office")?.state;
    const officeStateRef = useRef(officeState);
    const [officeProgress, setOfficeProgress] = useState("");
    useEffect(() => {
        officeStateRef.current = officeState;
        if (officeState && officeState !== "running") setOfficeProgress("");
    }, [officeState]);
    useEffect(() => {
        let active = true;
        const sub = listen<PluginInstallProgressPayload>("plugin_install_progress", ({ payload }) => {
            if (!active || payload.pluginCode !== "office-preview-plugin") return;
            const state = officeStateRef.current;
            if (state === "done" || state === "skipped" || state === "failed") return;
            const value =
                payload.percent ??
                (payload.totalBytes ? (100 * payload.downloadedBytes) / payload.totalBytes : null);
            if (payload.stage === "downloading" && value != null)
                setOfficeProgress(`正在下载 ${Math.round(value)}%`);
            else setOfficeProgress(payload.message);
        });
        return () => {
            active = false;
            void sub.then((stop) => stop());
        };
    }, []);
    return items.map((item) =>
        item.id === "office" && item.state === "running" && officeProgress
            ? { ...item, message: officeProgress }
            : item
    );
}
export function useOnboardingEntry(desktop: boolean) {
    const [canOpen, setCanOpen] = useState(false);
    useDataRefresh({
        events: ["feature_config_changed", "onboarding-state-changed"],
        initial: true,
        paused: !desktop,
        refresh: async (current) => {
            const result = await invoke<{ canOpen?: boolean }>("get_onboarding_state");
            if (current()) setCanOpen(!!result?.canOpen);
        },
    });
    return canOpen;
}
export const openOnboarding = () => invoke("open_onboarding_window").catch((e) => toast.error(String(e)));
function SetupStateIcon({ state }: { state: SetupItem["state"] }) {
    if (state === "done" || state === "skipped") {
        return <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-600 dark:text-emerald-400" />;
    }
    if (state === "running") {
        return <Loader2 className="h-4 w-4 shrink-0 animate-spin text-foreground" />;
    }
    if (state === "failed") {
        return <AlertCircle className="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />;
    }
    return <Circle className="h-3.5 w-3.5 shrink-0 text-muted-foreground/60" />;
}
export function SetupList({ items }: { items: SetupItem[] }) {
    return (
        <ul className="divide-y divide-border/60 text-sm">
            {items.map((item) => (
                <li key={item.id} className="flex items-center justify-between gap-3 py-2 first:pt-0 last:pb-0">
                    <div className="flex min-w-0 items-center gap-2.5">
                        <SetupStateIcon state={item.state} />
                        <span className="truncate font-medium text-foreground">{setupNames[item.id]}</span>
                    </div>
                    <div className="flex shrink-0 items-center gap-2">
                        <span
                            className={`max-w-[220px] truncate text-right text-xs ${
                                item.state === "failed"
                                    ? "text-amber-700 dark:text-amber-400"
                                    : item.state === "running"
                                      ? "text-foreground"
                                      : "text-muted-foreground"
                            }`}
                            title={item.message || states[item.state]}
                        >
                            {states[item.state]}
                            {item.message && item.message !== states[item.state] ? ` · ${item.message}` : ""}
                        </span>
                        {item.state === "failed" && (
                            <Button
                                size="sm"
                                variant="outline"
                                className="h-7 px-2.5 text-xs"
                                onClick={() =>
                                    void invoke("retry_onboarding_setup", { id: item.id }).catch((e) =>
                                        toast.error(String(e))
                                    )
                                }
                            >
                                重试
                            </Button>
                        )}
                    </div>
                </li>
            ))}
        </ul>
    );
}
export function OnboardingSetupChip() {
    const items = useSetupItems();
    const [showCompleted, setShowCompleted] = useState(false);
    const sawActiveRef = useRef(false);
    const finished = items.filter((i) => i.state === "done" || i.state === "skipped").length;
    const failed = items.filter((i) => i.state === "failed").length;
    const active = items.some((i) => i.state === "pending" || i.state === "running");
    const complete = items.length > 0 && finished === items.length;
    useEffect(() => {
        if (active) {
            sawActiveRef.current = true;
            setShowCompleted(false);
            return;
        }
        if (complete && sawActiveRef.current) {
            sawActiveRef.current = false;
            setShowCompleted(true);
            const timer = setTimeout(() => setShowCompleted(false), 3000);
            return () => clearTimeout(timer);
        }
    }, [active, complete]);
    const hasUnseenFailure = items.some((item) => item.state === "failed" && !item.acknowledged);
    if (!items.length || (!active && !hasUnseenFailure && !(complete && showCompleted))) return null;
    return (
        <Popover>
            <PopoverTrigger asChild>
                <Button variant="outline" size="sm" className="h-7 text-xs">
                    {hasUnseenFailure
                        ? `${failed} 项未完成`
                        : complete
                          ? "准备好了"
                          : `正在准备 ${finished}/${items.length}`}
                </Button>
            </PopoverTrigger>
            <PopoverContent align="end" className="w-80">
                <SetupList items={items} />
                {!active && hasUnseenFailure && (
                    <div className="mt-3 flex justify-end border-t pt-3">
                        <Button size="sm" onClick={() =>
                            void invoke("acknowledge_onboarding_setup").catch((e) => toast.error(String(e)))
                        }>
                            我知道了
                        </Button>
                    </div>
                )}
            </PopoverContent>
        </Popover>
    );
}
