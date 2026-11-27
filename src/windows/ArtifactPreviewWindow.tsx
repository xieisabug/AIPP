import { SavedChoicePreview, readPreviewReference } from "@/components/choices/ChoicePreview";
import { useArtifactVersionBuild } from "@/hooks/useArtifactVersionBuild";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bot, Database, Settings2 } from "lucide-react";
import "../styles/ArtifactPreviewWIndow.css";
import EnvironmentInstallDialog from "../components/EnvironmentInstallDialog";
import SaveArtifactDialog from "../components/SaveArtifactDialog";
import ArtifactViewer from "@/components/artifact/ArtifactViewer";
import { previewInstanceId } from "@/components/artifact/artifactRuntime";
import { useTheme } from "../hooks/useTheme";
import { useArtifactPreviewSession } from "@/hooks/useArtifactPreviewSession";
import type { ArtifactData } from "../hooks/useArtifactEvents";
import { elementCapture } from "@/components/artifact/elementCapture";
import { requestArtifactFix } from "@/utils/artifactFixRequest";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { AssistantBasicInfo } from "@/data/ArtifactCollection";
import {
    generateRandomDbId,
    loadArtifactRuntimeConfig,
    normalizeDbId,
    persistArtifactRuntimeConfig,
} from "@/utils/artifactConfig";

// localStorage 键名：记录窗口刷新前是否在预览，刷新后据此请求后端恢复
const ARTIFACT_CACHE_KEY = "artifact_preview_cache";
const CACHE_EXPIRY_MS = 24 * 60 * 60 * 1000;
const WINDOW_LABEL = "artifact_preview";

type RuntimeConfig = { db_id?: string; assistant_id?: number };

/** 「artifact_preview」窗口：聊天里点开的 Artifact 在这里预览。 */
export default function ArtifactPreviewWindow() {
    useTheme(WINDOW_LABEL);

    const [conversationIdForConfig, setConversationIdForConfig] = useState<number | undefined>(undefined);
    const [runtimeConfig, setRuntimeConfig] = useState<RuntimeConfig>({});
    const [assistants, setAssistants] = useState<AssistantBasicInfo[]>([]);
    const [showSaveDialog, setShowSaveDialog] = useState(false);

    const handleData = useCallback((data: ArtifactData) => {
        try {
            localStorage.setItem(ARTIFACT_CACHE_KEY, JSON.stringify({ type: data.type, timestamp: Date.now() }));
        } catch {
            /* 存储不可用时只是无法在刷新后恢复 */
        }
        const convId = typeof data.conversation_id === "number" ? data.conversation_id : undefined;
        setConversationIdForConfig(convId);
        // 数据里带绑定时优先，否则沿用本会话的临时配置
        const sessionConfig = loadArtifactRuntimeConfig(convId);
        const hasOverride = Boolean(data.db_id) || typeof data.assistant_id === "number";
        const next: RuntimeConfig = hasOverride
            ? {
                  db_id: data.db_id || sessionConfig.db_id,
                  assistant_id: typeof data.assistant_id === "number" ? data.assistant_id : sessionConfig.assistant_id,
              }
            : sessionConfig;
        setRuntimeConfig(next);
        if (hasOverride) persistArtifactRuntimeConfig(next, convId);
    }, []);

    const session = useArtifactPreviewSession({
        windowType: "preview",
        readyWindow: WINDOW_LABEL,
        closeRuntime: (kind) =>
            invoke(kind === "vue" ? "close_vue_preview" : "close_react_preview", {
                previewId: previewInstanceId(kind, WINDOW_LABEL),
            }),
        onData: handleData,
        followManifestUpdates: true,
    });
    const { content } = session;
    const hasDataRef = useRef(false);
    hasDataRef.current = session.hasReceivedData || content !== null;

    useEffect(() => {
        invoke<AssistantBasicInfo[]>("artifact_get_assistants")
            .then(setAssistants)
            .catch(() => setAssistants([]));
    }, []);

    // 窗口刷新后：若刷新前在预览且还没收到新数据，请后端按缓存重新推送
    const triedRestoreRef = useRef(false);
    useEffect(() => {
        if (triedRestoreRef.current) return;
        triedRestoreRef.current = true;
        const timer = setTimeout(() => {
            if (hasDataRef.current) return;
            try {
                const cached = JSON.parse(localStorage.getItem(ARTIFACT_CACHE_KEY) || "null");
                if (!cached || Date.now() - cached.timestamp > CACHE_EXPIRY_MS) {
                    localStorage.removeItem(ARTIFACT_CACHE_KEY);
                    return;
                }
            } catch {
                return;
            }
            invoke<string | null>("restore_artifact_preview", { sourceWindow: WINDOW_LABEL })
                .then((result) => {
                    if (result) session.addLog("log", "正在恢复上次的预览…");
                })
                .catch((error) => console.warn("[ArtifactPreviewWindow] restore failed", error));
        }, 300);
        return () => clearTimeout(timer);
        // 只在首次挂载时尝试一次
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    const updateRuntimeConfig = (patch: Partial<RuntimeConfig>) => {
        setRuntimeConfig((prev) => {
            const next = { ...prev, ...patch };
            persistArtifactRuntimeConfig(next, conversationIdForConfig);
            return next;
        });
    };

    const choiceReference = content && content.type === "html" ? readPreviewReference(content.code) : null;
    // 多文件版本按完整工程收藏（后端打包），其余按入口代码收藏
    const canSave =
        content !== null && ["vue", "react", "html", "svg", "markdown", "md", "mermaid", "project"].includes(content.type);
    const saveSource =
        content && conversationIdForConfig !== undefined && session.artifactKey && session.currentVersion !== undefined
            ? { conversation_id: conversationIdForConfig, artifact_key: session.artifactKey, version: session.currentVersion }
            : null;
    const conversationId = session.conversationId;

    const actions = content && (
        <>
            <Popover>
                <PopoverTrigger asChild>
                    <Button variant="ghost" size="icon" iconLabel="数据库与助手绑定">
                        <Settings2 className="h-4 w-4" />
                    </Button>
                </PopoverTrigger>
                <PopoverContent align="end" className="w-80">
                    <div className="space-y-4">
                        <div className="text-sm font-medium text-foreground">数据库与助手（未保存也可用）</div>
                        <div className="space-y-2">
                            <Label className="flex items-center gap-2 text-sm">
                                <Database className="h-4 w-4" />
                                数据库标识
                            </Label>
                            <div className="flex gap-2">
                                <Input
                                    value={runtimeConfig.db_id || ""}
                                    onChange={(event) =>
                                        updateRuntimeConfig({ db_id: normalizeDbId(event.target.value) || undefined })
                                    }
                                    placeholder="artifact-xxxx"
                                />
                                <Button variant="outline" size="sm" onClick={() => updateRuntimeConfig({ db_id: generateRandomDbId() })}>
                                    随机
                                </Button>
                            </div>
                            <p className="text-xs text-muted-foreground">仅支持字母、数字、下划线和连字符</p>
                        </div>
                        <div className="space-y-2">
                            <Label className="flex items-center gap-2 text-sm">
                                <Bot className="h-4 w-4" />
                                关联助手
                            </Label>
                            <Select
                                value={runtimeConfig.assistant_id ? String(runtimeConfig.assistant_id) : "none"}
                                onValueChange={(value) =>
                                    updateRuntimeConfig({
                                        assistant_id: value && value !== "none" ? parseInt(value, 10) : undefined,
                                    })
                                }
                            >
                                <SelectTrigger className="w-full">
                                    <SelectValue placeholder="选择一个助手（可选）" />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value="none">不关联助手</SelectItem>
                                    {assistants.map((assistant) => (
                                        <SelectItem key={assistant.id} value={String(assistant.id)}>
                                            <span className="flex items-center gap-2">
                                                <span>{assistant.icon}</span>
                                                <span>{assistant.name}</span>
                                            </span>
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                        </div>
                    </div>
                </PopoverContent>
            </Popover>
            {canSave && (
                <Button variant="outline" size="sm" onClick={() => setShowSaveDialog(true)}>
                    收藏
                </Button>
            )}
        </>
    );


    // 多文件版本的 Bun 构建（只在看多文件版本时打开）
    const projectBuild = useArtifactVersionBuild({
        conversationId: session.conversationId,
        artifactKey: session.artifactKey,
        version: session.currentVersion,
        enabled: session.content?.type === "project",
    });
    return (
        <div className="flex h-screen bg-background overflow-hidden" data-aipp-window={WINDOW_LABEL} data-aipp-slot="window-root">
            <div
                className="flex flex-col flex-1 bg-background rounded-xl m-2 shadow-lg border border-border min-h-0 overflow-hidden"
                data-aipp-slot="artifact-preview-main"
            >
                <ArtifactViewer
                    type={content?.type ?? null}
                    code={content?.code ?? ""}
                    previewUrl={session.previewUrl}
                    contentKey={content?.key ?? 0}
                    title={content?.data.name}
                    buildLogs={session.logs}
                    bridgeConfig={runtimeConfig}
                    actions={actions}
                    previewOverride={choiceReference ? <SavedChoicePreview reference={choiceReference} /> : undefined}
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
            </div>

            <EnvironmentInstallDialog {...session.environmentDialog} />

            {content && (
                <SaveArtifactDialog
                    isOpen={showSaveDialog}
                    onClose={() => setShowSaveDialog(false)}
                    artifactType={content.type === "md" ? "markdown" : content.type === "project" ? "html" : content.type}
                    code={content.code}
                    choiceReference={choiceReference}
                    initialDbId={runtimeConfig.db_id}
                    initialAssistantId={runtimeConfig.assistant_id}
                    source={saveSource}
                />
            )}
        </div>
    );
}
