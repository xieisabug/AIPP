import { useEffect, useState, useCallback, useRef } from "react";
import { listen, emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useTheme } from "../hooks/useTheme";
import { ChatSidebarContent } from "../components/chat-sidebar";
import { TodoItem, CodeArtifact, ContextItem } from "../components/chat-sidebar/types";
import { Button } from "../components/ui/button";
import { X, FileText, Globe, Image, ExternalLink } from "lucide-react";
import EmbeddedArtifactPreview from "../components/EmbeddedArtifactPreview";
import { cn } from "../utils/utils";

// Preview mode: 'artifact' shows EmbeddedArtifactPreview, 'context' shows context details
type PreviewMode = 'artifact' | 'context' | 'none';

// Context preview content
interface ContextPreview {
    context: ContextItem;
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
    useTheme();
    
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
    } | null>(null);
    const [dataReceived, setDataReceived] = useState(false);
    const [hasAutoPreviewedLatest, setHasAutoPreviewedLatest] = useState(false);
    
    // Sidebar resize state
    const [sidebarWidth, setSidebarWidth] = useState(DEFAULT_SIDEBAR_WIDTH);
    const [isResizing, setIsResizing] = useState(false);
    const resizeRef = useRef<{ startX: number; startWidth: number } | null>(null);

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
        })
            .then((res) => {
                console.log("Artifact preview started:", res);
            })
            .catch((error) => {
                console.error("Failed to run artifact:", error);
            });
    }, [sidebarData.conversationId]);

    // Handle context click - show context preview
    const handleContextClick = useCallback((item: ContextItem) => {
        if (item.type === 'search' && item.searchMarkdown) {
            setPreviewMode('artifact');
            setContextPreview(null);
            setPreviewPayload({ lang: "markdown", inputStr: item.searchMarkdown });
            const conversationId = sidebarData.conversationId ? parseInt(sidebarData.conversationId, 10) : undefined;
            invoke("run_artifacts", {
                lang: "markdown",
                inputStr: item.searchMarkdown,
                sourceWindow: "sidebar",
                conversationId,
            }).catch((error) => {
                console.error("Failed to preview search markdown:", error);
            });
            return;
        }
        setPreviewMode('context');
        setContextPreview({ context: item });
        setPreviewPayload(null);
    }, [sidebarData.conversationId]);

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
        }).catch((error) => {
            console.error("Failed to open preview in preview window:", error);
        });
    }, [previewPayload, sidebarData.conversationId]);

    const canOpenInPreviewWindow = !!previewPayload;
    const formatToolParameters = useCallback((raw?: string) => {
        if (!raw || raw.trim().length === 0) {
            return '{}';
        }
        try {
            return JSON.stringify(JSON.parse(raw), null, 2);
        } catch {
            return raw;
        }
    }, []);

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
            return (
                <div className="flex-1 flex flex-col min-h-0">
                    <div className="flex items-center justify-between p-3 border-b border-border flex-shrink-0">
                        <div className="flex items-center gap-2">
                            {context.type === 'search' ? (
                                <Globe className="h-4 w-4" />
                            ) : context.attachmentData?.type === 'Image' ? (
                                <Image className="h-4 w-4" />
                            ) : (
                                <FileText className="h-4 w-4" />
                            )}
                            <span className="font-medium text-sm truncate max-w-[200px]">{context.name}</span>
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
                        {context.type === 'loaded_mcp_tool' ? (
                            <div className="space-y-4">
                                <div className="rounded-lg border border-border bg-muted/30 p-3">
                                    <p className="text-xs text-muted-foreground">工具</p>
                                    <p className="text-sm font-mono break-all mt-1">
                                        {context.loadedToolData?.serverName || '未知服务'}::{context.loadedToolData?.toolName || context.name}
                                    </p>
                                    <p className="text-xs text-muted-foreground mt-2">
                                        状态: {context.details || context.loadedToolData?.status || '未知'}
                                    </p>
                                </div>
                                <div>
                                    <p className="text-xs text-muted-foreground mb-1">介绍</p>
                                    <div className="rounded border border-border bg-background max-h-64 overflow-auto">
                                        <pre className="p-3 text-sm whitespace-pre-wrap leading-6">
                                            {context.loadedToolData?.description?.trim() || '暂无工具介绍'}
                                        </pre>
                                    </div>
                                </div>
                                <div>
                                    <p className="text-xs text-muted-foreground mb-1">参数</p>
                                    <div className="rounded border border-border bg-background max-h-80 overflow-auto">
                                        <pre className="p-3 text-xs font-mono whitespace-pre-wrap break-all leading-5">
                                            {formatToolParameters(context.loadedToolData?.parameters)}
                                        </pre>
                                    </div>
                                </div>
                            </div>
                        ) : (
                            <>
                                {/* Image preview */}
                                {context.attachmentData?.type === 'Image' && context.attachmentData.content && (
                                    <img
                                        src={`data:image/png;base64,${context.attachmentData.content}`}
                                        alt={context.name}
                                        className="max-w-full h-auto rounded-lg"
                                    />
                                )}
                                {/* Text content preview */}
                                {context.attachmentData?.type === 'Text' && context.attachmentData.content && (
                                    <pre className="text-sm font-mono whitespace-pre-wrap break-all bg-muted p-4 rounded-lg">
                                        {context.attachmentData.content}
                                    </pre>
                                )}
                                {/* Search results */}
                                {context.type === 'search' && context.searchMarkdown && (
                                    <div className="prose prose-sm dark:prose-invert max-w-none">
                                        <pre className="whitespace-pre-wrap">{context.searchMarkdown}</pre>
                                    </div>
                                )}
                                {/* Search result items */}
                                {context.type === 'search' && context.searchResults && context.searchResults.length > 0 && (
                                    <div className="space-y-3">
                                        {context.searchResults.map((result, idx) => (
                                            <div key={idx} className="p-3 bg-muted rounded-lg">
                                                <a
                                                    href={result.url}
                                                    target="_blank"
                                                    rel="noopener noreferrer"
                                                    className="text-sm font-medium text-primary hover:underline"
                                                >
                                                    {result.title}
                                                </a>
                                                {result.snippet && (
                                                    <p className="text-xs text-muted-foreground mt-1">{result.snippet}</p>
                                                )}
                                            </div>
                                        ))}
                                    </div>
                                )}
                                {/* File path details */}
                                {context.details && !context.searchMarkdown && !context.searchResults && (
                                    <div className="text-sm text-muted-foreground">
                                        <p className="font-medium mb-2">路径：</p>
                                        <code className="text-xs bg-muted p-2 rounded block">{context.details}</code>
                                    </div>
                                )}
                                {/* Fallback for no content */}
                                {!context.attachmentData?.content &&
                                    !context.searchMarkdown &&
                                    !context.searchResults?.length &&
                                    !context.details && (
                                        <p className="text-muted-foreground">暂无可预览的内容</p>
                                    )}
                            </>
                        )}
                    </div>
                </div>
            );
        }

        return null;
    };

    return (
        <div className={cn("flex h-screen bg-background", isResizing && "select-none")}>
            {/* Left side - Preview area */}
            <div className="relative flex-1 flex flex-col border-r border-border min-w-0">
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
            >
                {/* Resize handle */}
                <div
                    className={cn(
                        "absolute left-0 top-0 bottom-0 w-1 cursor-ew-resize z-10 transition-colors",
                        "hover:bg-primary/50",
                        isResizing && "bg-primary/50"
                    )}
                    onMouseDown={handleResizeStart}
                />

                {/* Header */}
                <div className="flex items-center justify-between p-3 border-b border-border">
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
                <div className="flex-1 min-h-0 overflow-hidden">
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
