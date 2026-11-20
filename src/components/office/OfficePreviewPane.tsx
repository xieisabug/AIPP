import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
    isOfficePreviewPluginReady,
    recentOfficePluginInstallFailure,
    type OfficePreviewPluginStatus,
    type PluginInstallProgressPayload,
} from "@/services/officePreviewPlugin";

export interface OfficePreviewPaneProps {
    title: string;
    source: OfficePreviewSource;
    className?: string;
    heightClassName?: string;
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

export const OfficePreviewPane: React.FC<OfficePreviewPaneProps> = ({
    title,
    source,
    className = "",
    heightClassName = "h-[68vh]",
}) => {
    const isMobilePlatform = useIsMobilePlatform();
    const ext = useMemo(
        () => detectOfficeExtension(title) ?? detectOfficeExtension(source.localPath),
        [title, source.localPath],
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
    const [filePayload, setFilePayload] = useState<OfficeFilePayload | null>(null);
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
    const pluginReady = isOfficePreviewPluginReady(pluginStatus);

    const sourceKey = `${source.attachmentId ?? ""}|${source.relayToken ?? ""}|${source.localPath ?? ""}`;

    const postToViewer = useCallback(
        (message: Record<string, unknown>, transfer?: Transferable[]) => {
            const win = iframeRef.current?.contentWindow;
            // 查看器未就绪时 iframe 仍是 about:blank（宿主源），发过去只会触发 origin 不匹配告警
            if (!win || !viewerReadyRef.current) return false;
            win.postMessage(message, targetOrigin, transfer);
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
            const status = await installOfficePreviewPlugin();
            setPluginStatus(status);
        } catch (err) {
            setInstallError(err instanceof Error ? err.message : String(err));
        } finally {
            setInstalling(false);
        }
    }, []);

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

    // 未安装时自动下载安装（每个面板只自动触发一次；冷却期内的失败不自动重试，由用户点重试）
    useEffect(() => {
        if (isLegacy || !pluginStatus || pluginStatus.installed || autoInstallTriedRef.current) return;
        autoInstallTriedRef.current = true;
        const recentFailure = recentOfficePluginInstallFailure();
        if (recentFailure) {
            setInstallError(recentFailure);
            return;
        }
        void runInstall();
    }, [isLegacy, pluginStatus, runInstall]);

    // 插件就绪后读取已授权文件
    useEffect(() => {
        if (isLegacy || !pluginReady) return;

        const seq = ++loadSeqRef.current;
        setLoadingFile(true);
        setRendering(false);
        setPreviewError(null);
        setFilePayload(null);
        postToViewer({ type: "aipp-office:unload" });

        invoke<ArrayBuffer>("read_authorized_office_file", { source })
            .then((raw) => {
                if (loadSeqRef.current !== seq) return;
                setFilePayload(parseOfficeFileResponse(raw));
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
            // 切换文件或关闭时作废进行中的读取，并通知查看器卸载文档、释放 Worker 内存
            loadSeqRef.current++;
            postToViewer({ type: "aipp-office:unload" });
        };
        // eslint-disable-next-line react-hooks/exhaustive-deps -- sourceKey 代表 source 的内容
    }, [isLegacy, pluginReady, sourceKey, reloadToken, postToViewer]);

    // 文件读取完成且查看器就绪后，把字节转交给 iframe 内的 Worker 渲染
    useEffect(() => {
        if (!filePayload || !viewerReady) return;
        const seq = loadSeqRef.current;
        if (sentSeqRef.current !== seq) {
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
    }, [filePayload, viewerReady, isMobilePlatform, postToViewer]);

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
            } | null;
            if (!data || typeof data.type !== "string") return;

            if (data.type === "aipp-office:viewer-ready") {
                viewerReadyRef.current = true;
                setViewerReady(true);
                return;
            }
            if (data.requestId !== loadSeqRef.current) return;

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
    if (pluginStatus.installed && !pluginStatus.enabled) {
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
                        {installError ? "Office 预览组件下载失败" : "首次预览需要下载 Office 预览组件"}
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
};

export default OfficePreviewPane;
