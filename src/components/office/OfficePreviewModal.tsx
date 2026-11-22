import React, { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { ExternalLink, Loader2, PenLine, Save, Sparkles, X } from "lucide-react";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import {
    AlertDialog,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Textarea } from "@/components/ui/textarea";
import {
    detectOfficeExtension,
    isModernOfficeExtension,
    officeFormatLabel,
    officeSourceKey,
    OPEN_OFFICE_PREVIEW_EVENT,
    parseOfficePreviewError,
    type OfficePreviewTarget,
} from "@/utils/officeFile";
import {
    buildOfficeSelectionPrompt,
    describeSelectionLocation,
    hasSelectionContent,
    selectionExcerpt,
    type OfficeSelection,
} from "@/utils/officeSelection";
import {
    findOfficeSelectionReceiver,
    subscribeOfficeSelectionReceivers,
} from "@/services/officeSelectionReceiver";
import OfficePreviewPane, {
    OfficeFormatIcon,
    type OfficeLoadedFile,
    type OfficePaneMode,
    type OfficePreviewPaneHandle,
} from "./OfficePreviewPane";
import { useOfficeSaveFlow } from "./useOfficeSaveFlow";

const VIEWER_EDIT_PROTOCOL = 2;

interface RegisteredRef {
    refId: string;
    fileName: string;
    format: "docx" | "xlsx" | "pptx";
}

/** 交给助手前展示给用户确认的选区原文（与消息里附带的内容一致）。 */
function selectionPreviewText(selection: OfficeSelection): string {
    switch (selection.format) {
        case "docx":
            return selection.selectedText;
        case "xlsx":
            return selection.cells.map((row) => row.join("\t")).join("\n") + (selection.truncated ? "\n…" : "");
        case "pptx":
            return selection.selectedText || selection.shapeText || "（这张幻灯片没有文字）";
    }
}

/**
 * 全局唯一的 Office 预览窗口，在 App 根部挂载一次；任意入口通过
 * `openOfficePreviewModal()` 派发事件打开。同一时刻只有一个查看器实例，关闭即卸载释放内存。
 * 支持：只读预览、选中内容交给助手修改（#330）、手动编辑后另存副本（#331）。
 */
export const OfficePreviewModal: React.FC = () => {
    const [target, setTarget] = useState<OfficePreviewTarget | null>(null);
    const [openingExternal, setOpeningExternal] = useState(false);
    const [mode, setMode] = useState<OfficePaneMode>("view");
    const [dirty, setDirty] = useState(false);
    const [selection, setSelection] = useState<OfficeSelection | null>(null);
    const [loadedFile, setLoadedFile] = useState<OfficeLoadedFile | null>(null);
    const [protocol, setProtocol] = useState(0);
    const [aiOpen, setAiOpen] = useState(false);
    const [aiRequest, setAiRequest] = useState("");
    const [sending, setSending] = useState(false);
    const [aiError, setAiError] = useState<{ title: string; message: string; stale: boolean } | null>(null);
    const [saving, setSaving] = useState(false);
    const [paneKey, setPaneKey] = useState(0);
    const [confirmDiscard, setConfirmDiscard] = useState<null | (() => void)>(null);
    const paneRef = useRef<OfficePreviewPaneHandle | null>(null);
    const { startSave, dialog: saveDialog } = useOfficeSaveFlow();

    const resetSession = useCallback(() => {
        setMode("view");
        setDirty(false);
        setSelection(null);
        setLoadedFile(null);
        setProtocol(0);
        setAiOpen(false);
        setAiRequest("");
        setAiError(null);
    }, []);

    useEffect(() => {
        const handler = (event: Event) => {
            const detail = (event as CustomEvent<OfficePreviewTarget>).detail;
            if (!detail || !detail.source) return;
            resetSession();
            setTarget(detail);
        };
        window.addEventListener(OPEN_OFFICE_PREVIEW_EVENT, handler);
        return () => window.removeEventListener(OPEN_OFFICE_PREVIEW_EVENT, handler);
    }, [resetSession]);

    const receiver = useSyncExternalStore(
        subscribeOfficeSelectionReceivers,
        () => (target ? findOfficeSelectionReceiver(target.conversationId) : null),
    );

    const ext = target?.extension ?? detectOfficeExtension(target?.title) ?? detectOfficeExtension(target?.source.localPath);
    const editable = isModernOfficeExtension(ext);
    const canAskAi = Boolean(receiver) && editable && mode === "view";
    const supportsEdit = protocol >= VIEWER_EDIT_PROTOCOL;

    /** 有未保存的手动修改时先确认再继续。 */
    const guardDirty = useCallback(
        (action: () => void) => {
            if (mode === "edit" && dirty) setConfirmDiscard(() => action);
            else action();
        },
        [mode, dirty],
    );

    const close = useCallback(() => {
        setTarget(null);
        resetSession();
    }, [resetSession]);

    const handleOpenExternally = async () => {
        if (!target) return;
        setOpeningExternal(true);
        try {
            await invoke("open_office_file_externally", { source: target.source });
        } catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            toast.error(`调用外部应用失败：${msg}`);
        } finally {
            setOpeningExternal(false);
        }
    };

    const switchMode = (next: OfficePaneMode) => {
        guardDirty(() => {
            setMode(next);
            setDirty(false);
            setSelection(null);
            setAiOpen(false);
        });
    };

    const handleSaveCopy = useCallback(async () => {
        if (!target || !loadedFile || !paneRef.current) return;
        setSaving(true);
        let bytes: ArrayBuffer;
        try {
            bytes = await paneRef.current.requestSave();
        } catch (err) {
            toast.error(`读取编辑内容失败：${err instanceof Error ? err.message : String(err)}`);
            setSaving(false);
            return;
        }
        setSaving(false);
        const outcome = await startSave({
            source: target.source,
            fileName: loadedFile.fileName,
            format: loadedFile.format,
            purpose: "copy",
            copyOriginToken: loadedFile.copyOriginToken,
            bytes,
        });
        if (outcome.status === "saved") setDirty(false);
    }, [target, loadedFile, startSave]);

    const handleSendToAi = async () => {
        if (!target || !receiver || !selection || !loadedFile || !aiRequest.trim()) return;
        setSending(true);
        setAiError(null);
        try {
            const ref = await invoke<RegisteredRef>("register_office_selection_ref", {
                source: target.source,
                expectedSha256: loadedFile.sha256,
                conversationId: receiver.conversationId,
            });
            const prompt = buildOfficeSelectionPrompt(aiRequest, {
                refId: ref.refId,
                fileName: ref.fileName,
                format: ref.format,
                selection,
            });
            await receiver.send(prompt);
            toast.success("已把修改要求和选区发给助手", { description: "修改结果会另存为新文件，原文件不变。" });
            close();
        } catch (err) {
            const parsed = parseOfficePreviewError(err);
            setAiError({ title: parsed.title, message: parsed.message, stale: parsed.code === "stale_selection" });
        } finally {
            setSending(false);
        }
    };

    const reloadAfterExternalChange = () => {
        setAiError(null);
        setSelection(null);
        setAiOpen(false);
        setPaneKey((k) => k + 1);
    };

    const sourceKey = target ? officeSourceKey(target.source) : "";
    const location = selection ? describeSelectionLocation(selection) : "";
    const showSelectionBar = canAskAi && supportsEdit && hasSelectionContent(selection) && !aiOpen;
    const paneHeight = aiOpen || showSelectionBar ? "h-[64vh]" : "h-[78vh]";

    const header = useMemo(() => (mode === "edit" ? "编辑中 · 保存为副本" : "只读预览"), [mode]);

    return (
        <>
            <Dialog open={Boolean(target)} onOpenChange={(open) => !open && guardDirty(close)}>
                <DialogContent className="flex max-h-[92vh] w-[95vw] max-w-5xl sm:max-w-5xl flex-col gap-0 overflow-hidden p-0">
                    <DialogHeader className="flex flex-row flex-wrap items-center justify-between gap-2 border-b border-border/60 px-4 py-3 pr-12 text-left">
                        <div className="flex min-w-0 flex-1 items-center gap-2">
                            <OfficeFormatIcon ext={ext} className="h-4 w-4 shrink-0 text-muted-foreground" />
                            <DialogTitle className="truncate text-sm font-medium">
                                {target?.title || "Office 文档预览"}
                            </DialogTitle>
                            <Badge variant="secondary" className="shrink-0 text-[11px] font-normal" data-testid="office-mode-badge">
                                {header}
                            </Badge>
                        </div>
                        <DialogDescription className="sr-only">
                            {officeFormatLabel(ext)} 预览；编辑或交给 AI 修改的结果都另存为新文件，不修改原文件
                        </DialogDescription>
                        <div className="flex shrink-0 items-center gap-1.5">
                            {editable && supportsEdit && mode === "view" && (
                                <Button type="button" variant="outline" size="sm" onClick={() => switchMode("edit")} className="h-8 gap-1.5 text-xs" data-testid="office-enter-edit">
                                    <PenLine className="h-3.5 w-3.5" />
                                    编辑
                                </Button>
                            )}
                            {mode === "edit" && (
                                <>
                                    <Button type="button" variant="outline" size="sm" onClick={() => switchMode("view")} className="h-8 text-xs">
                                        退出编辑
                                    </Button>
                                    <Button
                                        type="button"
                                        size="sm"
                                        onClick={() => void handleSaveCopy()}
                                        disabled={saving || !loadedFile}
                                        className="h-8 gap-1.5 text-xs"
                                        data-testid="office-save-copy"
                                    >
                                        {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Save className="h-3.5 w-3.5" />}
                                        另存副本
                                    </Button>
                                </>
                            )}
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                onClick={() => void handleOpenExternally()}
                                disabled={openingExternal}
                                className="h-8 gap-1.5 text-xs"
                            >
                                {openingExternal ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <ExternalLink className="h-3.5 w-3.5" />}
                                用外部应用打开
                            </Button>
                        </div>
                    </DialogHeader>
                    {target && (
                        <OfficePreviewPane
                            key={`${sourceKey}#${paneKey}`}
                            ref={paneRef}
                            title={target.title}
                            source={target.source}
                            heightClassName={paneHeight}
                            mode={mode}
                            requireEdit={Boolean(receiver) || mode === "edit"}
                            onSelectionChange={setSelection}
                            onDirtyChange={setDirty}
                            onFileLoaded={setLoadedFile}
                            onViewerProtocol={setProtocol}
                            onSaveShortcut={() => void handleSaveCopy()}
                        />
                    )}

                    {showSelectionBar && selection && (
                        <div className="flex items-center gap-3 border-t border-border/60 px-4 py-2.5" data-testid="office-selection-bar">
                            <div className="min-w-0 flex-1">
                                <div className="truncate text-xs font-medium text-foreground">已选：{location}</div>
                                <div className="truncate text-xs text-muted-foreground">{selectionExcerpt(selection) || "（没有文字）"}</div>
                            </div>
                            <Button type="button" size="sm" className="h-8 shrink-0 gap-1.5 text-xs" onClick={() => setAiOpen(true)} data-testid="office-ask-ai">
                                <Sparkles className="h-3.5 w-3.5" />
                                让 AI 修改
                            </Button>
                        </div>
                    )}

                    {aiOpen && selection && receiver && (
                        <div className="space-y-2.5 border-t border-border/60 px-4 py-3" data-testid="office-ai-panel">
                            <div className="flex items-start justify-between gap-2">
                                <div className="min-w-0 space-y-0.5">
                                    <div className="truncate text-xs font-medium text-foreground">
                                        {loadedFile?.fileName ?? target?.title} · {location}
                                    </div>
                                    <div className="text-xs text-muted-foreground">
                                        发送到：{receiver.label}。助手修改后会另存为新文件，原文件不会被改动。
                                    </div>
                                </div>
                                <Button type="button" variant="ghost" size="icon" className="h-7 w-7 shrink-0" onClick={() => setAiOpen(false)} aria-label="取消">
                                    <X className="h-3.5 w-3.5" />
                                </Button>
                            </div>
                            <pre className="max-h-24 overflow-auto whitespace-pre-wrap break-all rounded-md border border-border/60 bg-muted/30 px-2.5 py-1.5 font-sans text-xs text-muted-foreground" data-testid="office-ai-target">
                                {selectionPreviewText(selection)}
                            </pre>
                            <Textarea
                                autoFocus
                                value={aiRequest}
                                onChange={(e) => setAiRequest(e.target.value)}
                                onKeyDown={(e) => {
                                    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                                        e.preventDefault();
                                        void handleSendToAi();
                                    }
                                }}
                                placeholder="写下修改要求，例如：改得更正式，控制在 50 字以内"
                                className="min-h-[64px] text-sm"
                                data-testid="office-ai-request"
                            />
                            {aiError && (
                                <div className="flex items-center justify-between gap-2 rounded-md border border-border/60 px-2.5 py-1.5 text-xs" data-testid="office-ai-error">
                                    <span className="min-w-0 break-words text-muted-foreground">
                                        <span className="font-medium text-foreground">{aiError.title}：</span>
                                        {aiError.message}
                                    </span>
                                    {aiError.stale && (
                                        <Button type="button" variant="outline" size="sm" className="h-7 shrink-0 text-xs" onClick={reloadAfterExternalChange}>
                                            重新加载
                                        </Button>
                                    )}
                                </div>
                            )}
                            <div className="flex justify-end gap-2">
                                <Button type="button" variant="outline" size="sm" className="h-8 text-xs" onClick={() => setAiOpen(false)}>
                                    取消
                                </Button>
                                <Button
                                    type="button"
                                    size="sm"
                                    className="h-8 gap-1.5 text-xs"
                                    disabled={sending || !aiRequest.trim() || !loadedFile}
                                    onClick={() => void handleSendToAi()}
                                    data-testid="office-ai-send"
                                >
                                    {sending && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                                    发送给助手
                                </Button>
                            </div>
                        </div>
                    )}
                </DialogContent>
            </Dialog>

            <AlertDialog open={confirmDiscard !== null} onOpenChange={(open) => !open && setConfirmDiscard(null)}>
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>放弃未保存的修改？</AlertDialogTitle>
                        <AlertDialogDescription>
                            编辑内容还没有另存。放弃后修改会丢失，原文件本来就不会被改动。
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <Button variant="outline" onClick={() => setConfirmDiscard(null)}>
                            继续编辑
                        </Button>
                        <Button
                            variant="destructive"
                            onClick={() => {
                                const action = confirmDiscard;
                                setConfirmDiscard(null);
                                setDirty(false);
                                action?.();
                            }}
                        >
                            放弃修改
                        </Button>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
            {saveDialog}
        </>
    );
};

export default OfficePreviewModal;
