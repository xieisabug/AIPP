import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useArtifactEvents, type ArtifactData, type EnvironmentCheckData } from "@/hooks/useArtifactEvents";
import { toPreviewType, type ArtifactPreviewType } from "@/components/artifact/artifactRuntime";

export interface ArtifactPreviewContent {
    type: ArtifactPreviewType;
    code: string;
    /** 每次收到新内容递增，用于让查看器重建预览。 */
    key: number;
    data: ArtifactData;
}

export interface UseArtifactPreviewSessionOptions {
    /** 'preview'：artifact_preview 与侧栏；'artifact'：收藏窗口。 */
    windowType: "preview" | "artifact";
    /** preview 类窗口的 ready 信号后缀：'artifact_preview' | 'sidebar'。 */
    readyWindow?: string;
    /** 安装环境后重试时交给后端的来源窗口。 */
    sourceWindow?: string;
    /** 结束当前 React/Vue 预览服务器（各窗口的实例 ID 不同）。 */
    closeRuntime: (kind: "react" | "vue", data: ArtifactData | null) => Promise<unknown>;
    /** 环境安装完成后的重试；默认 retry_preview_after_install。 */
    retryAfterInstall?: (lang: string, inputStr: string) => Promise<unknown>;
    /** 收到新内容后的宿主处理（缓存、绑定配置等）。 */
    onData?: (data: ArtifactData) => void;
    /** 当前会话的 Artifact 被重新发布时，重新加载本窗口缓存的预览。 */
    followManifestUpdates?: boolean;
}

/**
 * 预览窗口的会话状态：接收后端事件、记录构建日志、处理环境安装与预览服务器的结束。
 * 渲染交给 ArtifactViewer；三个预览入口（预览窗口、侧栏、收藏窗口）共用。
 */
export function useArtifactPreviewSession(options: UseArtifactPreviewSessionOptions) {
    const { windowType, readyWindow, sourceWindow, followManifestUpdates = false } = options;
    const optionsRef = useRef(options);
    optionsRef.current = options;

    const [content, setContent] = useState<ArtifactPreviewContent | null>(null);
    const [previewUrl, setPreviewUrl] = useState<string | null>(null);
    const contentRef = useRef<ArtifactPreviewContent | null>(null);
    contentRef.current = content;
    const keyRef = useRef(0);

    const [environment, setEnvironment] = useState<{ tool: string; message: string; open: boolean }>({
        tool: "",
        message: "",
        open: false,
    });
    const pendingInstallRef = useRef<{ lang: string; inputStr: string; installing: boolean }>({
        lang: "",
        inputStr: "",
        installing: false,
    });

    const stopRuntime = useCallback(async (current: ArtifactPreviewContent | null) => {
        if (!current || (current.type !== "react" && current.type !== "vue")) return;
        try {
            await optionsRef.current.closeRuntime(current.type, current.data);
        } catch (error) {
            console.warn("[ArtifactPreviewSession] failed to stop preview runtime", error);
        }
    }, []);

    const handleArtifactData = useCallback((data: ArtifactData) => {
        const type = toPreviewType(data.type);
        if (!type || typeof data.original_code !== "string") return;
        keyRef.current += 1;
        setPreviewUrl(null);
        setContent({ type, code: data.original_code, key: keyRef.current, data });
        optionsRef.current.onData?.(data);
    }, []);

    const handleRedirect = useCallback((url: string) => {
        setPreviewUrl(url);
    }, []);

    const handleEnvironmentCheck = useCallback((data: EnvironmentCheckData) => {
        pendingInstallRef.current = { lang: data.lang, inputStr: data.input_str, installing: false };
        setEnvironment({ tool: data.tool, message: data.message, open: true });
    }, []);

    const handleInstallStarted = useCallback((data: { tool: string; lang: string; input_str: string }) => {
        pendingInstallRef.current = { lang: data.lang, inputStr: data.input_str, installing: true };
        setEnvironment((prev) => ({ ...prev, open: false }));
    }, []);

    const handleReset = useCallback(() => {
        void stopRuntime(contentRef.current);
        setContent(null);
        setPreviewUrl(null);
    }, [stopRuntime]);

    // 安装完成回调需要 addLog，而 addLog 来自下面的 useArtifactEvents；经 ref 转一次。
    const finishInstallRef = useRef<(tool: string, success: boolean) => void>(() => {});
    const handleBunFinished = useCallback((success: boolean) => finishInstallRef.current("Bun", success), []);
    const handleUvFinished = useCallback((success: boolean) => finishInstallRef.current("uv", success), []);

    const events = useArtifactEvents({
        windowType,
        readyWindow,
        onArtifactData: handleArtifactData,
        onRedirect: handleRedirect,
        onEnvironmentCheck: handleEnvironmentCheck,
        onEnvironmentInstallStarted: handleInstallStarted,
        onBunInstallFinished: handleBunFinished,
        onUvInstallFinished: handleUvFinished,
        onReset: handleReset,
    });
    const { addLog } = events;

    finishInstallRef.current = (tool: string, success: boolean) => {
        const pending = pendingInstallRef.current;
        if (!success) {
            addLog("error", `${tool} 安装失败`);
            pending.installing = false;
            return;
        }
        if (!pending.installing) return;
        addLog("success", `${tool} 安装成功，正在重新启动预览…`);
        const retry =
            optionsRef.current.retryAfterInstall ??
            ((lang: string, inputStr: string) =>
                invoke("retry_preview_after_install", { lang, inputStr, sourceWindow }));
        retry(pending.lang, pending.inputStr)
            .catch((error) => addLog("error", `重新启动预览失败: ${error}`))
            .finally(() => {
                pending.installing = false;
            });
    };

    const respondEnvironment = useCallback(
        async (confirmed: boolean) => {
            const pending = pendingInstallRef.current;
            try {
                await invoke("confirm_environment_install", {
                    tool: environment.tool,
                    confirmed,
                    lang: pending.lang,
                    inputStr: pending.inputStr,
                });
                if (!confirmed) setEnvironment((prev) => ({ ...prev, open: false }));
            } catch (error) {
                addLog("error", `${confirmed ? "确认" : "取消"}安装失败: ${error}`);
            }
        },
        [environment.tool, addLog],
    );

    // 同一会话的 Artifact 被重新发布：让后端按本窗口缓存重新推送
    const conversationId = typeof content?.data.conversation_id === "number" ? content.data.conversation_id : undefined;
    useEffect(() => {
        if (!followManifestUpdates || conversationId === undefined) return;
        const subscription = listen<{ conversation_id: number }>("artifact-manifest-updated", (event) => {
            if (event.payload.conversation_id !== conversationId) return;
            invoke("restore_artifact_preview", { sourceWindow: readyWindow ?? sourceWindow }).catch((error) =>
                console.warn("[ArtifactPreviewSession] refresh after manifest update failed", error),
            );
        });
        return () => {
            void subscription.then((unlisten) => unlisten());
        };
    }, [followManifestUpdates, conversationId, readyWindow, sourceWindow]);

    // 关闭窗口或卸载时结束预览服务器
    useEffect(() => {
        let unlisten: (() => void) | null = null;
        let disposed = false;
        getCurrentWebviewWindow()
            .onCloseRequested(() => stopRuntime(contentRef.current))
            .then((fn) => {
                if (disposed) fn();
                else unlisten = fn;
            })
            .catch(() => {});
        return () => {
            disposed = true;
            unlisten?.();
            void stopRuntime(contentRef.current);
        };
    }, [stopRuntime]);

    return {
        content,
        previewUrl,
        conversationId,
        logs: events.logs,
        addLog,
        hasReceivedData: events.hasReceivedData,
        /** 本地编辑（Draw.io）后更新内容但不重建预览。 */
        updateCode: useCallback((code: string) => {
            setContent((prev) => (prev ? { ...prev, code } : prev));
        }, []),
        environmentDialog: {
            tool: environment.tool,
            message: environment.message,
            isOpen: environment.open,
            onConfirm: () => void respondEnvironment(true),
            onCancel: () => void respondEnvironment(false),
        },
    };
}
