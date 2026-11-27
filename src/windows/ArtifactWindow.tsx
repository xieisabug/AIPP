import { CollectedChoicePreview, archivePrefix } from "@/components/choices/ChoicePreview";
import { useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import "../styles/ArtifactPreviewWIndow.css";
import EnvironmentInstallDialog from "../components/EnvironmentInstallDialog";
import ArtifactViewer from "@/components/artifact/ArtifactViewer";
import { useTheme } from "../hooks/useTheme";
import { useArtifactPreviewSession } from "@/hooks/useArtifactPreviewSession";
import type { ArtifactData } from "../hooks/useArtifactEvents";
import { formatIconDisplay } from "@/utils/emojiUtils";
import { requestArtifactFixOpening } from "@/utils/artifactFixRequest";

/** 结束收藏项的 React/Vue 预览服务器（后端按收藏 ID 登记）。 */
function stopCollectionRuntime(kind: "react" | "vue", artifactId: number | undefined) {
    if (typeof artifactId !== "number") return Promise.resolve();
    return invoke(kind === "vue" ? "close_vue_artifact" : "close_react_artifact", { artifactId });
}

/** 「artifact」窗口：打开收藏的 Artifact。同一窗口复用，打开另一项时结束上一项的预览服务。 */
export default function ArtifactWindow() {
    useTheme("artifact");

    const runningRef = useRef<{ id: number; type: "react" | "vue" } | null>(null);
    const lastIdRef = useRef<number | null>(null);
    const handleData = useCallback((data: ArtifactData) => {
        const previous = runningRef.current;
        if (previous && previous.id !== data.id) {
            void stopCollectionRuntime(previous.type, previous.id).catch(() => {});
        }
        runningRef.current =
            typeof data.id === "number" && (data.type === "react" || data.type === "vue")
                ? { id: data.id, type: data.type }
                : null;
        lastIdRef.current = typeof data.id === "number" ? data.id : null;
    }, []);

    const session = useArtifactPreviewSession({
        windowType: "artifact",
        closeRuntime: (kind, data) => stopCollectionRuntime(kind, data?.id),
        // 安装好环境后按收藏 ID 重新打开，沿用收藏里的代码与绑定
        retryAfterInstall: () =>
            lastIdRef.current !== null
                ? invoke("open_artifact_window", { artifactId: lastIdRef.current })
                : Promise.resolve(),
        onData: handleData,
    });
    const { content } = session;
    const info = content?.data;

    const loadingHeader = info ? (
        <div className="flex flex-col items-center gap-3">
            {info.icon && (
                <div className="flex h-16 w-16 items-center justify-center rounded-2xl border border-border bg-muted text-3xl">
                    {(() => {
                        const icon = formatIconDisplay(info.icon);
                        return icon.isImage ? <img src={icon.display} alt="" className="h-12 w-12 object-cover" /> : icon.display;
                    })()}
                </div>
            )}
            <div className="text-center">
                <h1 className="text-xl font-semibold text-foreground">{info.name}</h1>
                {info.description && <p className="mt-1 text-sm text-muted-foreground">{info.description}</p>}
            </div>
        </div>
    ) : null;

    // 从会话收藏的小工具：报错与「选择元素修改」交回原会话继续改（会话没打开时先打开）
    const sourceConversationId = typeof info?.source_conversation_id === "number" ? info.source_conversation_id : null;

    const archivedChoiceId =
        content?.type === "html" && content.code.startsWith(archivePrefix) && typeof info?.id === "number" ? info.id : null;

    return (
        <div className="flex h-screen bg-background" data-aipp-window="artifact" data-aipp-slot="window-root">
            <div className="flex flex-1 flex-col min-h-0" data-aipp-slot="artifact-main">
                <ArtifactViewer
                    type={content?.type ?? null}
                    code={archivedChoiceId !== null ? "" : content?.code ?? ""}
                    previewUrl={session.previewUrl}
                    contentKey={content?.key ?? 0}
                    title={info?.name}
                    buildLogs={session.logs}
                    bridgeConfig={info ? { db_id: info.db_id, assistant_id: info.assistant_id } : undefined}
                    loadingHeader={loadingHeader}
                    previewOverride={
                        archivedChoiceId !== null ? (
                            <CollectedChoicePreview key={archivedChoiceId} collectionId={archivedChoiceId} />
                        ) : undefined
                    }
                    onCodeChange={session.updateCode}
                    onFixWithAI={
                        sourceConversationId !== null
                            ? (text, attachments) => requestArtifactFixOpening(sourceConversationId, text, attachments)
                            : undefined
                    }
                    currentVersion={info?.source_version}
                />
            </div>
            <EnvironmentInstallDialog {...session.environmentDialog} />
        </div>
    );
}
