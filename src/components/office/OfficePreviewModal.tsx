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
    buildOfficeReviewPrompt,
    describeSelectionLocation,
    hasSelectionContent,
    type OfficeSuggestion,
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

type Point = { x: number; y: number };
type SuggestionDraft = OfficeSuggestion & { point: Point };

interface RegisteredRef {
    documentText?: string;
    refId: string;
    fileName: string;
    format: "docx" | "xlsx" | "pptx";
}

/** 交给助手前展示给用户确认的选区原文（与消息里附带的内容一致）。 */
function selectionPreviewText(selection: OfficeSelection): string {
    switch (selection.format) {
        case "docx":
            return selection.selectedText;
        case "xlsx": {
            const text = selection.cells.map(row => row.join("\t")).join("\n");
            return (text.trim() ? text : "（选区内没有显示值）") + (selection.truncated ? "\n…" : "");
        }
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
    const [suggestions, setSuggestions] = useState<OfficeSuggestion[]>([]);
    const [draft, setDraft] = useState<SuggestionDraft | null>(null);
    const nextSuggestion = useRef(1);
    const reviewArea = useRef<HTMLDivElement>(null);
    const session = useRef(0);
    const sendLock = useRef(false);
    const selectionFingerprint = useRef("");
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
        session.current++;
        sendLock.current = false;
        setSending(false);
        nextSuggestion.current = 1;
        selectionFingerprint.current = "";
        setSuggestions([]);
        setDraft(null);
        setAiRequest("");
        setAiError(null);
    }, []);

    useEffect(() => {
        const handler = (event: Event) => {
            const detail = (event as CustomEvent<OfficePreviewTarget>).detail;
            if (!detail || !detail.source) return;
            resetSession();
            setPaneKey(k => k + 1);
            setTarget(detail);
        };
        window.addEventListener(OPEN_OFFICE_PREVIEW_EVENT, handler);
        return () => window.removeEventListener(OPEN_OFFICE_PREVIEW_EVENT, handler);
    }, [resetSession]);

    const receiver = useSyncExternalStore(
        subscribeOfficeSelectionReceivers,
        () => (target ? findOfficeSelectionReceiver(target.conversationId) : null),
    );

    const ext = loadedFile?.format ?? target?.extension ?? detectOfficeExtension(target?.title) ?? detectOfficeExtension(target?.source.localPath);
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
            setDraft(null);
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
        if (!target || !receiver || !loadedFile || draft || sendLock.current || aiError?.stale || !(suggestions.length || aiRequest.trim())) return;
        const currentSession = session.current;
        sendLock.current = true;
        setSending(true);
        setAiError(null);
        try {
            const ref = await invoke<RegisteredRef>("register_office_selection_ref", {
                source: target.source,
                expectedSha256: suggestions.length ? selectionFingerprint.current : loadedFile.sha256,
                includeDocumentText: Boolean(aiRequest.trim()),
                conversationId: receiver.conversationId,
            });
            if (currentSession !== session.current || receiver !== findOfficeSelectionReceiver(target.conversationId)) {
                throw new Error("接收会话已切换，请核对后重新发送");
            }
            await receiver.send(buildOfficeReviewPrompt(ref, aiRequest, suggestions));
            if (currentSession !== session.current) return;
            toast.success("修改意见已发送");
            close();
        } catch (err) {
            if (currentSession !== session.current) return;
            const parsed = parseOfficePreviewError(err);
            setAiError({ title: parsed.title, message: parsed.message, stale: parsed.code === "stale_selection" });
        } finally {
            if (currentSession === session.current) {
                sendLock.current = false;
                setSending(false);
            }
        }
    };

    const reloadAfterExternalChange = () => {
        setAiError(null);
        setSelection(null);
        setDraft(null);
        setSuggestions([]);
        setLoadedFile(null);
        setPaneKey((k) => k + 1);
    };

    const sourceKey = target ? officeSourceKey(target.source) : "";
    function editSuggestion(item: OfficeSuggestion, point: Point) {
        if (draft || sending || aiError?.stale) return;
        const rect = reviewArea.current?.getBoundingClientRect();
        if (!rect) return;
        setDraft({ ...item, point: { x: point.x - rect.left, y: point.y - rect.top } });
    }

    function startSuggestion(value: OfficeSelection, point: Point) {
        if (!loadedFile || draft || sending) return;
        if (suggestions.length && selectionFingerprint.current !== loadedFile.sha256) {
            setAiError({ title: "文件已被修改", message: "已有建议来自旧版本，请重新加载后选择。", stale: true });
            return;
        }
        selectionFingerprint.current = loadedFile.sha256;
        editSuggestion({ id: nextSuggestion.current, selection: value, comment: "" }, point);
    }

    function confirmSuggestion() {
        if (!draft?.comment.trim()) return;
        const item = { id: draft.id, selection: draft.selection, comment: draft.comment.trim() };
        if (item.id === nextSuggestion.current) {
            nextSuggestion.current++;
            setSuggestions(items => [...items, item]);
        } else {
            setSuggestions(items => items.map(old => old.id === item.id ? item : old));
        }
        setDraft(null);
    }

    const header = useMemo(() => (mode === "edit" ? "编辑中 · 保存为副本" : "只读预览"), [mode]);

    return (
        <>
            <Dialog open={Boolean(target)} onOpenChange={(open) => !open && !sending && guardDirty(close)}>
                <DialogContent onEscapeKeyDown={(event) => {
                    if (draft || sending) event.preventDefault();
                    if (draft) setDraft(null);
                }} className="flex max-h-[92vh] w-[95vw] max-w-5xl sm:max-w-5xl flex-col gap-0 overflow-hidden p-0">
                    <DialogHeader className="flex shrink-0 flex-row flex-wrap items-center justify-between gap-2 border-b border-border/60 px-4 py-3 pr-12 text-left">
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
                                <Button type="button" variant="outline" size="sm" disabled={sending || Boolean(draft)} onClick={() => switchMode("edit")} className="h-8 gap-1.5 text-xs" data-testid="office-enter-edit">
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
                    <div ref={reviewArea} className="relative flex h-[78vh] min-h-0 flex-col">
                        {target && (
                            <OfficePreviewPane
                                key={`${sourceKey}#${paneKey}`}
                                ref={paneRef}
                                title={target.title}
                                extension={target.extension}
                                source={target.source}
                                heightClassName="min-h-0 flex-1"
                                mode={mode}
                                requireEdit={Boolean(receiver) || mode === "edit"}
                                onSelectionChange={(value, point) => {
                                    setSelection(value);
                                    if (point && canAskAi && hasSelectionContent(value)) startSuggestion(value, point);
                                }}
                                onDirtyChange={setDirty}
                                onFileLoaded={setLoadedFile}
                                onViewerProtocol={setProtocol}
                                onSaveShortcut={() => void handleSaveCopy()}
                            />
                        )}

                        {canAskAi && supportsEdit && receiver && (
                            <div className="shrink-0 space-y-2 border-t px-4 py-2.5" data-testid="office-ai-panel">
                                <div className="flex items-center justify-between gap-2 text-xs">
                                    <span className="font-medium">AI 修改 <span className="font-normal text-muted-foreground">· {receiver.label}</span></span>
                                    {hasSelectionContent(selection) && (
                                        <Button size="sm" variant="ghost" className="h-6 text-xs" disabled={!!draft || sending || aiError?.stale}
                                            onClick={event => {
                                                const rect = event.currentTarget.getBoundingClientRect();
                                                startSuggestion(selection, { x: rect.left, y: rect.top });
                                            }}>给选区加建议</Button>
                                    )}
                                </div>
                                <div className="rounded-md border bg-background px-2.5 py-1.5 focus-within:border-ring">
                                    {suggestions.length > 0 && (
                                        <div className="flex max-h-20 flex-wrap gap-1.5 overflow-y-auto py-1">
                                            {suggestions.map(item => (
                                                <Badge key={item.id} variant="secondary" className="gap-1">
                                                    <button type="button" disabled={!!draft || sending} onClick={event => {
                                                        const rect = event.currentTarget.getBoundingClientRect();
                                                        editSuggestion(item, { x: rect.left, y: rect.top });
                                                    }}>建议 {item.id}</button>
                                                    <button type="button" className="aipp-icon-button" title={`删除建议 ${item.id}`}
                                                        disabled={!!draft || sending} onClick={() => setSuggestions(items => items.filter(s => s.id !== item.id))}>
                                                        <X className="size-3" />
                                                    </button>
                                                </Badge>
                                            ))}
                                        </div>
                                    )}
                                    <div className="flex items-end gap-2">
                                        <Textarea value={aiRequest} disabled={sending} onChange={e => setAiRequest(e.target.value)}
                                            aria-label="总意见" placeholder="写下总意见，或框选文档内容添加建议"
                                            onKeyDown={e => {
                                                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                                                    e.preventDefault();
                                                    void handleSendToAi();
                                                }
                                            }}
                                            className="min-h-10 max-h-24 resize-none border-0 bg-transparent px-0 py-1 shadow-none focus-visible:ring-0" />
                                        <Button size="sm" disabled={!!draft || sending || aiError?.stale || !loadedFile || !(aiRequest.trim() || suggestions.length)}
                                            onClick={() => void handleSendToAi()} data-testid="office-ai-send">
                                            {sending ? <Loader2 className="size-3.5 animate-spin" /> : <Sparkles className="size-3.5" />}发送
                                        </Button>
                                    </div>
                                </div>
                                {aiError && (
                                    <div role="alert" className="flex items-center justify-between gap-2 text-xs text-destructive" data-testid="office-ai-error">
                                        <span>{aiError.title}：{aiError.message}</span>
                                        {aiError.stale && <Button variant="outline" size="sm" onClick={reloadAfterExternalChange}>
                                            {suggestions.length ? "重新加载并清除旧选区" : "重新加载"}
                                        </Button>}
                                    </div>
                                )}
                            </div>
                        )}
                        {draft && canAskAi && (
                            <form role="dialog" aria-label={`建议 ${draft.id}`} onSubmit={e => { e.preventDefault(); confirmSuggestion(); }}
                                className="absolute z-10 flex max-h-full w-72 max-w-full flex-col gap-2 overflow-auto rounded-lg border bg-popover p-3 text-popover-foreground shadow-lg"
                                style={{
                                    left: `clamp(0px, ${draft.point.x + 6}px, 100% - min(18rem, 100%))`,
                                    ...(draft.point.y < (reviewArea.current?.clientHeight ?? 0) / 2
                                        ? { top: draft.point.y + 6 }
                                        : { bottom: Math.max(0, (reviewArea.current?.clientHeight ?? 0) - draft.point.y + 6) }),
                                }}>
                                <div className="text-xs font-medium">{describeSelectionLocation(draft.selection)}</div>
                                <pre className="max-h-16 overflow-auto whitespace-pre-wrap break-words border-l-2 pl-2 font-sans text-xs text-muted-foreground">{selectionPreviewText(draft.selection)}</pre>
                                <Textarea autoFocus aria-label="调整意见" placeholder="写下这处的调整意见" value={draft.comment}
                                    onChange={e => setDraft({ ...draft, comment: e.target.value })} className="min-h-14 max-h-24 resize-none" />
                                <div className="flex justify-end gap-2">
                                    <Button type="button" size="sm" variant="ghost" onClick={() => setDraft(null)}>取消</Button>
                                    <Button type="submit" size="sm" disabled={!draft.comment.trim()}>确定</Button>
                                </div>
                            </form>
                        )}
                    </div>
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
