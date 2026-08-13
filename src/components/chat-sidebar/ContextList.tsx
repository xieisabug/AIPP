import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { File, FileText, Search, FolderOpen, FileInput, FileQuestion, ExternalLink, ChevronDown, Image, Sparkles } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { invoke } from '@tauri-apps/api/core';
import { ContextItem } from './types';
import { cn } from '@/utils/utils';
import MCP from '@/assets/mcp.svg?react';

interface ContextListProps {
    items: ContextItem[];
    className?: string;
    focusedItemId?: string | null;
    selectedItemId?: string | null;
    onItemClick?: (item: ContextItem) => void;
    onPreviewFileClick?: (item: ContextItem) => void;
}

const SEARCH_AUTO_EXPAND_MS = 5000;
const RECENT_SEARCH_FINISH_WINDOW_MS = 10000;
const FOCUSED_SEARCH_EXPAND_DELAY_MS = 300;

const getPathBasename = (value: string): string => {
    const trimmed = value.trim();
    const parts = trimmed.split(/[\\/]+/).filter(Boolean);
    return parts.pop() || trimmed;
};

const getDisplayName = (item: ContextItem): string => {
    if (item.type === 'read_file') {
        return getPathBasename(item.name);
    }
    return item.name;
};

const getContextIcon = (type: ContextItem['type'], attachmentType?: string) => {
    if ((type === 'user_file' || type === 'generated_image') && attachmentType === 'Image') {
        return <Image className="h-4 w-4 text-blue-500 flex-shrink-0" />;
    }
    switch (type) {
        case 'user_file':
            return <FileInput className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        case 'generated_image':
            return <Image className="h-4 w-4 text-blue-500 flex-shrink-0" />;
        case 'skill':
            return <Sparkles className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        case 'read_file':
            return <File className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        case 'preview_file':
            return <FileText className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        case 'search':
            return <Search className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        case 'list_directory':
            return <FolderOpen className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        case 'loaded_mcp_tool':
            return <MCP className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        default:
            return <FileQuestion className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
    }
};

const getContextLabel = (type: ContextItem['type']): string => {
    switch (type) {
        case 'user_file':
            return '用户文件';
        case 'generated_image':
            return '生成图片';
        case 'skill':
            return 'Skills';
        case 'read_file':
            return '读取文件';
        case 'preview_file':
            return '预览文件';
        case 'search':
            return '搜索';
        case 'list_directory':
            return '目录';
        case 'loaded_mcp_tool':
            return '已加载工具';
        default:
            return '其他';
    }
};

const getItemTimestamp = (timestamp?: Date): number | null => {
    if (!timestamp) return null;

    const value = new Date(timestamp).getTime();
    return Number.isFinite(value) ? value : null;
};

const ContextList: React.FC<ContextListProps> = ({
    items,
    className,
    focusedItemId,
    selectedItemId,
    onItemClick,
    onPreviewFileClick,
}) => {
    const [expandedSearchIds, setExpandedSearchIds] = useState<Set<string>>(new Set());
    const manuallyToggledSearchIdsRef = useRef<Set<string>>(new Set());
    const seenSearchIdsRef = useRef<Set<string> | null>(null);
    const autoCloseTimersRef = useRef<Map<string, number>>(new Map());
    const itemRefs = useRef<Map<string, HTMLDivElement>>(new Map());

    // Scroll the focused item into view whenever focusedItemId changes.
    // Use block:"start" so the item lands at the top of the viewport (just
    // barely visible) instead of the bottom — feels more like "locate this".
    useEffect(() => {
        if (!focusedItemId) return;
        const el = itemRefs.current.get(focusedItemId);
        if (!el) return;
        el.scrollIntoView({ behavior: "smooth", block: "start" });
    }, [focusedItemId, items]);

    // Shortly after highlighting starts, auto-expand the focused item if it's a search
    // list with results. The timer is kept in a ref and NOT tied to this
    // effect's cleanup, so it still fires when the parent clears
    // focusedItemId at the 1s mark.
    const focusExpandTimerRef = useRef<number | null>(null);
    useEffect(() => {
        return () => {
            if (focusExpandTimerRef.current !== null) {
                window.clearTimeout(focusExpandTimerRef.current);
                focusExpandTimerRef.current = null;
            }
        };
    }, []);

    useEffect(() => {
        if (!focusedItemId) return;

        const item = items.find((i) => i.id === focusedItemId);
        if (!item || item.type !== 'search' || !item.searchResults || item.searchResults.length === 0) {
            return;
        }

        if (focusExpandTimerRef.current !== null) {
            window.clearTimeout(focusExpandTimerRef.current);
        }

        focusExpandTimerRef.current = window.setTimeout(() => {
            focusExpandTimerRef.current = null;
            manuallyToggledSearchIdsRef.current.add(focusedItemId);
            const existingAutoClose = autoCloseTimersRef.current.get(focusedItemId);
            if (existingAutoClose) {
                window.clearTimeout(existingAutoClose);
                autoCloseTimersRef.current.delete(focusedItemId);
            }
            setExpandedSearchIds((prev) => {
                if (prev.has(focusedItemId)) return prev;
                const next = new Set(prev);
                next.add(focusedItemId);
                return next;
            });
        }, FOCUSED_SEARCH_EXPAND_DELAY_MS);
    }, [focusedItemId, items]);

    const searchResultItems = useMemo(
        () => items.filter((item) => item.type === 'search' && item.searchResults && item.searchResults.length > 0),
        [items],
    );

    useEffect(() => {
        const currentSearchIds = new Set(searchResultItems.map((item) => item.id));

        const previousSearchIds = seenSearchIdsRef.current ?? new Set<string>();
        const now = Date.now();
        const autoExpandIds = searchResultItems
            .filter((item) => {
                if (previousSearchIds.has(item.id)) return false;
                const finishedAt = getItemTimestamp(item.timestamp);
                if (finishedAt === null) return false;
                const ageMs = now - finishedAt;
                return ageMs >= 0 && ageMs <= RECENT_SEARCH_FINISH_WINDOW_MS;
            })
            .map((item) => item.id);

        seenSearchIdsRef.current = currentSearchIds;

        autoCloseTimersRef.current.forEach((timer, id) => {
            if (!currentSearchIds.has(id)) {
                window.clearTimeout(timer);
                autoCloseTimersRef.current.delete(id);
            }
        });
        manuallyToggledSearchIdsRef.current.forEach((id) => {
            if (!currentSearchIds.has(id)) {
                manuallyToggledSearchIdsRef.current.delete(id);
            }
        });

        setExpandedSearchIds((prev) => {
            const next = new Set<string>();
            prev.forEach((id) => {
                if (currentSearchIds.has(id)) {
                    next.add(id);
                }
            });
            autoExpandIds.forEach((id) => next.add(id));
            if (next.size === prev.size && Array.from(next).every((id) => prev.has(id))) {
                return prev;
            }
            return next;
        });

        autoExpandIds.forEach((id) => {
            const existingTimer = autoCloseTimersRef.current.get(id);
            if (existingTimer) {
                window.clearTimeout(existingTimer);
            }

            const timer = window.setTimeout(() => {
                autoCloseTimersRef.current.delete(id);
                if (manuallyToggledSearchIdsRef.current.has(id)) {
                    return;
                }
                setExpandedSearchIds((prev) => {
                    if (!prev.has(id)) return prev;
                    const next = new Set(prev);
                    next.delete(id);
                    return next;
                });
            }, SEARCH_AUTO_EXPAND_MS);
            autoCloseTimersRef.current.set(id, timer);
        });
    }, [searchResultItems]);

    useEffect(() => {
        return () => {
            autoCloseTimersRef.current.forEach((timer) => window.clearTimeout(timer));
            autoCloseTimersRef.current.clear();
        };
    }, []);

    const handleOpenUrl = useCallback((url?: string) => {
        if (!url) return;
        openUrl(url).catch(console.error);
    }, []);

    const handleOpenMarkdownPreview = useCallback(async (markdown: string) => {
        if (!markdown) return;
        try {
            await invoke('run_artifacts', { lang: 'markdown', inputStr: markdown });
        } catch (error) {
            console.error('Open markdown preview failed', error);
        }
    }, []);

    const handleOpenAttachment = useCallback(async (item: ContextItem) => {
        if (!item.attachmentData) return;
        
        const { type, content, url } = item.attachmentData;
        
        if (type === 'Image' && content) {
            // Open image using Tauri backend
            try {
                await invoke('open_image', { imageData: content });
            } catch (e) {
                console.error('Open image failed', e);
            }
        } else if (url) {
            // Open file using system default application
            try {
                await openUrl(url);
            } catch (e) {
                console.error('Open file failed', e);
            }
        }
    }, []);

    const toggleSearchExpansion = useCallback((id: string) => {
        manuallyToggledSearchIdsRef.current.add(id);
        const existingTimer = autoCloseTimersRef.current.get(id);
        if (existingTimer) {
            window.clearTimeout(existingTimer);
            autoCloseTimersRef.current.delete(id);
        }

        setExpandedSearchIds((prev) => {
            const next = new Set(prev);
            if (next.has(id)) {
                next.delete(id);
            } else {
                next.add(id);
            }
            return next;
        });
    }, []);

    const handleItemClick = useCallback((item: ContextItem) => {
        if (item.type === 'preview_file' && onPreviewFileClick) {
            onPreviewFileClick(item);
        }
        if (onItemClick) {
            onItemClick(item);
            return;
        }
        if (item.type === 'search' && item.searchMarkdown) {
            handleOpenMarkdownPreview(item.searchMarkdown);
        } else if (item.attachmentData) {
            handleOpenAttachment(item);
        }
    }, [handleOpenAttachment, handleOpenMarkdownPreview, onItemClick, onPreviewFileClick]);

    if (items.length === 0) {
        return (
            <div className={cn("p-4 text-sm text-muted-foreground text-center", className)}>
                <FileQuestion className="h-8 w-8 mx-auto mb-2 opacity-50" />
                暂无上下文
            </div>
        );
    }

    // Group by type
    const groupedItems = items.reduce((acc, item) => {
        if (!acc[item.type]) {
            acc[item.type] = [];
        }
        acc[item.type].push(item);
        return acc;
    }, {} as Record<ContextItem['type'], ContextItem[]>);

    return (
        <div className={cn("flex flex-col gap-3 p-2", className)}>
            {Object.entries(groupedItems).map(([type, typeItems]) => (
                <div key={type} className="flex flex-col gap-1.5">
                    <div className="flex items-center gap-1.5 px-1">
                        {getContextIcon(type as ContextItem['type'])}
                        <span className="text-xs font-medium text-muted-foreground">
                            {getContextLabel(type as ContextItem['type'])}
                        </span>
                        <span className="text-xs text-muted-foreground/60">
                            ({typeItems.length})
                        </span>
                    </div>
                    <div className="flex flex-col gap-1">
                        {typeItems.map((item) => (
                            <div
                                key={item.id}
                                className="flex flex-col"
                                ref={(el) => {
                                    if (el) {
                                        itemRefs.current.set(item.id, el);
                                    } else {
                                        itemRefs.current.delete(item.id);
                                    }
                                }}
                            >
                                <div
                                    className={cn(
                                        "flex items-center gap-2 px-2.5 py-2 rounded-lg border border-border bg-background transition-colors",
                                        "hover:bg-muted/40 cursor-pointer",
                                        item.id === selectedItemId && "bg-muted/50 border-primary/30",
                                        item.id === focusedItemId && "ring-2 ring-primary/60 bg-muted/60 border-primary/40",
                                    )}
                                    onClick={() => handleItemClick(item)}
                                >
                                    {getContextIcon(item.type, item.attachmentData?.type)}
                                    <div className="flex-1 min-w-0">
                                        <p className="text-sm font-medium truncate" title={item.name}>
                                            {getDisplayName(item)}
                                        </p>
                                        {item.details && item.details !== item.name && (
                                            <p className="text-xs text-muted-foreground truncate mt-0.5" title={item.details}>
                                                {item.details}
                                            </p>
                                        )}
                                    </div>
                                    {item.type === 'search' && item.searchResults && item.searchResults.length > 0 && (
                                        <button
                                            type="button"
                                            className="h-6 w-6 flex items-center justify-center text-muted-foreground hover:text-foreground"
                                            onClick={(event) => {
                                                event.stopPropagation();
                                                toggleSearchExpansion(item.id);
                                            }}
                                            aria-label={expandedSearchIds.has(item.id) ? "收起搜索结果" : "展开搜索结果"}
                                            title={expandedSearchIds.has(item.id) ? "收起搜索结果" : "展开搜索结果"}
                                        >
                                            <ChevronDown
                                                className={cn(
                                                    "h-4 w-4 transition-transform",
                                                    expandedSearchIds.has(item.id) && "rotate-180"
                                                )}
                                            />
                                        </button>
                                    )}
                                </div>
                                {item.type === 'search' && item.searchResults && item.searchResults.length > 0 && expandedSearchIds.has(item.id) && (
                                    <div className="ml-3 mt-1 flex flex-col gap-1 border-l border-border pl-2">
                                        {item.searchResults.map((result, index) => (
                                            <button
                                                key={`${item.id}-${result.url}-${index}`}
                                                type="button"
                                                className="text-left px-2 py-1.5 rounded-md transition-colors hover:bg-muted/40"
                                                onClick={(event) => {
                                                    event.stopPropagation();
                                                    handleOpenUrl(result.url);
                                                }}
                                            >
                                                <div className="flex items-center gap-1.5">
                                                    <span className="text-sm text-foreground flex-1 truncate" title={result.title}>
                                                        {result.title}
                                                    </span>
                                                    <ExternalLink className="h-3 w-3 text-muted-foreground flex-shrink-0" />
                                                </div>
                                                {result.snippet && (
                                                    <p className="text-xs text-muted-foreground line-clamp-2 mt-0.5" title={result.snippet}>
                                                        {result.snippet}
                                                    </p>
                                                )}
                                            </button>
                                        ))}
                                    </div>
                                )}
                            </div>
                        ))}
                    </div>
                </div>
            ))}
        </div>
    );
};

export default React.memo(ContextList);
