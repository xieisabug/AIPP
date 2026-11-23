import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import type { AppUpdateStatus } from "@/data/Update";
import { useFeatureAvailableOnPlatform } from "@/lib/mobileUnsupported";

export function useAppUpdater() {
    const available = useFeatureAvailableOnPlatform("app_updater");
    const [status, setStatus] = useState<AppUpdateStatus | null>(null);
    const [readError, setReadError] = useState("");

    useEffect(() => {
        if (!available) return;
        let active = true;
        let unlisten: (() => void) | undefined;
        const apply = (next: AppUpdateStatus) => {
            if (!active) return;
            setReadError("");
            setStatus(previous => !previous || next.revision > previous.revision ? next : previous);
        };
        const refresh = () => {
            void invoke<AppUpdateStatus>("get_app_update_status").then(apply).catch(error => {
                if (active) setReadError(`读取更新进度失败：${String(error)}`);
            });
        };
        // Subscribe first so a change during hydration cannot be missed.
        void listen<AppUpdateStatus>("app-update-status", event => apply(event.payload)).then(stop => {
            if (!active) { stop(); return; }
            unlisten = stop;
            refresh();
        }).catch(error => {
            if (active) setReadError(`订阅更新进度失败：${String(error)}`);
        });
        window.addEventListener("focus", refresh);
        return () => {
            active = false;
            unlisten?.();
            window.removeEventListener("focus", refresh);
        };
    }, [available]);

    return { available, status, readError };
}

export function checkAppUpdate(useProxy = false) {
    return invoke("check_app_update", { useProxy }).catch(error => toast.error(String(error)));
}

export function installAppUpdate(version: string) {
    return invoke("install_app_update", { version }).catch(error => toast.error(String(error)));
}
