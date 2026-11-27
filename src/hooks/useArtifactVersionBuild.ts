import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface BuildView {
    url: string;
    lease: string;
}

/** 交给 ArtifactViewer 的多文件版本构建状态。 */
export interface ArtifactProjectBuild {
    /** 构建输出的预览地址；没有构建过时为 null。 */
    url: string | null;
    building: boolean;
    error: string;
    build: (outputDir: string) => Promise<void>;
}

/**
 * 会话多文件 Artifact 某个版本的 Bun 构建：有构建结果时打开它的预览服务（离开时释放），
 * 没有时提供构建入口。与比选工程共用构建与预览服务（`close_choice_build` 释放）。
 */
export function useArtifactVersionBuild(options: {
    conversationId: number | undefined;
    artifactKey: string | null | undefined;
    version: number | undefined;
    enabled: boolean;
}): ArtifactProjectBuild | undefined {
    const { conversationId, artifactKey, version, enabled } = options;
    const active = enabled && conversationId !== undefined && !!artifactKey && version !== undefined;
    const [view, setView] = useState<BuildView | null>(null);
    const [building, setBuilding] = useState(false);
    const [error, setError] = useState("");
    const [generation, setGeneration] = useState(0);

    useEffect(() => {
        setView(null);
        setError("");
        if (!active) return;
        let alive = true;
        const opening = invoke<BuildView | null>("open_artifact_version_build", { conversationId, artifactKey, version });
        opening
            .then((result) => {
                if (alive) setView(result);
            })
            .catch((e) => {
                if (alive) setError(String(e));
            });
        return () => {
            alive = false;
            void opening
                .then((result) => result && invoke("close_choice_build", { lease: result.lease }))
                .catch(() => {});
        };
    }, [active, conversationId, artifactKey, version, generation]);

    const build = useCallback(
        async (outputDir: string) => {
            if (!active) return;
            setBuilding(true);
            setError("");
            try {
                await invoke("build_artifact_version", { conversationId, artifactKey, version, outputDir });
                setGeneration((value) => value + 1);
            } catch (e) {
                setError(String(e));
            } finally {
                setBuilding(false);
            }
        },
        [active, conversationId, artifactKey, version],
    );

    if (!active) return undefined;
    return { url: view?.url ?? null, building, error, build };
}
