import { SavedChoicePreview, readPreviewReference } from "@/components/choices/ChoicePreview";
import { invoke } from "@tauri-apps/api/core";
import EnvironmentInstallDialog from "@/components/EnvironmentInstallDialog";
import ArtifactViewer from "@/components/artifact/ArtifactViewer";
import { previewInstanceId } from "@/components/artifact/artifactRuntime";
import { useArtifactPreviewSession } from "@/hooks/useArtifactPreviewSession";
import { requestArtifactFix } from "@/utils/artifactFixRequest";
import { cn } from "@/utils/utils";

const WINDOW_LABEL = "sidebar";

interface EmbeddedArtifactPreviewProps {
    className?: string;
    /** 在独立预览窗口打开当前内容。 */
    onOpenInWindow?: () => void;
}

/** 侧栏窗口内嵌的 Artifact 预览；使用自己的 React/Vue 预览实例，与预览窗口互不影响。 */
export default function EmbeddedArtifactPreview({ className, onOpenInWindow }: EmbeddedArtifactPreviewProps) {
    const session = useArtifactPreviewSession({
        windowType: "preview",
        readyWindow: WINDOW_LABEL,
        sourceWindow: WINDOW_LABEL,
        closeRuntime: (kind) =>
            invoke(kind === "vue" ? "close_vue_preview" : "close_react_preview", {
                previewId: previewInstanceId(kind, WINDOW_LABEL),
            }),
        followManifestUpdates: true,
    });
    const { content, conversationId } = session;
    const choiceReference = content && content.type === "html" ? readPreviewReference(content.code) : null;
    const bridgeConfig = content ? { db_id: content.data.db_id, assistant_id: content.data.assistant_id } : undefined;

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
                onFixWithAI={
                    conversationId !== undefined ? (report) => requestArtifactFix(conversationId, report) : undefined
                }
                onCodeChange={session.updateCode}
            />
            <EnvironmentInstallDialog {...session.environmentDialog} />
        </div>
    );
}
