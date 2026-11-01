import { useDataRefresh } from "@/hooks/useDataRefresh";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";

export interface ModelForSelect {
    name: string;
    code: string;
    id: number;
    llm_provider_id: number;
}

/**
 * 获取过滤后的模型列表。
 * 订阅 llm_provider_changed，并在页面重新可见时刷新，避免移动端设置页 keep-alive 后目录过期。
 */
export const useFilteredModels = (assistantType: number | null, shouldFetch: boolean = true) => {
    const [models, setModels] = useState<ModelForSelect[]>([]);
    const [loading, setLoading] = useState(shouldFetch);
    const [error, setError] = useState<string | null>(null);

    const requestRefresh = useDataRefresh({
        events: ["llm_provider_changed"],
        identity: `${shouldFetch}:${assistantType}`,
        initial: true,
        refresh: async (isCurrent) => {
            if (!shouldFetch || assistantType === null) {
                setLoading(false);
                return;
            }

            setLoading(true);
            await invoke<Array<ModelForSelect>>("get_filtered_models_for_select", { assistantType })
                .then((modelList) => {
                    if (!isCurrent()) return;
                    setModels(modelList);
                    setError(null);
                })
                .catch((err) => {
                    if (!isCurrent()) return;
                    const errorMsg = "获取模型列表失败: " + getErrorMessage(err);
                    setError(errorMsg);
                    toast.error(errorMsg);
                })
                .finally(() => {
                    if (isCurrent()) setLoading(false);
                });
        },
    });

    // 页面激活（从其他设置页切回 / 从后台回前台）时合理刷新目录，不触发表单重置。
    useEffect(() => {
        if (!shouldFetch || assistantType === null) return;
        const refreshWhenVisible = () => {
            if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
            requestRefresh();
        };
        const onMenuActivated = (event: Event) => {
            const menu = (event as CustomEvent<string>).detail;
            if (menu && menu !== "assistant-config" && menu !== "feature-assistant-config") return;
            requestRefresh();
        };
        window.addEventListener("focus", refreshWhenVisible);
        document.addEventListener("visibilitychange", refreshWhenVisible);
        window.addEventListener("aipp:config-menu-activated", onMenuActivated as EventListener);
        return () => {
            window.removeEventListener("focus", refreshWhenVisible);
            document.removeEventListener("visibilitychange", refreshWhenVisible);
            window.removeEventListener("aipp:config-menu-activated", onMenuActivated as EventListener);
        };
    }, [shouldFetch, assistantType, requestRefresh]);

    return { models, loading, error, requestRefresh };
};
