import { useEffect, useState, useCallback, useRef } from "react";
import { listen, emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTheme } from "../hooks/useTheme";
import { ChatSidebarContent } from "../components/chat-sidebar";
import { TodoItem, CodeArtifact, ContextItem } from "../components/chat-sidebar/types";
import { Button } from "../components/ui/button";
import { X, FileText, Globe, Image, ExternalLink, FolderOpen, Search, Sparkles, LoaderCircle, AlertCircle } from "lucide-react";
import EmbeddedArtifactPreview from "../components/EmbeddedArtifactPreview";
import RawTextRenderer from "../components/RawTextRenderer";
import UnifiedMarkdown from "../components/UnifiedMarkdown";
import { cn } from "../utils/utils";
import { buildPreviewPayloadFromContextItem, hydrateContextPreview } from "../components/chat-sidebar/contextPreview";

// Preview mode: 'artifact' shows EmbeddedArtifactPreview, 'context' shows context details
type PreviewMode = 'artifact' | 'context' | 'none';

// Context preview content
interface ContextPreview {
    context: ContextItem;
    loading?: boolean;
    error?: string;
}

// Sidebar data from main window
interface SidebarData {
    todos: TodoItem[];
    artifacts: CodeArtifact[];
    contextItems: ContextItem[];
    conversationId: string;
}

// Sidebar width constraints
const DEFAULT_SIDEBAR_WIDTH = 300;
const MIN_SIDEBAR_WIDTH = 200;
const MAX_SIDEBAR_WIDTH = 500;

function SidebarWindow() {
    useTheme("sidebar");
    
    const [sidebarData, setSidebarData] = useState<SidebarData>({
        todos: [],
        artifacts: [],
        contextItems: [],
        conversationId: "",
    });
    const [previewMode, setPreviewMode] = useState<PreviewMode>('none');
    const [contextPreview, setContextPreview] = useState<ContextPreview | null>(null);
    const [previewPayload, setPreviewPayload] = useState<{
        lang: string;
        inputStr: string;
        dbId?: string;
        assistantId?: number;
        artifactKey?: string;
        entryFile?: string;
    } | null>(null);
    const [dataReceived, setDataReceived] = useState(false);
    const [hasAutoPreviewedLatest, setHasAutoPreviewedLatest] = useState(false);
    
    // Sidebar resize state
    const [sidebarWidth, setSidebarWidth] = useState(DEFAULT_SIDEBAR_WIDTH);
    const [isResizing, setIsResizing] = useState(false);
    const resizeRef = useRef<{ startX: number; startWidth: number } | null>(null);
    const contextRequestIdRef = useRef(0);

    // Listen for sidebar data sync from main window
    useEffect(() => {
        const unlisten = listen<SidebarData>("sidebar-data-sync", (event) => {
            setSidebarData(event.payload);
            setDataReceived(true);
        });

        return () => {
            unlisten.then((f) => f());
        };
    }, []);

    // Auto-preview the latest artifact when data is first received
    useEffect(() => {
        if (dataReceived && !hasAutoPreviewedLatest && sidebarData.artifacts.length > 0) {
            // Get the last artifact (most recent)
            const latestArtifact = sidebarData.artifacts[sidebarData.artifacts.length - 1];
            
            // Auto-preview it
            setPreviewMode('artifact');
            setContextPreview(null);
            setPreviewPayload({
                lang: latestArtifact.language,
                inputStr: latestArtifact.code,
                dbId: latestArtifact.dbId,
                assistantId: latestArtifact.assistantId,
                artifactKey: latestArtifact.artifactKey,
                entryFile: latestArtifact.entryFile,
            });
            setHasAutoPreviewedLatest(true);

            const conversationId = sidebarData.conversationId ? parseInt(sidebarData.conversationId, 10) : undefined;
            invoke("run_artifacts", {
                lang: latestArtifact.language,
                inputStr: latestArtifact.code,
                sourceWindow: "sidebar",
                conversationId,
                dbId: latestArtifact.dbId,
                assistantId: latestArtifact.assistantId,
                artifactKey: latestArtifact.artifactKey,
                entryFile: latestArtifact.entryFile,
            })
                .then((res) => {
                    console.log("Auto-preview latest artifact:", res);
                })
                .catch((error) => {
                    console.error("Failed to auto-preview artifact:", error);
                });
        }
    }, [dataReceived, hasAutoPreviewedLatest, sidebarData.artifacts, sidebarData.conversationId]);

    // Keep sending ready signal until data is received
    useEffect(() => {
        if (dataReceived) return;

        // Send initial ready signal
        emit("sidebar-window-ready");

        // Keep sending ready signals every 200ms until data received
        const interval = setInterval(() => {
            if (!dataReceived) {
                emit("sidebar-window-ready");
            }
        }, 200);

        // Timeout after 10 seconds
        const timeout = setTimeout(() => {
            clearInterval(interval);
        }, 10000);

        return () => {
            clearInterval(interval);
            clearTimeout(timeout);
        };
    }, [dataReceived]);

    // Handle resize drag
    const handleResizeStart = useCallback((e: React.MouseEvent) => {
        e.preventDefault();
        setIsResizing(true);
        resizeRef.current = { startX: e.clientX, startWidth: sidebarWidth };
    }, [sidebarWidth]);

    useEffect(() => {
        if (!isResizing) return;

        const handleMouseMove = (e: MouseEvent) => {
            if (!resizeRef.current) return;
            // Dragging left increases width (since sidebar is on the right)
            const delta = resizeRef.current.startX - e.clientX;
            const newWidth = Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, resizeRef.current.startWidth + delta));
            setSidebarWidth(newWidth);
        };

        const handleMouseUp = () => {
            setIsResizing(false);
            resizeRef.current = null;
        };

        document.addEventListener('mousemove', handleMouseMove);
        document.addEventListener('mouseup', handleMouseUp);

        return () => {
            document.removeEventListener('mousemove', handleMouseMove);
            document.removeEventListener('mouseup', handleMouseUp);
        };
    }, [isResizing]);

    // Handle artifact click - run artifact preview and show in left panel
    const handleArtifactClick = useCallback((artifact: CodeArtifact) => {
        // Switch to artifact preview mode
        setPreviewMode('artifact');
        setContextPreview(null);
        setPreviewPayload({
            lang: artifact.language,
            inputStr: artifact.code,
            dbId: artifact.dbId,
            assistantId: artifact.assistantId,
            artifactKey: artifact.artifactKey,
            entryFile: artifact.entryFile,
        });
        
        // Call run_artifacts to start the preview
        const conversationId = sidebarData.conversationId ? parseInt(sidebarData.conversationId, 10) : undefined;
        invoke("run_artifacts", {
            lang: artifact.language,
            inputStr: artifact.code,
            sourceWindow: "sidebar",
            conversationId,
            dbId: artifact.dbId,
            assistantId: artifact.assistantId,
            artifactKey: artifact.artifactKey,
            entryFile: artifact.entryFile,
        })
            .then((res) => {
                console.log("Artifact preview started:", res);
            })
            .catch((error) => {
                console.error("Failed to run artifact:", error);
            });
    }, [sidebarData.conversationId]);

    const cacheContextItem = useCallback((nextItem: ContextItem) => {
        setSidebarData((prev) => ({
            ...prev,
            contextItems: prev.contextItems.map((item) => (item.id === nextItem.id ? nextItem : item)),
        }));
    }, []);

    const handleContextClick = useCallback(async (item: ContextItem) => {
        const requestId = ++contextRequestIdRef.current;
        setPreviewMode('context');
        setContextPreview({
            context: item,
            loading: item.previewStatus === 'needs_load',
        });
        setPreviewPayload(buildPreviewPayloadFromContextItem(item));

        if (item.previewStatus !== 'needs_load') {
            return;
        }

        try {
            const hydratedItem = await hydrateContextPreview(item);
            if (requestId !== contextRequestIdRef.current) {
                return;
            }

            cacheContextItem(hydratedItem);
            setContextPreview({
                context: hydratedItem,
                loading: false,
            });
            setPreviewPayload(buildPreviewPayloadFromContextItem(hydratedItem));
        } catch (error) {
            if (requestId !== contextRequestIdRef.current) {
                return;
            }

            setContextPreview({
                context: {
                    ...item,
                    previewStatus: 'error',
                },
                loading: false,
                error: error instanceof Error ? error.message : String(error),
            });
            setPreviewPayload(null);
        }
    }, [cacheContextItem]);

    // Close the window
    const handleClose = useCallback(() => {
        invoke("close_sidebar_window");
    }, []);

    // Clear preview
    const handleClearPreview = useCallback(() => {
        setPreviewMode('none');
        setContextPreview(null);
        setPreviewPayload(null);
    }, []);

    const handleOpenInPreviewWindow = useCallback(() => {
        if (!previewPayload) return;
        const conversationId = sidebarData.conversationId ? parseInt(sidebarData.conversationId, 10) : undefined;
        invoke("run_artifacts", {
            lang: previewPayload.lang,
            inputStr: previewPayload.inputStr,
            conversationId,
            dbId: previewPayload.dbId,
            assistantId: previewPayload.assistantId,
            artifactKey: previewPayload.artifactKey,
            entryFile: previewPayload.entryFile,
        }).catch((error) => {
            console.error("Failed to open preview in preview window:", error);
        });
    }, [previewPayload, sidebarData.conversationId]);

    const canOpenInPreviewWindow = !!previewPayload;

    const handleOpenUrl = useCallback((url?: string) => {
        if (!url) return;
        openUrl(url).catch((error) => {
            console.error("Failed to open URL:", error);
        });
    }, []);

    const renderContextIcon = useCallback((context: ContextItem) => {
        if (context.type === 'search') {
            return <Search className="h-4 w-4" />;
        }
        if (context.type === 'list_directory') {
            return <FolderOpen className="h-4 w-4" />;
        }
        if (context.type === 'skill') {
            return <Sparkles className="h-4 w-4" />;
        }
        if (context.attachmentData?.type === 'Image' || context.previewData?.contentType === 'image') {
            return <Image className="h-4 w-4" />;
        }
        if (context.type === 'loaded_mcp_tool') {
            return <Globe className="h-4 w-4" />;
        }
        return <FileText className="h-4 w-4" />;
    }, []);

    const renderPreviewItems = useCallback((context: ContextItem) => {
        const items = context.previewData?.items;
        if (!items || items.length === 0) {
            return null;
        }

        return (
            <div className="space-y-3">
                <p className="text-xs text-muted-foreground">条目</p>
                {items.map((item, index) => (
                    <div key={`${context.id}-item-${index}`} className="rounded-lg border border-border bg-muted/30 p-3">
                        <div className="flex items-center justify-between gap-3">
                            <div className="min-w-0">
                                <p className="text-sm font-medium break-all">{item.label}</p>
                                {item.description && (
                                    <p className="text-xs text-muted-foreground mt-1 break-all">{item.description}</p>
                                )}
                            </div>
                            {item.url && (
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    className="h-7 px-2"
                                    onClick={() => handleOpenUrl(item.url)}
                                >
                                    <ExternalLink className="h-3.5 w-3.5" />
                                    打开
                                </Button>
                            )}
                        </div>
                        {item.value && !item.url && (
                            <pre className="mt-3 max-h-48 overflow-auto rounded bg-background p-3 text-xs font-mono whitespace-pre-wrap break-all">
                                {item.value}
                            </pre>
                        )}
                    </div>
                ))}
            </div>
        );
    }, [handleOpenUrl]);

    const renderPreviewMetadata = useCallback((context: ContextItem) => {
        const metadata = context.previewData?.metadata;
        if (!metadata || Object.keys(metadata).length === 0) {
            return null;
        }

        return (
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                {Object.entries(metadata).map(([label, value]) => (
                    <div key={`${context.id}-${label}`} className="rounded-lg border border-border bg-muted/20 p-3">
                        <p className="text-xs text-muted-foreground">{label}</p>
                        <p className="text-sm mt-1 break-all">{value}</p>
                    </div>
                ))}
            </div>
        );
    }, []);

    const renderContextPreviewBody = useCallback((preview: ContextPreview) => {
        const { context, loading, error } = preview;
        const previewData = context.previewData;

        if (loading) {
            return (
                <div className="flex h-full items-center justify-center text-muted-foreground">
                    <div className="flex items-center gap-2 text-sm">
                        <LoaderCircle className="h-4 w-4 animate-spin" />
                        正在加载详情…
                    </div>
                </div>
            );
        }

        if (error) {
            return (
                <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm text-destructive">
                    <div className="flex items-center gap-2 font-medium">
                        <AlertCircle className="h-4 w-4" />
                        加载失败
                    </div>
                    <p className="mt-2 break-all">{error}</p>
                </div>
            );
        }

        if (!previewData) {
            return <p className="text-muted-foreground">暂无可预览的内容</p>;
        }

        return (
            <div className="space-y-4">
                {renderPreviewMetadata(context)}

                {previewData.contentType === 'image' && previewData.content && (
                    <img
                        src={`data:image/png;base64,${previewData.content}`}
                        alt={previewData.title || context.name}
                        className="max-w-full h-auto rounded-lg border border-border"
                    />
                )}

                {previewData.contentType === 'markdown' && previewData.content && (
                    <div className="rounded-lg border border-border bg-background p-4">
                        <UnifiedMarkdown className="break-all" noProseWrapper={false}>
                            {previewData.content}
                        </UnifiedMarkdown>
                    </div>
                )}

                {previewData.content && previewData.contentType !== 'markdown' && previewData.contentType !== 'image' && (
                    <div className="rounded-lg border border-border bg-background p-4">
                        {previewData.contentType === 'text' ? (
                            <RawTextRenderer content={previewData.content} />
                        ) : (
                            <pre className="text-sm font-mono whitespace-pre-wrap break-all">
                                {previewData.content}
                            </pre>
                        )}
                    </div>
                )}

                {renderPreviewItems(context)}

                {!previewData.content && !previewData.items?.length && previewData.path && (
                    <div className="rounded-lg border border-border bg-muted/20 p-4">
                        <p className="text-xs text-muted-foreground mb-2">路径</p>
                        <code className="text-xs break-all">{previewData.path}</code>
                    </div>
                )}

                {!previewData.content && !previewData.items?.length && !previewData.path && (
                    <p className="text-muted-foreground">暂无可预览的内容</p>
                )}
            </div>
        );
    }, [renderPreviewItems, renderPreviewMetadata]);

    // Render preview content based on mode
    const renderPreview = () => {
        if (previewMode === 'none') {
            return (
                <div className="flex-1 flex items-center justify-center text-muted-foreground">
                    <div className="text-center">
                        <FileText className="h-12 w-12 mx-auto mb-3 opacity-50" />
                        <p>点击右侧列表项预览内容</p>
                    </div>
                </div>
            );
        }

        if (previewMode === 'artifact') {
            // Use embedded artifact preview component
            return <EmbeddedArtifactPreview className="flex-1" previewOnly />;
        }

        if (previewMode === 'context' && contextPreview) {
            const context = contextPreview.context;
            const previewTitle = context.previewData?.title || context.name;
            const previewSubtitle = context.previewData?.subtitle || context.details;
            return (
                <div className="flex-1 flex flex-col min-h-0">
                    <div className="flex items-center justify-between p-3 border-b border-border flex-shrink-0">
                        <div className="flex items-start gap-2 min-w-0">
                            <div className="mt-0.5 flex-shrink-0">
                                {renderContextIcon(context)}
                            </div>
                            <div className="min-w-0">
                                <span className="font-medium text-sm truncate max-w-[240px] block" title={previewTitle}>
                                    {previewTitle}
                                </span>
                                {previewSubtitle && (
                                    <p className="text-xs text-muted-foreground truncate mt-0.5" title={previewSubtitle}>
                                        {previewSubtitle}
                                    </p>
                                )}
                            </div>
                        </div>
                        <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7"
                            onClick={handleClearPreview}
                        >
                            <X className="h-4 w-4" />
                        </Button>
                    </div>
                    <div className="flex-1 overflow-auto p-4">
                        {renderContextPreviewBody(contextPreview)}
                    </div>
                </div>
            );
        }

        return null;
    };

    return (
        <div
            className={cn("flex h-screen bg-background", isResizing && "select-none")}
            data-aipp-window="sidebar"
            data-aipp-slot="window-root"
        >
            {/* Left side - Preview area */}
            <div className="relative flex-1 flex flex-col border-r border-border min-w-0" data-aipp-slot="sidebar-preview-pane">
                {renderPreview()}
                {canOpenInPreviewWindow && (
                    <Button
                        variant="secondary"
                        size="sm"
                        className="absolute top-3 right-3 z-20 h-8 px-3 shadow-md bg-background/90 backdrop-blur-sm"
                        onClick={handleOpenInPreviewWindow}
                        title="在预览窗口打开"
                    >
                        <ExternalLink className="h-3.5 w-3.5" />
                        在预览窗口打开
                    </Button>
                )}
            </div>

            {/* Right side - Sidebar content with resize handle */}
            <div 
                className="relative flex-shrink-0 flex flex-col bg-background"
                style={{ width: sidebarWidth }}
                data-aipp-slot="sidebar-panel"
            >
                {/* Resize handle */}
                <div
                    className={cn(
                        "absolute left-0 top-0 bottom-0 w-1 cursor-ew-resize z-10 transition-colors",
                        "hover:bg-primary/50",
                        isResizing && "bg-primary/50"
                    )}
                    onMouseDown={handleResizeStart}
                    data-aipp-slot="sidebar-resize-handle"
                />

                {/* Header */}
                <div className="flex items-center justify-between p-3 border-b border-border" data-aipp-slot="sidebar-header">
                    <span className="text-sm font-medium">详情</span>
                    <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7"
                        onClick={handleClose}
                        title="关闭"
                    >
                        <X className="h-4 w-4" />
                    </Button>
                </div>

                {/* Content */}
                <div className="flex-1 min-h-0 overflow-hidden" data-aipp-slot="sidebar-content">
                    {sidebarData.conversationId ? (
                        <ChatSidebarContent
                            className="h-full p-2"
                            todos={sidebarData.todos}
                            artifacts={sidebarData.artifacts}
                            contextItems={sidebarData.contextItems}
                            onArtifactClick={handleArtifactClick}
                            onContextClick={handleContextClick}
                        />
                    ) : (
                        <div className="flex items-center justify-center h-full text-muted-foreground">
                            <p>暂无对话</p>
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
}

export default SidebarWindow;
