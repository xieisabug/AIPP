import { SavedChoicePreview, readPreviewReference } from "@/components/choices/ChoicePreview";
import { useArtifactVersionBuild } from "@/hooks/useArtifactVersionBuild";
import { invoke } from "@tauri-apps/api/core";
import EnvironmentInstallDialog from "@/components/EnvironmentInstallDialog";
import ArtifactViewer from "@/components/artifact/ArtifactViewer";
import { previewInstanceId } from "@/components/artifact/artifactRuntime";
import { useArtifactPreviewSession } from "@/hooks/useArtifactPreviewSession";
import { requestArtifactFix } from "@/utils/artifactFixRequest";
import { elementCapture } from "@/components/artifact/elementCapture";
import { Button } from "@/components/ui/button";
import { X } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/utils/utils";

interface EmbeddedArtifactPreviewProps {
    className?: string;
    /** 所在窗口标签：侧栏窗口 `sidebar`，聊天窗口分栏 `chat_ui`。后端按它投递预览、区分 React/Vue 预览实例。 */
    windowLabel?: string;
    /** 在独立预览窗口打开当前内容。 */
    onOpenInWindow?: () => void;
    /** 关闭面板（聊天窗口分栏）。 */
    onClose?: () => void;
    /** 工具栏最右侧的额外内容（聊天窗口分栏放 Windows 窗口按钮）。 */
    trailing?: ReactNode;
    /** 见 useArtifactPreviewSession 的同名选项；聊天窗口分栏传 false。 */
    stopOnWindowClose?: boolean;
}

/** 嵌在窗口里的 Artifact 预览（侧栏窗口、聊天窗口分栏）；使用自己的 React/Vue 预览实例，与预览窗口互不影响。 */
export default function EmbeddedArtifactPreview({
    className,
    windowLabel = "sidebar",
    onOpenInWindow,
    onClose,
    trailing,
    stopOnWindowClose,
}: EmbeddedArtifactPreviewProps) {
    const session = useArtifactPreviewSession({
        windowType: "preview",
        readyWindow: windowLabel,
        sourceWindow: windowLabel,
        closeRuntime: (kind) =>
            invoke(kind === "vue" ? "close_vue_preview" : "close_react_preview", {
                previewId: previewInstanceId(kind, windowLabel),
            }),
        followManifestUpdates: true,
        stopOnWindowClose,
    });
    const { content, conversationId } = session;
    const choiceReference = content && content.type === "html" ? readPreviewReference(content.code) : null;
    const bridgeConfig = content ? { db_id: content.data.db_id, assistant_id: content.data.assistant_id } : undefined;


    // 多文件版本的 Bun 构建（只在看多文件版本时打开）
    const projectBuild = useArtifactVersionBuild({
        conversationId: session.conversationId,
        artifactKey: session.artifactKey,
        version: session.currentVersion,
        enabled: session.content?.type === "project",
    });
    return (
        <div className={cn("flex flex-col min-h-0", className)}>
            <ArtifactViewer
                compact
                type={content?.type ?? null}
                code={content?.code ?? ""}
                previewUrl={session.previewUrl}
                contentKey={content?.key ?? 0}
                title={content?.data.name}
                buildLogs={session.logs}
                bridgeConfig={bridgeConfig}
                previewOverride={choiceReference ? <SavedChoicePreview reference={choiceReference} /> : undefined}
                onOpenInWindow={onOpenInWindow}
                toolbarEnd={
                    onClose || trailing ? (
                        <>
                            {onClose && (
                                <Button variant="ghost" size="icon" iconLabel="关闭预览" onClick={onClose}>
                                    <X className="h-4 w-4" />
                                </Button>
                            )}
                            {trailing}
                        </>
                    ) : undefined
                }
                onFixWithAI={
                    conversationId !== undefined
                        ? (report, attachments) => requestArtifactFix(conversationId, report, attachments)
                        : undefined
                }
                onCaptureElement={elementCapture(conversationId, session)}
                onCodeChange={session.updateCode}
                projectFiles={session.projectFiles}
                projectBuild={projectBuild}
                entryFile={content?.data.entry_file}
                versions={session.versions}
                currentVersion={session.currentVersion}
                onSelectVersion={session.selectVersion}
                loadVersionFiles={session.loadVersionFiles}
            />
            <EnvironmentInstallDialog {...session.environmentDialog} />
        </div>
    );
}
