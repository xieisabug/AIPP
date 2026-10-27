import { useDataRefresh } from "@/hooks/useDataRefresh";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";

export interface ModelForSelect {
    name: string;
    code: string;
    id: number;
    llm_provider_id: number;
}

export const useModels = (shouldFetch: boolean = true) => {
    const [models, setModels] = useState<ModelForSelect[]>([]);
    const [loading, setLoading] = useState(shouldFetch);
    const [error, setError] = useState<string | null>(null);

    useDataRefresh({
        events: ["llm_provider_changed"],
        identity: String(shouldFetch),
        initial: true,
        refresh: async (isCurrent) => {
        if (!shouldFetch) {
            setLoading(false);
            return;
        }

        setLoading(true);
        await invoke<Array<ModelForSelect>>("get_models_for_select")
            .then((modelList) => {
                const uniqueModels = new Map<string, ModelForSelect>();
                for (const model of modelList) {
                    const key = `${model.code}%%${model.llm_provider_id}`;
                    if (!uniqueModels.has(key)) {
                        uniqueModels.set(key, model);
                    }
                }
                if (!isCurrent()) return;
                setModels(Array.from(uniqueModels.values()));
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

    return { models, loading, error };
};
