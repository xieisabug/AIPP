import { useEffect, useState } from "react";
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

    useEffect(() => {
        if (!shouldFetch || assistantType === null) {
            setLoading(false);
            return;
        }

        setLoading(true);
        invoke<Array<ProviderForSelect>>("get_filtered_providers", { assistantType })
            .then((providerList) => {
                setProviders(providerList);
                setError(null);
            })
            .catch((err) => {
                const errorMsg = "获取提供商列表失败: " + getErrorMessage(err);
                setError(errorMsg);
                toast.error(errorMsg);
            })
            .finally(() => {
                setLoading(false);
            });
    }, [shouldFetch, assistantType]);

    return { providers, loading, error };
};
