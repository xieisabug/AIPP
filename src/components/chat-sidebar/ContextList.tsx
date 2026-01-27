import React, { useCallback, useState } from 'react';
import { File, Search, FolderOpen, FileInput, FileQuestion, ExternalLink, ChevronDown, Image } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { invoke } from '@tauri-apps/api/core';
import { ContextItem } from './types';
import { cn } from '@/utils/utils';

interface ContextListProps {
    items: ContextItem[];
    className?: string;
    onItemClick?: (item: ContextItem) => void;
}

const getContextIcon = (type: ContextItem['type'], attachmentType?: string) => {
    if (type === 'user_file' && attachmentType === 'Image') {
        return <Image className="h-4 w-4 text-blue-500 flex-shrink-0" />;
    }
    switch (type) {
        case 'user_file':
            return <FileInput className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        case 'read_file':
            return <File className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        case 'search':
            return <Search className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        case 'list_directory':
            return <FolderOpen className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
        default:
            return <FileQuestion className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
    }
};

const getContextLabel = (type: ContextItem['type']): string => {
    switch (type) {
        case 'user_file':
            return '用户文件';
        case 'read_file':
            return '读取文件';
        case 'search':
            return '搜索';
        case 'list_directory':
            return '目录';
        default:
            return '其他';
    }
};

const ContextList: React.FC<ContextListProps> = ({ items, className, onItemClick }) => {
    const [collapsedSearchIds, setCollapsedSearchIds] = useState<Set<string>>(new Set());

    const handleOpenUrl = useCallback((url?: string) => {
        if (!url) return;
        openUrl(url).catch(console.error);
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

    const toggleSearchCollapse = useCallback((id: string) => {
        setCollapsedSearchIds((prev) => {
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
        if (item.attachmentData) {
            handleOpenAttachment(item);
        } else if (onItemClick) {
            onItemClick(item);
        }
    }, [handleOpenAttachment, onItemClick]);

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
                            <div key={item.id} className="flex flex-col">
                                <div
                                    className={cn(
                                        "flex items-center gap-2 px-2.5 py-2 rounded-lg border border-border bg-background transition-colors",
                                        "hover:bg-muted/40 cursor-pointer"
                                    )}
                                    onClick={() => handleItemClick(item)}
                                >
                                    {getContextIcon(item.type, item.attachmentData?.type)}
                                    <div className="flex-1 min-w-0">
                                        <p className="text-sm font-medium truncate">{item.name}</p>
                                        {item.details && item.details !== item.name && (
                                            <p className="text-xs text-muted-foreground truncate mt-0.5">
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
                                                toggleSearchCollapse(item.id);
                                            }}
                                        >
                                            <ChevronDown
                                                className={cn(
                                                    "h-4 w-4 transition-transform",
                                                    !collapsedSearchIds.has(item.id) && "rotate-180"
                                                )}
                                            />
                                        </button>
                                    )}
                                </div>
                                {item.type === 'search' && item.searchResults && item.searchResults.length > 0 && !collapsedSearchIds.has(item.id) && (
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
                                                    <span className="text-sm text-foreground flex-1 truncate">
                                                        {result.title}
                                                    </span>
                                                    <ExternalLink className="h-3 w-3 text-muted-foreground flex-shrink-0" />
                                                </div>
                                                {result.snippet && (
                                                    <p className="text-xs text-muted-foreground line-clamp-2 mt-0.5">
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
