import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";

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

    useEffect(() => {
        if (!shouldFetch) {
            setLoading(false);
            return;
        }

        setLoading(true);
        invoke<Array<ModelForSelect>>("get_models_for_select")
            .then((modelList) => {
                setModels(modelList);
                setError(null);
            })
            .catch((err) => {
                const errorMsg = "获取模型列表失败: " + err;
                setError(errorMsg);
                toast.error(errorMsg);
            })
            .finally(() => {
                setLoading(false);
            });
    }, [shouldFetch]);

    return { models, loading, error };
};