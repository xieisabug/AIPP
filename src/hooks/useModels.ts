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
    request_mode?: string;
    is_decision?: boolean;
}

export interface UseModelsOptions {
    /** 为 true 时额外包含决策类模型（Jev / OpenAI Decisions / Gemini Decisions）。 */
    includeDecision?: boolean;
}

export const useModels = (shouldFetch: boolean = true, options?: UseModelsOptions) => {
    const includeDecision = Boolean(options?.includeDecision);
    const [models, setModels] = useState<ModelForSelect[]>([]);
    const [loading, setLoading] = useState(shouldFetch);
    const [error, setError] = useState<string | null>(null);

    useDataRefresh({
        events: ["llm_provider_changed"],
        identity: `${String(shouldFetch)}:${String(includeDecision)}`,
        initial: true,
        refresh: async (isCurrent) => {
        if (!shouldFetch) {
            setLoading(false);
            return;
        }

        setLoading(true);
        const request = includeDecision
            ? invoke<Array<ModelForSelect>>("get_models_for_select", { includeDecision: true })
            : invoke<Array<ModelForSelect>>("get_models_for_select");
        await request
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
