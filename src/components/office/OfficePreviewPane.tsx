import React, { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import {
    AlertTriangle,
    Download,
    ExternalLink,
    FileSpreadsheet,
    FileText,
    Loader2,
    Lock,
    Power,
    Presentation,
    RefreshCw,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import {
    detectOfficeExtension,
    isLegacyOfficeExtension,
    OFFICE_PREVIEW_PLUGIN_CODE,
    officeFormatLabel,
    type OfficeFilePayload,
    type OfficeExtension,
    type OfficePreviewSource,
    type ParsedOfficePreviewError,
    parseOfficeFileResponse,
    parseOfficePreviewError,
    resolveOfficeViewerUrl,
    viewerTargetOrigin,
} from "@/utils/officeFile";
import {
    enableOfficePreviewPlugin,
    getOfficePreviewPluginStatus,
    installOfficePreviewPlugin,
    isOfficeEditPluginReady,
    isOfficePreviewPluginReady,
    officePreviewPluginNeedsInstall,
    recentOfficePluginInstallFailure,
    type OfficePreviewPluginStatus,
    type PluginInstallProgressPayload,
} from "@/services/officePreviewPlugin";
import { isOfficeSelection, type OfficeSelection } from "@/utils/officeSelection";

export type OfficePaneMode = "view" | "edit";

export interface OfficeLoadedFile {
    fileName: string;
    format: OfficeFilePayload["format"];
    sha256: string;
    sizeBytes: number;
    copyOriginToken?: string;
}

export interface OfficePreviewPaneProps {
    title: string;
    extension?: OfficeExtension | null;
    source: OfficePreviewSource;
    className?: string;
    heightClassName?: string;
    /** view：只读预览（可选中内容交给 AI）；edit：手动编辑，保存由宿主发起（总是另存副本）。 */
    mode?: OfficePaneMode;
    /** 需要选区 / 编辑能力（协议 2）时为 true：旧版组件会自动更新。 */
    requireEdit?: boolean;
    onSelectionChange?: (selection: OfficeSelection | null) => void;
    onDirtyChange?: (dirty: boolean) => void;
    onFileLoaded?: (file: OfficeLoadedFile | null) => void;
    /** 查看器协议版本（2 起支持选区、编辑与 AI 修改）。 */
    onViewerProtocol?: (protocol: number) => void;
    /** 编辑器里按了 Ctrl/Cmd+S。 */
    onSaveShortcut?: () => void;
}

export interface OfficePreviewPaneHandle {
    /** 取当前编辑器里的文档字节（编辑模式）。 */
    requestSave: () => Promise<ArrayBuffer>;
}

/** 渲染超时：BetterOffice DOCX 冷启动在 Android 约 3 秒，45 秒仍无回执视为卡死。 */
const RENDER_WATCHDOG_MS = 45_000;

export const OfficeFormatIcon: React.FC<{ ext?: string | null; className?: string }> = ({
    ext,
    className = "h-4 w-4 shrink-0 text-muted-foreground",
}) => {
    switch (ext) {
        case "xlsx":
        case "xls":
            return <FileSpreadsheet className={className} />;
        case "pptx":
        case "ppt":
            return <Presentation className={className} />;
        default:
            return <FileText className={className} />;
    }
};

function formatMiB(bytes?: number | null): string {
    if (bytes == null) return "";
    return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

export const OfficePreviewPane = forwardRef<OfficePreviewPaneHandle, OfficePreviewPaneProps>(function OfficePreviewPane({
    title,
    extension,
    source,
    className = "",
    heightClassName = "h-[68vh]",
    mode = "view",
    requireEdit = false,
    onSelectionChange,
    onDirtyChange,
    onFileLoaded,
    onViewerProtocol,
    onSaveShortcut,
}, ref) {
    const isMobilePlatform = useIsMobilePlatform();
    const ext = useMemo(
        () => extension ?? detectOfficeExtension(title) ?? detectOfficeExtension(source.localPath),
        [extension, title, source.localPath],
    );
    const isLegacy = isLegacyOfficeExtension(ext);

    const [pluginStatus, setPluginStatus] = useState<OfficePreviewPluginStatus | null>(null);
    const [statusError, setStatusError] = useState<string | null>(null);
    const [installing, setInstalling] = useState(false);
    const [installProgress, setInstallProgress] = useState<PluginInstallProgressPayload | null>(null);
    const [installError, setInstallError] = useState<string | null>(null);
    const [enabling, setEnabling] = useState(false);

    const [loadingFile, setLoadingFile] = useState(false);
    const [rendering, setRendering] = useState(false);
    /** 读取到的文件与它所属的读取序号：只发送当前这次读取的字节（字节转移给查看器后即失效）。 */
    const [filePayload, setFilePayload] = useState<(OfficeFilePayload & { seq: number }) | null>(null);
    const [previewError, setPreviewError] = useState<ParsedOfficePreviewError | null>(null);
    const [viewerReady, setViewerReady] = useState(false);
    const [openingExternal, setOpeningExternal] = useState(false);
    const [reloadToken, setReloadToken] = useState(0);

    const iframeRef = useRef<HTMLIFrameElement | null>(null);
    // 错误页会卸载 iframe；重新挂载的新 iframe 要重新发 viewer-ready 才能接收文档
    const setIframe = useCallback((node: HTMLIFrameElement | null) => {
        iframeRef.current = node;
        if (!node) {
            viewerReadyRef.current = false;
            setViewerReady(false);
        }
    }, []);
    const loadSeqRef = useRef(0);
    const sentSeqRef = useRef(-1);
    /** 已收到 loaded/error 回执的 requestId；看门狗只在仍未收到回执时报超时。 */
    const settledSeqRef = useRef(-1);
    const viewerReadyRef = useRef(false);
    const autoInstallTriedRef = useRef(false);
    const viewerUrl = useMemo(() => resolveOfficeViewerUrl(), []);
    const targetOrigin = useMemo(() => viewerTargetOrigin(viewerUrl), [viewerUrl]);
    const needEdit = requireEdit || mode === "edit";
    const pluginReady = needEdit ? isOfficeEditPluginReady(pluginStatus) : isOfficePreviewPluginReady(pluginStatus);
    const pluginNeedsInstall =
        officePreviewPluginNeedsInstall(pluginStatus)
        || Boolean(needEdit && pluginStatus?.installed && pluginStatus.enabled && !pluginStatus.editSupported);

    const sourceKey = `${source.refId ?? ""}|${source.attachmentId ?? ""}|${source.relayToken ?? ""}|${source.localPath ?? ""}`;
    const callbacksRef = useRef({ onSelectionChange, onDirtyChange, onFileLoaded, onViewerProtocol, onSaveShortcut });
    callbacksRef.current = { onSelectionChange, onDirtyChange, onFileLoaded, onViewerProtocol, onSaveShortcut };
    const pendingSavesRef = useRef(new Map<number, { resolve: (buf: ArrayBuffer) => void; reject: (err: Error) => void }>());

    const postToViewer = useCallback(
        (message: Record<string, unknown>, transfer?: Transferable[]) => {
            const win = iframeRef.current?.contentWindow;
            // 查看器未就绪时 iframe 仍是 about:blank（宿主源），发过去只会触发 origin 不匹配告警
            if (!win || !viewerReadyRef.current) return false;
            try {
                win.postMessage(message, targetOrigin, transfer);
            } catch (err) {
                // 例如转移已失效的 ArrayBuffer：不让异常冒出 effect，按未发送处理
                console.warn("[OfficePreviewPane] postMessage to viewer failed:", err);
                return false;
            }
            return true;
        },
        [targetOrigin],
    );

    const refreshStatus = useCallback(async () => {
        try {
            const status = await getOfficePreviewPluginStatus();
            setPluginStatus(status);
            setStatusError(null);
            return status;
        } catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            console.error("[OfficePreviewPane] get_office_preview_plugin_status failed:", err);
            setStatusError(msg);
            return null;
        }
    }, []);

    const runInstall = useCallback(async () => {
        setInstalling(true);
        setInstallError(null);
        setInstallProgress((prev) =>
            prev?.stage === "downloading"
                ? prev
                : {
                      pluginCode: OFFICE_PREVIEW_PLUGIN_CODE,
                      stage: "downloading",
                      downloadedBytes: 0,
                      totalBytes: null,
                      percent: null,
                      message: "正在准备下载 Office 预览组件...",
                  },
        );
        try {
            const status = await installOfficePreviewPlugin({ requireEdit: needEdit });
            setPluginStatus(status);
        } catch (err) {
            setInstallError(err instanceof Error ? err.message : String(err));
        } finally {
            setInstalling(false);
        }
    }, [needEdit]);

    useEffect(() => {
        void refreshStatus();
    }, [refreshStatus]);

    useEffect(() => {
        let mounted = true;
        const unlisteners: Array<() => void> = [];
        const track = (p: Promise<() => void>) =>
            void p.then((fn) => {
                if (mounted) unlisteners.push(fn);
                else fn();
            });

        track(
            listen<PluginInstallProgressPayload>("plugin_install_progress", (event) => {
                if (event.payload?.pluginCode === OFFICE_PREVIEW_PLUGIN_CODE) {
                    setInstallProgress(event.payload);
                }
            }),
        );
        // 插件在插件中心被安装/停用/卸载时同步状态
        track(listen("plugin_registry_changed", () => void refreshStatus()));

        return () => {
            mounted = false;
            unlisteners.forEach((fn) => fn());
        };
    }, [refreshStatus]);

    // 需要编辑能力时（进入编辑、选区），旧版组件再自动更新一次。必须在下面的自动安装之前执行
    useEffect(() => {
        if (needEdit) autoInstallTriedRef.current = false;
    }, [needEdit]);

    // 未安装时自动下载安装（每个面板只自动触发一次；冷却期内的失败不自动重试，由用户点重试）
    useEffect(() => {
        if (isLegacy || !pluginNeedsInstall || autoInstallTriedRef.current) return;
        autoInstallTriedRef.current = true;
        const recentFailure = recentOfficePluginInstallFailure();
        if (recentFailure) {
            setInstallError(recentFailure);
            return;
        }
        void runInstall();
    }, [isLegacy, pluginNeedsInstall, runInstall]);

    // 插件就绪后读取已授权文件
    useEffect(() => {
        if (isLegacy || !pluginReady) return;

        const seq = ++loadSeqRef.current;
        setLoadingFile(true);
        setRendering(false);
        setPreviewError(null);
        setFilePayload(null);
        callbacksRef.current.onSelectionChange?.(null);
        callbacksRef.current.onDirtyChange?.(false);
        callbacksRef.current.onFileLoaded?.(null);
        postToViewer({ type: "aipp-office:unload" });

        const request = !isMobilePlatform && mode === "edit"
            ? { source, bindCopySource: true }
            : { source };
        let copyOriginToken: string | undefined;
        const releaseOrigin = (token?: string) => {
            if (token) void invoke("release_office_copy_origin", { token }).catch(console.warn);
        };
        invoke<ArrayBuffer>("read_authorized_office_file", request)
            .then((raw) => {
                const payload = parseOfficeFileResponse(raw);
                if (loadSeqRef.current !== seq) {
                    releaseOrigin(payload.copyOriginToken);
                    return;
                }
                copyOriginToken = payload.copyOriginToken;
                setFilePayload({ ...payload, seq });
                callbacksRef.current.onFileLoaded?.({
                    fileName: payload.fileName,
                    format: payload.format,
                    sha256: payload.sha256,
                    sizeBytes: payload.sizeBytes,
                    ...(copyOriginToken ? { copyOriginToken } : {}),
                });
                setLoadingFile(false);
                setRendering(true);
            })
            .catch((err) => {
                if (loadSeqRef.current !== seq) return;
                setLoadingFile(false);
                setRendering(false);
                setPreviewError(parseOfficePreviewError(err));
            });

        return () => {
            // 切换文件 / 模式或关闭时作废进行中的读取，并通知查看器卸载文档、释放 Worker 内存
            loadSeqRef.current++;
            releaseOrigin(copyOriginToken);
            postToViewer({ type: "aipp-office:unload" });
            pendingSavesRef.current.forEach((pending) => pending.reject(new Error("文档已关闭或重新加载，保存已取消")));
            pendingSavesRef.current.clear();
        };
        // eslint-disable-next-line react-hooks/exhaustive-deps -- sourceKey 代表 source 的内容
    }, [isLegacy, pluginReady, sourceKey, reloadToken, postToViewer, mode, isMobilePlatform]);

    // 文件读取完成且查看器就绪后，把字节转交给 iframe 内的 Worker 渲染
    useEffect(() => {
        if (!filePayload || !viewerReady) return;
        const seq = loadSeqRef.current;
        // 模式切换等触发重新读取时，同一次提交里旧 payload 还在：它的字节可能已转移，不能再发
        if (filePayload.seq !== seq) return;
        if (sentSeqRef.current !== seq) {
            if (filePayload.bytes.byteLength === 0 && filePayload.sizeBytes > 0) return;
            // 转移 ArrayBuffer 所有权（零拷贝），宿主侧不再保留文档字节
            const bytes = filePayload.bytes;
            const sent = postToViewer(
                {
                    type: "aipp-office:load",
                    requestId: seq,
                    format: filePayload.format,
                    fileName: filePayload.fileName,
                    bytes,
                    isMobile: isMobilePlatform || window.innerWidth < 768,
                    colorMode: document.documentElement.classList.contains("dark") ? "dark" : "light",
                    mode,
                },
                [bytes],
            );
            if (!sent) return;
            sentSeqRef.current = seq;
        }
        if (settledSeqRef.current === seq) return;
        setRendering(true);

        const timer = window.setTimeout(() => {
            if (loadSeqRef.current !== seq || settledSeqRef.current === seq) return;
            setRendering(false);
            setPreviewError({
                code: "render_timeout",
                title: "文档渲染超时",
                message: "后台渲染超过 45 秒仍未完成（文件可能过于复杂或设备内存不足），可重试或用外部应用打开。",
            });
        }, RENDER_WATCHDOG_MS);

        return () => window.clearTimeout(timer);
    }, [filePayload, viewerReady, isMobilePlatform, postToViewer, mode]);

    // 只接收本面板 iframe 发来的消息；过期 requestId 的结果直接丢弃
    useEffect(() => {
        const onMessage = (event: MessageEvent) => {
            const frameWindow = iframeRef.current?.contentWindow;
            if (!frameWindow || event.source !== frameWindow) return;
            const data = event.data as {
                type?: string;
                requestId?: number;
                code?: string;
                message?: string;
                protocol?: number;
                selection?: unknown;
                bytes?: unknown;
            } | null;
            if (!data || typeof data.type !== "string") return;

            if (data.type === "aipp-office:viewer-ready") {
                viewerReadyRef.current = true;
                setViewerReady(true);
                callbacksRef.current.onViewerProtocol?.(typeof data.protocol === "number" ? data.protocol : 1);
                return;
            }
            if (data.type === "aipp-office:saved" || data.type === "aipp-office:save-error") {
                const pending = pendingSavesRef.current.get(data.requestId ?? -1);
                if (!pending) return;
                pendingSavesRef.current.delete(data.requestId ?? -1);
                if (data.type === "aipp-office:saved" && data.bytes instanceof ArrayBuffer) pending.resolve(data.bytes);
                else pending.reject(new Error(data.message || "编辑器没有返回文件内容"));
                return;
            }
            if (data.requestId !== loadSeqRef.current) return;

            if (data.type === "aipp-office:selection") {
                callbacksRef.current.onSelectionChange?.(isOfficeSelection(data.selection) ? data.selection : null);
                return;
            }
            if (data.type === "aipp-office:dirty") {
                callbacksRef.current.onDirtyChange?.(true);
                return;
            }
            if (data.type === "aipp-office:save-shortcut") {
                callbacksRef.current.onSaveShortcut?.();
                return;
            }

            if (data.type === "aipp-office:loaded") {
                settledSeqRef.current = data.requestId;
                setRendering(false);
                setPreviewError(null);
            } else if (data.type === "aipp-office:error") {
                settledSeqRef.current = data.requestId;
                setRendering(false);
                setPreviewError(
                    parseOfficePreviewError(
                        `OFFICE_PREVIEW_ERROR:${data.code || "render_error"}:${data.message || "查看器未提供错误详情"}`,
                    ),
                );
            }
        };
        window.addEventListener("message", onMessage);
        return () => window.removeEventListener("message", onMessage);
    }, []);

    useImperativeHandle(ref, () => ({
        requestSave: () =>
            new Promise<ArrayBuffer>((resolve, reject) => {
                const requestId = loadSeqRef.current;
                if (settledSeqRef.current !== requestId) {
                    reject(new Error("文档还没有加载完成"));
                    return;
                }
                pendingSavesRef.current.set(requestId, { resolve, reject });
                if (!postToViewer({ type: "aipp-office:save-request", requestId })) {
                    pendingSavesRef.current.delete(requestId);
                    reject(new Error("Office 组件尚未就绪"));
                }
            }),
    }), [postToViewer]);

    const handleEnable = async () => {
        setEnabling(true);
        try {
            setPluginStatus(await enableOfficePreviewPlugin());
        } catch (err) {
            toast.error(`启用 Office 预览组件失败：${err instanceof Error ? err.message : String(err)}`);
        } finally {
            setEnabling(false);
        }
    };

    const handleOpenExternally = async () => {
        setOpeningExternal(true);
        try {
            await invoke("open_office_file_externally", { source });
        } catch (err) {
            toast.error(`调用外部应用失败：${err instanceof Error ? err.message : String(err)}`);
        } finally {
            setOpeningExternal(false);
        }
    };

    const handleRetry = () => {
        setPreviewError(null);
        setReloadToken((n) => n + 1);
    };

    const externalButton = (variant: "default" | "outline" | "ghost") => (
        <Button
            type="button"
            variant={variant}
            size="sm"
            onClick={() => void handleOpenExternally()}
            disabled={openingExternal}
            className="min-h-9 gap-1.5"
        >
            {openingExternal ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <ExternalLink className="h-3.5 w-3.5" />}
            用外部应用打开
        </Button>
    );

    const shell = `flex flex-col items-center justify-center gap-3 px-6 py-10 text-center ${heightClassName} ${className}`;

    // 旧版二进制格式：不需要插件，直接提示
    if (isLegacy) {
        return (
            <div className={`${shell} bg-muted/20`} data-testid="office-preview-error" data-error-code="unsupported_legacy_format">
                <AlertTriangle className="h-5 w-5 text-muted-foreground" />
                <div className="max-w-md space-y-1">
                    <div className="text-sm font-medium text-foreground">暂不支持旧版二进制 Office 格式</div>
                    <div className="text-xs leading-5 text-muted-foreground">
                        当前文件为 .{ext} 格式，应用内预览仅支持 .docx / .xlsx / .pptx，请用外部办公软件打开或另存为新版格式。
                    </div>
                </div>
                {externalButton("default")}
            </div>
        );
    }

    if (previewError) {
        const isEncrypted = previewError.code === "password_encrypted";
        return (
            <div className={`${shell} bg-muted/20`} data-testid="office-preview-error" data-error-code={previewError.code}>
                {isEncrypted ? (
                    <Lock className="h-5 w-5 text-muted-foreground" />
                ) : (
                    <AlertTriangle className="h-5 w-5 text-muted-foreground" />
                )}
                <div className="max-w-md space-y-1">
                    <div className="text-sm font-medium text-foreground">{previewError.title}</div>
                    <div className="break-words text-xs leading-5 text-muted-foreground">{previewError.message}</div>
                </div>
                <div className="flex flex-wrap items-center justify-center gap-2 pt-1">
                    <Button type="button" variant="outline" size="sm" onClick={handleRetry} className="min-h-9 gap-1.5">
                        <RefreshCw className="h-3.5 w-3.5" />
                        重试
                    </Button>
                    {externalButton("default")}
                </div>
            </div>
        );
    }

    if (!pluginStatus) {
        return (
            <div className={`${shell} bg-muted/10`}>
                {statusError ? (
                    <>
                        <AlertTriangle className="h-5 w-5 text-muted-foreground" />
                        <div className="max-w-md break-words text-xs text-muted-foreground">
                            检查 Office 预览组件状态失败：{statusError}
                        </div>
                        <div className="flex flex-wrap items-center justify-center gap-2">
                            <Button type="button" variant="outline" size="sm" onClick={() => void refreshStatus()} className="min-h-9 gap-1.5">
                                <RefreshCw className="h-3.5 w-3.5" />
                                重试
                            </Button>
                            {externalButton("ghost")}
                        </div>
                    </>
                ) : (
                    <>
                        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
                        <span className="text-xs text-muted-foreground">正在检查 Office 预览组件...</span>
                    </>
                )}
            </div>
        );
    }

    // 已安装但被用户在插件中心停用：尊重用户选择，不自动启用
    if (pluginStatus.installed && !pluginStatus.enabled && !pluginStatus.updateRequired) {
        return (
            <div className={`${shell} bg-muted/15`} data-testid="office-plugin-disabled">
                <OfficeFormatIcon ext={ext} className="h-6 w-6 text-muted-foreground" />
                <div className="max-w-md space-y-1">
                    <div className="text-sm font-medium text-foreground">Office 预览组件已停用</div>
                    <div className="text-xs leading-5 text-muted-foreground">
                        启用后即可在应用内查看 {officeFormatLabel(ext)}。
                    </div>
                </div>
                <div className="flex flex-wrap items-center justify-center gap-2">
                    <Button type="button" size="sm" onClick={() => void handleEnable()} disabled={enabling} className="min-h-9 gap-1.5">
                        {enabling ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Power className="h-3.5 w-3.5" />}
                        启用并预览
                    </Button>
                    {externalButton("ghost")}
                </div>
            </div>
        );
    }

    // 未安装（或安装不完整）：自动下载，展示进度；失败时可重试
    if (!pluginReady) {
        const pct = installProgress?.percent ?? null;
        const sizeText =
            installProgress?.stage === "downloading" && installProgress.totalBytes
                ? `${formatMiB(installProgress.downloadedBytes)} / ${formatMiB(installProgress.totalBytes)}`
                : null;
        return (
            <div className={`${shell} bg-muted/15`} data-testid="office-plugin-install-prompt">
                <OfficeFormatIcon ext={ext} className="h-6 w-6 text-muted-foreground" />
                <div className="max-w-md space-y-1">
                    <div className="text-sm font-medium text-foreground">
                        {installError
                            ? "Office 预览组件下载失败"
                            : pluginStatus.updateRequired || (pluginStatus.installed && needEdit)
                              ? needEdit && !pluginStatus.updateRequired
                                  ? "正在更新 Office 组件以支持编辑与 AI 修改"
                                  : "正在更新 Office 预览组件"
                              : "首次预览需要下载 Office 预览组件"}
                    </div>
                    <div className="text-xs leading-5 text-muted-foreground">
                        组件约 42 MB，只需下载一次；文档全程在本机解析，不会上传。
                    </div>
                </div>

                {installError ? (
                    <div className="max-w-md break-words text-xs text-muted-foreground" data-testid="office-plugin-install-error">
                        {installError}
                    </div>
                ) : (
                    <div className="w-full max-w-sm space-y-2" data-testid="office-plugin-install-progress">
                        {pct != null ? (
                            <Progress value={pct} className="h-1.5" />
                        ) : (
                            <div className="flex justify-center">
                                <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                            </div>
                        )}
                        <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
                            <span className="truncate">{installProgress?.message || "正在准备下载..."}</span>
                            <span className="shrink-0 tabular-nums">{sizeText ?? (pct != null ? `${pct}%` : "")}</span>
                        </div>
                    </div>
                )}

                <div className="flex flex-wrap items-center justify-center gap-2 pt-1">
                    {installError && (
                        <Button type="button" size="sm" disabled={installing} onClick={() => void runInstall()} className="min-h-9 gap-1.5">
                            <Download className="h-3.5 w-3.5" />
                            重新下载
                        </Button>
                    )}
                    {externalButton(installError ? "outline" : "ghost")}
                </div>
            </div>
        );
    }

    return (
        <div className={`relative flex flex-col overflow-hidden bg-background ${heightClassName} ${className}`}>
            {(loadingFile || rendering) && (
                <div
                    className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 bg-background/85"
                    data-testid="office-preview-loading"
                >
                    <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
                    <span className="text-xs text-muted-foreground">{loadingFile ? "正在读取文档..." : "正在渲染文档..."}</span>
                </div>
            )}
            <iframe
                ref={setIframe}
                src={viewerUrl}
                title={title || "Office 文档预览"}
                className="block h-full w-full flex-1 border-0 bg-background"
            />
        </div>
    );
});

export default OfficePreviewPane;
