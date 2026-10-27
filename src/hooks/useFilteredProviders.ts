import { useDataRefresh } from "@/hooks/useDataRefresh";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";

export interface ProviderForSelect {
    id: number;
    name: string;
    api_type: string;
    description: string;
    is_official: boolean;
    is_enabled: boolean;
}

/**
 * 获取过滤后的提供商列表
 * @param assistantType 助手类型 (4: ACP 助手, 其他: 普通助手)
 * @param shouldFetch 是否执行获取
 */
export const useFilteredProviders = (assistantType: number | null, shouldFetch: boolean = true) => {
    const [providers, setProviders] = useState<ProviderForSelect[]>([]);
    const [loading, setLoading] = useState(shouldFetch);
    const [error, setError] = useState<string | null>(null);

    useDataRefresh({
        events: ["llm_provider_changed"],
        identity: `${shouldFetch}:${assistantType}`,
        initial: true,
        refresh: async (isCurrent) => {
        if (!shouldFetch || assistantType === null) {
            setLoading(false);
            return;
        }

        setLoading(true);
        await invoke<Array<ProviderForSelect>>("get_filtered_providers", { assistantType })
            .then((providerList) => {
                if (!isCurrent()) return;
                setProviders(providerList);
                setError(null);
            })
            .catch((err) => {
                if (!isCurrent()) return;
                const errorMsg = "获取提供商列表失败: " + getErrorMessage(err);
                setError(errorMsg);
                toast.error(errorMsg);
            })
            .finally(() => {
                if (isCurrent()) setLoading(false);
            });
        },
    });

    return { providers, loading, error };
};
