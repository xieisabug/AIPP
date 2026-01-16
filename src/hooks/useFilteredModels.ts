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
 * 获取过滤后的模型列表
 * @param assistantType 助手类型 (4: ACP 助手, 其他: 普通助手)
 * @param shouldFetch 是否执行获取
 */
export const useFilteredModels = (assistantType: number | null, shouldFetch: boolean = true) => {
    const [models, setModels] = useState<ModelForSelect[]>([]);
    const [loading, setLoading] = useState(shouldFetch);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (!shouldFetch || assistantType === null) {
            setLoading(false);
            return;
        }

        setLoading(true);
        invoke<Array<ModelForSelect>>("get_filtered_models_for_select", { assistantType })
            .then((modelList) => {
                setModels(modelList);
                setError(null);
            })
            .catch((err) => {
                const errorMsg = "获取模型列表失败: " + getErrorMessage(err);
                setError(errorMsg);
                toast.error(errorMsg);
            })
            .finally(() => {
                setLoading(false);
            });
    }, [shouldFetch, assistantType]);

    return { models, loading, error };
};
