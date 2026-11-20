import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { AlertTriangle, Download, ExternalLink, Eye, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";
import { OfficeFormatIcon } from "@/components/office/OfficePreviewPane";
import { useOfficeSaveFlow } from "@/components/office/useOfficeSaveFlow";
import {
    detectOfficeExtension,
    isModernOfficeExtension,
    officeFormatLabel,
    openOfficePreviewModal,
    parseOfficePreviewError,
} from "@/utils/officeFile";

export interface OfficeEditResult {
    status?: string;
    output_ref?: string;
    file_name?: string;
    format?: string;
    size_bytes?: number;
    source_file_name?: string;
}

interface RefStatus {
    available: boolean;
    problem?: string | null;
    fileName: string;
    sizeBytes: number;
}

const parseJson = (text: string | undefined): unknown => {
    if (!text) return undefined;
    try {
        return JSON.parse(text);
    } catch {
        return undefined;
    }
};

/** 成功结果存为 MCP content 数组：`[{"type":"json","json":{…}}]`。 */
export function parseOfficeEditResult(result: string | undefined): OfficeEditResult | null {
    const parsed = parseJson(result);
    const parts = Array.isArray(parsed) ? parsed : (parsed as { content?: unknown[] } | undefined)?.content;
    if (Array.isArray(parts)) {
        for (const part of parts) {
            if (part && typeof part === "object") {
                const p = part as { type?: string; json?: unknown; text?: string };
                if (p.json && typeof p.json === "object") return p.json as OfficeEditResult;
                const inner = parseJson(p.text);
                if (inner && typeof inner === "object" && "output_ref" in (inner as object)) return inner as OfficeEditResult;
            }
        }
    }
    if (parsed && typeof parsed === "object" && "output_ref" in (parsed as object)) return parsed as OfficeEditResult;
    return null;
}

/** 失败时错误文本形如 `工具执行错误: [{"type":"text","text":"office_edit 失败（code）：说明"}]`。 */
export function parseOfficeEditError(error: string | undefined): { code: string; message: string } {
    const raw = error ?? "";
    const m = /office_edit 失败（([a-z_]+)）：([\s\S]*?)(?:"\s*}\s*]\s*)?$/.exec(raw);
    if (m) return { code: m[1], message: m[2].replace(/\\n/g, "\n").replace(/\\"/g, '"').trim() };
    return { code: "failed", message: raw.replace(/^工具执行(错误|失败)[:：]\s*/, "").trim() || "未说明原因" };
}

function formatSize(bytes?: number): string {
    if (bytes == null) return "";
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / 1048576).toFixed(1)} MB`;
}

/**
 * office_edit 的成果卡：只有文件真正写好并登记后才显示可用的预览 / 打开 / 另存；
 * 执行中、失败、文件丢失都如实显示状态。
 */
export default function OfficeEditToolCall(props: McpToolComponentProps) {
    const call = props.currentToolCall;
    const status = call?.status ?? props.status;
    const result = useMemo(() => parseOfficeEditResult(call?.result), [call?.result]);
    const failure = status === "failed" ? parseOfficeEditError(call?.error) : null;
    const outputRef = result?.output_ref;
    const [refStatus, setRefStatus] = useState<RefStatus | null>(null);
    const [refError, setRefError] = useState<string | null>(null);
    const [opening, setOpening] = useState(false);
    const { startSave, dialog } = useOfficeSaveFlow();

    useEffect(() => {
        if (!outputRef) return;
        let alive = true;
        invoke<RefStatus>("get_office_file_ref_status", { refId: outputRef })
            .then((s) => alive && setRefStatus(s))
            .catch((err) => alive && setRefError(parseOfficePreviewError(err).message));
        return () => {
            alive = false;
        };
    }, [outputRef]);

    const fileName = refStatus?.fileName ?? result?.file_name ?? "Office 文件";
    const ext = detectOfficeExtension(fileName) ?? (result?.format ? detectOfficeExtension(`x.${result.format}`) : null);
    const source = outputRef ? { refId: outputRef } : null;
    const unavailable = refError ?? (refStatus && !refStatus.available ? refStatus.problem ?? "文件不可用" : null);
    const ready = Boolean(source && refStatus?.available);

    if (status === "pending" || status === "executing" || (!result && !failure && status !== "success")) {
        return (
            <div className="my-1 flex items-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-sm text-muted-foreground" data-testid="office-edit-card" data-state="running">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                正在修改 Office 文件，完成后另存为新文件…
            </div>
        );
    }

    if (failure || !result) {
        return (
            <div className="my-1 rounded-lg border border-border bg-card px-3 py-2 text-sm" data-testid="office-edit-card" data-state="failed">
                <div className="flex items-center gap-2 font-medium text-foreground">
                    <AlertTriangle className="h-3.5 w-3.5 text-muted-foreground" />
                    没有生成修改后的文件
                </div>
                <div className="mt-1 whitespace-pre-wrap break-words text-xs text-muted-foreground">
                    {failure?.message ?? "工具没有返回结果"}
                    {failure?.message?.includes("原文件未改动") ? "" : "（原文件未改动）"}
                </div>
            </div>
        );
    }

    const preview = () => {
        if (!source) return;
        openOfficePreviewModal({ title: fileName, extension: ext, source, conversationId: props.conversationId ?? null });
    };
    const openExternally = async () => {
        if (!source) return;
        setOpening(true);
        try {
            await invoke("open_office_file_externally", { source });
        } catch (err) {
            toast.error(`调用外部应用失败：${err instanceof Error ? err.message : String(err)}`);
        } finally {
            setOpening(false);
        }
    };
    const saveAs = () => {
        if (!source || !isModernOfficeExtension(ext)) return;
        void startSave({ source, fileName, format: ext, purpose: "output" });
    };

    return (
        <div className="my-1 rounded-lg border border-border bg-card text-sm" data-testid="office-edit-card" data-state={ready ? "ready" : "unavailable"}>
            <div className="flex items-center gap-2.5 px-3 py-2.5">
                <OfficeFormatIcon ext={ext} className="h-5 w-5 shrink-0 text-muted-foreground" />
                <div className="min-w-0 flex-1">
                    <div className="truncate font-medium text-foreground">{fileName}</div>
                    <div className="truncate text-xs text-muted-foreground">
                        {unavailable
                            ? unavailable
                            : `${officeFormatLabel(ext)} · ${formatSize(refStatus?.sizeBytes ?? result.size_bytes)} · 已另存为新文件${result.source_file_name ? `，「${result.source_file_name}」未改动` : ""}`}
                    </div>
                </div>
            </div>
            <div className="flex flex-wrap gap-1.5 border-t border-border/60 px-3 py-2">
                <Button type="button" variant="outline" size="sm" className="h-7 gap-1 text-xs" disabled={!ready} onClick={preview}>
                    <Eye className="h-3.5 w-3.5" />
                    预览
                </Button>
                <Button type="button" variant="outline" size="sm" className="h-7 gap-1 text-xs" disabled={!ready || opening} onClick={() => void openExternally()}>
                    {opening ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <ExternalLink className="h-3.5 w-3.5" />}
                    用外部应用打开
                </Button>
                <Button type="button" variant="outline" size="sm" className="h-7 gap-1 text-xs" disabled={!ready} onClick={saveAs}>
                    <Download className="h-3.5 w-3.5" />
                    另存为
                </Button>
            </div>
            {dialog}
        </div>
    );
}
