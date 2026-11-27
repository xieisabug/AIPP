import { Fragment, ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { toast } from "sonner";
import {
    AlertTriangle,
    ChevronDown,
    Copy,
    Download,
    ExternalLink,
    Loader2,
    Maximize2,
    Minimize2,
    PanelTopOpen,
    RotateCw,
    Wand2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import ArtifactPreviewCodeBlock from "@/components/ArtifactPreviewCodeBlock";
import { useArtifactBridge } from "@/hooks/useArtifactBridge";
import type { LogLine } from "@/hooks/useArtifactEvents";
import type { ArtifactBridgeConfig } from "@/data/ArtifactCollection";
import { cn } from "@/utils/utils";
import MermaidPreview from "./MermaidPreview";
import MarkdownPreview from "./MarkdownPreview";
import DrawioPreview from "./DrawioPreview";
import ProjectFrame, { readProjectFile, type ProjectFiles } from "./ProjectFrame";
import {
    ArtifactConsoleEntry,
    ArtifactPreviewType,
    buildFixReport,
    codeLanguageFor,
    codeLanguageForFile,
    injectConsoleScript,
    needsDevServer,
    previewTypeLabel,
    readRuntimeConsoleMessage,
    sourceFileExtension,
} from "./artifactRuntime";

export type ArtifactViewerView = "preview" | "code" | "console";

export interface ArtifactViewerProps {
    type: ArtifactPreviewType | null;
    code: string;
    /** React/Vue 的本机预览地址；为空表示服务器还在启动。 */
    previewUrl?: string | null;
    /** 每次载入新内容时变化：重建预览并清空运行时控制台。 */
    contentKey: string | number;
    title?: string;
    /** 后端构建/启动日志。 */
    buildLogs: LogLine[];
    bridgeConfig?: ArtifactBridgeConfig;
    /** 侧栏等窄空间：工具栏只留图标。 */
    compact?: boolean;
    /** 宿主专属按钮（保存、临时配置等），放在工具栏右侧。 */
    actions?: ReactNode;
    /** 替代默认预览（比选快照等引用型内容）。 */
    previewOverride?: ReactNode;
    /** 加载中界面顶部的内容（收藏项的图标与名称）。 */
    loadingHeader?: ReactNode;
    /** 把报错交给 AI；返回 false 表示没有界面接收。不传则不显示该操作。 */
    onFixWithAI?: (report: string) => Promise<boolean>;
    /** 在独立窗口打开（侧栏使用）。 */
    onOpenInWindow?: () => void;
    /** Draw.io 编辑后的最新内容。 */
    onCodeChange?: (code: string) => void;
    /** 多文件版本的全部文件（`type === "project"` 时使用）与入口。 */
    projectFiles?: ProjectFiles | null;
    entryFile?: string;
    /** 会话 Artifact 的版本：多于一个时工具栏可切换。 */
    versions?: { version: number; created_at: string }[];
    currentVersion?: number;
    onSelectVersion?: (version: number) => void;
}

function formatVersionTime(value: string): string {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return value;
    return date.toLocaleString(undefined, { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

const MAX_RUNTIME_ENTRIES = 500;

function buildLogLevel(type: LogLine["type"]): ArtifactConsoleEntry["level"] {
    return type === "error" ? "error" : type === "success" ? "success" : "log";
}

/**
 * Artifact 统一查看器：预览窗口、侧栏与收藏窗口共用。
 * 负责渲染各类型内容、代码视图、控制台（构建日志 + 页面运行时输出）与报错交给 AI。
 */
export default function ArtifactViewer({
    type,
    code,
    previewUrl,
    contentKey,
    title,
    buildLogs,
    bridgeConfig,
    compact = false,
    actions,
    previewOverride,
    loadingHeader,
    onFixWithAI,
    onOpenInWindow,
    onCodeChange,
    projectFiles,
    entryFile,
    versions = [],
    currentVersion,
    onSelectVersion,
}: ArtifactViewerProps) {
    const rootRef = useRef<HTMLDivElement | null>(null);
    const previewAreaRef = useRef<HTMLDivElement | null>(null);
    const urlIframeRef = useRef<HTMLIFrameElement | null>(null);
    const [view, setView] = useState<ArtifactViewerView>("preview");
    const [runtimeEntries, setRuntimeEntries] = useState<ArtifactConsoleEntry[]>([]);
    const [reloadNonce, setReloadNonce] = useState(0);
    const [isFullscreen, setIsFullscreen] = useState(false);
    const [mermaidSvg, setMermaidSvg] = useState<string | null>(null);
    const [isSendingFix, setIsSendingFix] = useState(false);
    const [codeFile, setCodeFile] = useState<string | null>(null);

    const { sendConfig } = useArtifactBridge({
        iframeRef: urlIframeRef,
        config: bridgeConfig ?? {},
        allowedOrigins: ["http://localhost", "http://127.0.0.1"],
    });

    useEffect(() => {
        sendConfig();
    }, [sendConfig, bridgeConfig?.db_id, bridgeConfig?.assistant_id]);

    // 新内容：回到预览、清空运行时输出
    useEffect(() => {
        setRuntimeEntries([]);
        setMermaidSvg(null);
        setView("preview");
        setCodeFile(null);
    }, [contentKey]);

    // 只接收本查看器内 iframe 里的诊断脚本消息
    useEffect(() => {
        const handleMessage = (event: MessageEvent) => {
            const entry = readRuntimeConsoleMessage(event.data);
            if (!entry) return;
            const frames = previewAreaRef.current?.querySelectorAll("iframe") ?? [];
            const fromOwnFrame = Array.from(frames).some((frame) => frame.contentWindow === event.source);
            if (!fromOwnFrame) return;
            setRuntimeEntries((prev) =>
                prev.length >= MAX_RUNTIME_ENTRIES ? [...prev.slice(1), entry] : [...prev, entry],
            );
        };
        window.addEventListener("message", handleMessage);
        return () => window.removeEventListener("message", handleMessage);
    }, []);

    useEffect(() => {
        const handleChange = () => setIsFullscreen(document.fullscreenElement === rootRef.current);
        document.addEventListener("fullscreenchange", handleChange);
        return () => document.removeEventListener("fullscreenchange", handleChange);
    }, []);

    const consoleEntries = useMemo<ArtifactConsoleEntry[]>(
        () => [
            ...buildLogs.map((log, index) => ({
                level: buildLogLevel(log.type),
                message: log.message,
                source: "build" as const,
                time: index,
            })),
            ...runtimeEntries,
        ],
        [buildLogs, runtimeEntries],
    );
    const errorCount = useMemo(() => consoleEntries.filter((entry) => entry.level === "error").length, [consoleEntries]);
    const runtimeErrorCount = useMemo(
        () => runtimeEntries.filter((entry) => entry.level === "error").length,
        [runtimeEntries],
    );

    const htmlDocument = useMemo(
        () => (type === "html" || type === "svg" || type === "xml" ? injectConsoleScript(code) : ""),
        [type, code],
    );

    const isWaitingForServer = needsDevServer(type) && !previewUrl;
    const isWaitingForFiles = type === "project" && !projectFiles;
    const isReady = type !== null && !isWaitingForServer && !isWaitingForFiles;
    const projectFileNames = useMemo(() => (projectFiles ? Object.keys(projectFiles).sort() : []), [projectFiles]);
    const shownCodeFile = codeFile ?? entryFile ?? projectFileNames[0] ?? null;

    const reload = useCallback(() => {
        setRuntimeEntries([]);
        setReloadNonce((n) => n + 1);
    }, []);

    const toggleFullscreen = useCallback(async () => {
        try {
            if (document.fullscreenElement) {
                await document.exitFullscreen();
            } else {
                await rootRef.current?.requestFullscreen();
            }
        } catch (error) {
            toast.error(`无法切换全屏：${String(error)}`);
        }
    }, []);

    const copyCode = useCallback(async () => {
        try {
            await writeText(code);
            toast.success("代码已复制");
        } catch (error) {
            toast.error(`复制失败：${String(error)}`);
        }
    }, [code]);

    const saveToFile = useCallback(
        async (content: string, extension: string, label: string) => {
            try {
                const baseName = (title || "artifact").replace(/[\\/:*?"<>|]/g, "_").slice(0, 80) || "artifact";
                const filePath = await save({
                    defaultPath: `${baseName}.${extension}`,
                    filters: [{ name: label, extensions: [extension] }],
                });
                if (!filePath) return;
                await writeFile(filePath, new TextEncoder().encode(content));
                toast.success(`已保存到 ${filePath}`);
            } catch (error) {
                toast.error(`保存失败：${String(error)}`);
            }
        },
        [title],
    );

    const fixReport = useMemo(
        () => buildFixReport({ title, type, entries: consoleEntries }),
        [title, type, consoleEntries],
    );

    const sendFix = useCallback(async () => {
        if (!onFixWithAI || !fixReport) return;
        setIsSendingFix(true);
        try {
            const delivered = await onFixWithAI(fixReport);
            if (delivered) {
                toast.success("已把报错填入会话输入框");
            } else {
                toast.error("没有打开该 Artifact 所在的会话，请先打开原会话再试");
            }
        } finally {
            setIsSendingFix(false);
        }
    }, [onFixWithAI, fixReport]);

    const renderLoading = () => {
        const recent = buildLogs.slice(-6);
        const lastError = [...buildLogs].reverse().find((log) => log.type === "error");
        return (
            <div className="flex-1 flex flex-col items-center justify-center gap-4 p-6 text-center">
                {loadingHeader}
                {lastError ? (
                    <AlertTriangle className="h-6 w-6 text-destructive" />
                ) : (
                    <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                )}
                <div className="w-full max-w-xl space-y-1 text-sm">
                    {recent.length === 0 ? (
                        <p className="text-muted-foreground">{type === null ? "等待预览数据…" : "正在启动预览…"}</p>
                    ) : (
                        recent.map((log, index) => (
                            <p
                                key={index}
                                className={cn(
                                    "break-words",
                                    log.type === "error" ? "text-destructive" : "text-muted-foreground",
                                    index === recent.length - 1 && log.type !== "error" && "text-foreground",
                                )}
                            >
                                {log.message}
                            </p>
                        ))
                    )}
                </div>
                {buildLogs.length > recent.length && (
                    <Button variant="ghost" size="sm" onClick={() => setView("console")}>
                        查看完整日志
                    </Button>
                )}
            </div>
        );
    };

    const renderPreview = () => {
        if (previewOverride) return <Fragment key={reloadNonce}>{previewOverride}</Fragment>;
        if (!isReady) return renderLoading();
        switch (type) {
            case "mermaid":
                return (
                    <MermaidPreview
                        key={contentKey}
                        code={code}
                        onRendered={(result) => {
                            if ("svg" in result) {
                                setMermaidSvg(result.svg);
                            } else {
                                setMermaidSvg(null);
                                setRuntimeEntries((prev) => [
                                    ...prev,
                                    { level: "error", message: result.error, source: "runtime", time: Date.now() },
                                ]);
                            }
                        }}
                    />
                );
            case "markdown":
            case "md":
                return <MarkdownPreview code={code} />;
            case "drawio":
                return (
                    <DrawioPreview
                        key={contentKey}
                        xml={code}
                        onChange={onCodeChange}
                        onSave={(xml) => void saveToFile(xml, "drawio", "Draw.io")}
                    />
                );
            case "html":
            case "svg":
            case "xml":
                return (
                    <iframe
                        key={`${contentKey}-${reloadNonce}`}
                        srcDoc={htmlDocument}
                        className="flex-1 w-full min-h-0 border-0 bg-white"
                        sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                        title={title || "Artifact"}
                    />
                );
            case "project":
                return (
                    <ProjectFrame
                        key={`${contentKey}-${reloadNonce}`}
                        title={title || "Artifact"}
                        files={projectFiles!}
                        entry={entryFile ?? projectFileNames[0] ?? ""}
                    />
                );
            case "react":
            case "vue":
                return (
                    <iframe
                        key={`${contentKey}-${reloadNonce}`}
                        ref={urlIframeRef}
                        src={previewUrl ?? undefined}
                        className="flex-1 w-full min-h-0 border-0 bg-white"
                        sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                        title={title || "Artifact"}
                        onLoad={() => setTimeout(sendConfig, 100)}
                    />
                );
            default:
                return renderLoading();
        }
    };

    const renderConsole = () => (
        <div className="flex-1 flex flex-col min-h-0">
            <div className="flex items-center justify-between gap-2 border-b border-border px-3 py-1.5 text-xs text-muted-foreground">
                <span>
                    {errorCount > 0 ? `${errorCount} 个错误` : "没有错误"} · 共 {consoleEntries.length} 条
                </span>
                <div className="flex items-center gap-1">
                    {onFixWithAI && fixReport && (
                        <Button variant="outline" size="sm" onClick={() => void sendFix()} disabled={isSendingFix}>
                            <Wand2 className="h-3.5 w-3.5" />
                            让 AI 修复
                        </Button>
                    )}
                    <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => setRuntimeEntries([])}
                        disabled={runtimeEntries.length === 0}
                    >
                        清空运行输出
                    </Button>
                </div>
            </div>
            <div className="flex-1 overflow-auto p-2 font-mono text-xs leading-5">
                {consoleEntries.length === 0 ? (
                    <p className="p-2 text-muted-foreground">暂无输出</p>
                ) : (
                    consoleEntries.map((entry, index) => (
                        <div
                            key={index}
                            className={cn(
                                "flex gap-2 border-b border-border/50 px-1 py-0.5 whitespace-pre-wrap break-words",
                                entry.level === "error" && "text-destructive",
                                entry.level === "warn" && "text-foreground font-medium",
                                (entry.level === "debug" || entry.level === "log" || entry.level === "info") &&
                                    entry.source === "build" &&
                                    "text-muted-foreground",
                            )}
                        >
                            <span className="w-10 shrink-0 select-none text-muted-foreground">
                                {entry.source === "build" ? "构建" : entry.level === "warn" ? "警告" : "页面"}
                            </span>
                            <span className="min-w-0 flex-1">{entry.message}</span>
                        </div>
                    ))
                )}
            </div>
        </div>
    );

    const renderCode = () => {
        const projectFile = projectFiles && shownCodeFile ? shownCodeFile : null;
        return (
            <div className="flex-1 flex flex-col min-h-0">
                {projectFiles && (
                    <div className="flex items-center gap-2 border-b border-border px-3 py-1.5 text-xs text-muted-foreground">
                        <span>文件</span>
                        <select
                            aria-label="工程文件"
                            className="h-7 max-w-full rounded-md border border-border bg-background px-2 text-xs text-foreground"
                            value={shownCodeFile ?? ""}
                            onChange={(event) => setCodeFile(event.target.value)}
                        >
                            {projectFileNames.map((name) => (
                                <option key={name} value={name}>
                                    {name === entryFile ? `${name}（入口）` : name}
                                </option>
                            ))}
                        </select>
                        <span>共 {projectFileNames.length} 个</span>
                    </div>
                )}
                <div className="flex-1 overflow-auto min-h-0 p-3">
                    <div className="w-full [&_pre]:!overflow-visible [&_pre]:!m-0 [&_pre]:!p-0 [&_pre]:!bg-transparent">
                        <ArtifactPreviewCodeBlock
                            language={projectFile ? codeLanguageForFile(projectFile) : codeLanguageFor(type)}
                            className="bg-transparent"
                        >
                            {projectFile ? readProjectFile(projectFiles!, projectFile) : code}
                        </ArtifactPreviewCodeBlock>
                    </div>
                </div>
            </div>
        );
    };

    const iconSize = "h-4 w-4";
    const canExportSvg = type === "mermaid" ? Boolean(mermaidSvg) : type === "svg";
    // 多文件工程导出入口文件，扩展名跟随入口
    const exportExtension =
        type === "project" ? entryFile?.split(".").pop() || "txt" : type ? sourceFileExtension(type) : "txt";

    return (
        <div ref={rootRef} className="flex flex-1 flex-col min-h-0 bg-background" data-aipp-slot="artifact-viewer">
            <div
                className={cn(
                    "flex flex-shrink-0 items-center gap-2 border-b border-border",
                    compact ? "px-2 py-1" : "px-3 py-1.5",
                )}
                data-aipp-slot="artifact-viewer-toolbar"
            >
                <div className="min-w-0 flex-1 truncate text-sm text-muted-foreground" title={title}>
                    {title && <span className="text-foreground">{title}</span>}
                    {type && <span className={cn(title && "ml-2", "text-xs")}>{previewTypeLabel(type)}</span>}
                </div>
                {versions.length > 1 && currentVersion !== undefined && onSelectVersion && (
                    <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                            <Button variant="ghost" size="sm" className="h-7 gap-1 px-2 text-xs" aria-label="切换版本">
                                v{currentVersion}
                                {currentVersion !== versions[0]?.version && <span className="text-muted-foreground">（旧版）</span>}
                                <ChevronDown className="h-3.5 w-3.5" />
                            </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="max-h-80 overflow-auto">
                            {versions.map((item, index) => (
                                <DropdownMenuItem
                                    key={item.version}
                                    onSelect={() => onSelectVersion(item.version)}
                                    className={cn(item.version === currentVersion && "font-medium")}
                                >
                                    <span className="w-10">v{item.version}</span>
                                    <span className="text-xs text-muted-foreground">
                                        {formatVersionTime(item.created_at)}
                                        {index === 0 && " · 最新"}
                                    </span>
                                </DropdownMenuItem>
                            ))}
                        </DropdownMenuContent>
                    </DropdownMenu>
                )}
                {actions}
                {type && code && (
                    <>
                        <Button variant="ghost" size="icon" iconLabel="复制代码" onClick={() => void copyCode()}>
                            <Copy className={iconSize} />
                        </Button>
                        <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                                <Button variant="ghost" size="icon" iconLabel="导出">
                                    <Download className={iconSize} />
                                </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                                <DropdownMenuItem
                                    onSelect={() =>
                                        void saveToFile(code, exportExtension, previewTypeLabel(type))
                                    }
                                >
                                    保存源文件（.{exportExtension}）
                                </DropdownMenuItem>
                                {canExportSvg && (
                                    <DropdownMenuItem
                                        onSelect={() => void saveToFile(type === "svg" ? code : mermaidSvg ?? "", "svg", "SVG")}
                                    >
                                        导出 SVG 图片
                                    </DropdownMenuItem>
                                )}
                            </DropdownMenuContent>
                        </DropdownMenu>
                    </>
                )}
                {isReady && (type === "html" || type === "svg" || type === "xml" || type === "project" || needsDevServer(type)) && (
                    <Button variant="ghost" size="icon" iconLabel="重新加载" onClick={reload}>
                        <RotateCw className={iconSize} />
                    </Button>
                )}
                {previewUrl && needsDevServer(type) && !compact && (
                    <Button variant="ghost" size="icon" iconLabel="在浏览器中打开" onClick={() => void openUrl(previewUrl)}>
                        <ExternalLink className={iconSize} />
                    </Button>
                )}
                {onOpenInWindow ? (
                    <Button variant="ghost" size="icon" iconLabel="在预览窗口打开" onClick={onOpenInWindow}>
                        <PanelTopOpen className={iconSize} />
                    </Button>
                ) : (
                    <Button
                        variant="ghost"
                        size="icon"
                        iconLabel={isFullscreen ? "退出全屏" : "全屏"}
                        onClick={() => void toggleFullscreen()}
                    >
                        {isFullscreen ? <Minimize2 className={iconSize} /> : <Maximize2 className={iconSize} />}
                    </Button>
                )}
                <Tabs value={view} onValueChange={(value) => setView(value as ArtifactViewerView)}>
                    <TabsList className={compact ? "h-8" : undefined}>
                        <TabsTrigger value="preview">预览</TabsTrigger>
                        <TabsTrigger value="code" disabled={!code && !projectFiles}>
                            代码
                        </TabsTrigger>
                        <TabsTrigger value="console">
                            控制台
                            {errorCount > 0 && (
                                <span className="ml-1 rounded-full bg-destructive px-1.5 text-[10px] leading-4 text-destructive-foreground">
                                    {errorCount}
                                </span>
                            )}
                        </TabsTrigger>
                    </TabsList>
                </Tabs>
            </div>

            <div ref={previewAreaRef} className="relative flex flex-1 flex-col min-h-0">
                {/* 预览始终挂载，切到代码/控制台不重载页面、不丢运行状态 */}
                <div className={cn("flex flex-1 flex-col min-h-0", view !== "preview" && "hidden")}>{renderPreview()}</div>
                {view === "code" && renderCode()}
                {view === "console" && renderConsole()}

                {view === "preview" && runtimeErrorCount > 0 && (
                    <div className="flex flex-shrink-0 items-center gap-2 border-t border-border bg-background px-3 py-1.5 text-xs">
                        <AlertTriangle className="h-3.5 w-3.5 text-destructive" />
                        <span className="text-destructive">页面运行出现 {runtimeErrorCount} 个错误</span>
                        <div className="ml-auto flex items-center gap-1">
                            <Button variant="ghost" size="sm" onClick={() => setView("console")}>
                                查看
                            </Button>
                            {onFixWithAI && (
                                <Button variant="outline" size="sm" onClick={() => void sendFix()} disabled={isSendingFix}>
                                    <Wand2 className="h-3.5 w-3.5" />
                                    让 AI 修复
                                </Button>
                            )}
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
}
