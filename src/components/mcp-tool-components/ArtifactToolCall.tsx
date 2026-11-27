import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { AlertTriangle, AppWindow, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ConversationArtifactItem } from "@/hooks/useExplicitArtifacts";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";
import { artifactPreviewTarget } from "@/components/artifact/artifactPane";

export interface ShowArtifactResult {
    artifact_key: string;
    title?: string;
    language?: string;
    preview_type?: string;
    entry_file?: string;
    /** 这次发布对应的版本；卡片打开的就是这个版本。 */
    version?: number;
    /** 发布后的运行检查（show_artifact 默认执行）。 */
    diagnostics?: { status: "ok" | "errors" | "skipped"; errors?: string[] };
    /** 调用参数里的 check=false 表示跳过运行检查。 */
    check?: boolean;
}

const parseJson = (text: string | undefined): unknown => {
    if (!text) return undefined;
    try {
        return JSON.parse(text);
    } catch {
        return undefined;
    }
};

const isResult = (value: unknown): value is ShowArtifactResult =>
    Boolean(value && typeof value === "object" && typeof (value as ShowArtifactResult).artifact_key === "string");

/** 成功结果存为 MCP content 数组：`[{"type":"json","json":{…}}]`。 */
export function parseShowArtifactResult(result: string | undefined): ShowArtifactResult | null {
    const parsed = parseJson(result);
    const parts = Array.isArray(parsed) ? parsed : (parsed as { content?: unknown[] } | undefined)?.content;
    if (Array.isArray(parts)) {
        for (const part of parts) {
            const p = part as { json?: unknown; text?: string } | null;
            if (isResult(p?.json)) return p.json;
            const inner = parseJson(p?.text);
            if (isResult(inner)) return inner;
        }
    }
    return isResult(parsed) ? parsed : null;
}

/** 失败时错误文本形如 `工具执行错误: [{"type":"text","text":"…"}]`，只取说明。 */
export function parseShowArtifactError(error: string | undefined): string {
    const raw = (error ?? "").replace(/^工具执行(错误|失败)[:：]\s*/, "").trim();
    const parts = parseJson(raw);
    if (Array.isArray(parts)) {
        const text = parts.map((p) => (p as { text?: string })?.text).filter(Boolean).join("\n");
        if (text) return text;
    }
    return raw || "未说明原因";
}

const LANGUAGE_LABELS: Record<string, string> = {
    html: "HTML",
    svg: "SVG",
    markdown: "Markdown",
    mermaid: "Mermaid 图",
    drawio: "draw.io 图",
    react: "React",
    jsx: "React",
    tsx: "React",
    vue: "Vue",
};

function artifactKindLabel(language?: string): string {
    if (!language) return "Artifact";
    const label = LANGUAGE_LABELS[language.toLowerCase()] ?? language.toUpperCase();
    return `${label} · Artifact`;
}

/**
 * show_artifact 的成果卡：发布成功后显示标题，点击打开最新版本的预览（带上发布时绑定的数据库与助手）。
 * 管家简洁聊天和群聊里没有执行卡片，这张卡就是用户看到 Artifact 的入口。
 */
export default function ArtifactToolCall(props: McpToolComponentProps) {
    const call = props.currentToolCall;
    const status = call?.status ?? props.status;
    const conversationId = props.conversationId ?? call?.conversation_id;
    const result = useMemo(() => parseShowArtifactResult(call?.result), [call?.result]);
    const params = useMemo(
        () => (parseJson(call?.parameters ?? props.parameters) ?? {}) as Partial<ShowArtifactResult>,
        [call?.parameters, props.parameters],
    );
    const [opening, setOpening] = useState(false);

    if (status === "failed") {
        return (
            <div className="my-1 rounded-lg border border-border bg-card px-3 py-2 text-sm" data-testid="artifact-card" data-state="failed">
                <div className="flex items-center gap-2 font-medium text-foreground">
                    <AlertTriangle className="h-3.5 w-3.5 text-muted-foreground" />
                    Artifact 发布失败
                </div>
                <div className="mt-1 whitespace-pre-wrap break-words text-xs text-muted-foreground">
                    {parseShowArtifactError(call?.error ?? props.error)}
                </div>
            </div>
        );
    }

    if (status !== "success" || !result) {
        return (
            <div className="my-1 flex items-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-sm text-muted-foreground" data-testid="artifact-card" data-state="running">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {params.check === false ? "正在发布" : "正在发布并检查"} Artifact{params.title ? `「${params.title}」` : ""}…
            </div>
        );
    }

    const title = result.title || result.artifact_key;
    const runtimeErrors = result.diagnostics?.status === "errors" ? result.diagnostics.errors?.length ?? 0 : 0;
    const open = async () => {
        if (conversationId == null) return;
        setOpening(true);
        try {
            const items = await invoke<ConversationArtifactItem[]>("list_conversation_artifacts", { conversationId });
            const item = items.find((candidate) => candidate.artifact_key === result.artifact_key);
            if (!item) {
                toast.error(`找不到 Artifact「${title}」，可能已被删除`);
                return;
            }
            await invoke("run_artifacts", {
                lang: item.language || item.preview_type,
                inputStr: item.code,
                conversationId,
                dbId: item.db_id ?? undefined,
                assistantId: item.assistant_id ?? undefined,
                artifactKey: item.artifact_key,
                entryFile: item.entry_file,
                version: result.version,
                ...artifactPreviewTarget(),
            });
        } catch (error) {
            toast.error(`打开 Artifact 失败：${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setOpening(false);
        }
    };

    return (
        <div className="my-1 flex items-center gap-2.5 rounded-lg border border-border bg-card px-3 py-2.5 text-sm" data-testid="artifact-card" data-state="ready">
            <AppWindow className="h-5 w-5 shrink-0 text-muted-foreground" />
            <div className="min-w-0 flex-1">
                <div className="truncate font-medium text-foreground">{title}</div>
                <div className="truncate text-xs text-muted-foreground">
                    {artifactKindLabel(result.preview_type || result.language)}
                    {result.version !== undefined && result.version > 1 && ` · v${result.version}`}
                    {runtimeErrors > 0 && (
                        <span className="text-destructive" title={result.diagnostics?.errors?.join("\n\n")}>
                            {` · 运行检查发现 ${runtimeErrors} 个错误`}
                        </span>
                    )}
                </div>
            </div>
            <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 shrink-0 gap-1 text-xs"
                disabled={opening || conversationId == null}
                onClick={() => void open()}
            >
                {opening && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                打开
            </Button>
        </div>
    );
}
