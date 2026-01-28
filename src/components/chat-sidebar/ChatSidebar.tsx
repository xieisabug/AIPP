import React, { useState, useEffect, useCallback, useRef } from 'react';
import { cn } from '@/utils/utils';
import { Button } from '@/components/ui/button';
import { PanelRightClose, ExternalLink, ChevronLeft } from 'lucide-react';
import ChatSidebarContent from './ChatSidebarContent';
import { TodoItem, CodeArtifact, ContextItem } from './types';

interface ChatSidebarProps {
    todos: TodoItem[];
    artifacts: CodeArtifact[];
    contextItems: ContextItem[];
    conversationId: string;
    className?: string;
    onOpenWindow?: () => void;
    onArtifactClick?: (artifact: CodeArtifact) => void;
    onContextClick?: (item: ContextItem) => void;
    onExpandChange?: (isExpanded: boolean, width: number) => void;
}

export const DEFAULT_SIDEBAR_WIDTH = 280;
export const MIN_SIDEBAR_WIDTH = 200;
export const MAX_SIDEBAR_WIDTH = 500;
export const COLLAPSED_WIDTH = 24;

const ChatSidebar: React.FC<ChatSidebarProps> = ({
    todos,
    artifacts,
    contextItems,
    conversationId,
    className,
    onOpenWindow,
    onArtifactClick,
    onContextClick,
    onExpandChange,
}) => {
    const [isExpanded, setIsExpanded] = useState(false);
    const [hasAutoExpanded, setHasAutoExpanded] = useState(false);
    const [sidebarWidth, setSidebarWidth] = useState(DEFAULT_SIDEBAR_WIDTH);
    const [isResizing, setIsResizing] = useState(false);
    const prevConversationId = useRef(conversationId);
    const resizeRef = useRef<{ startX: number; startWidth: number } | null>(null);

    // Check if there's any data to display
    const hasData = todos.length > 0 || artifacts.length > 0 || contextItems.length > 0;
    const dataCount = todos.length + artifacts.length + contextItems.length;

    // Notify parent when expansion state or width changes
    useEffect(() => {
        onExpandChange?.(isExpanded, isExpanded ? sidebarWidth : COLLAPSED_WIDTH);
    }, [isExpanded, sidebarWidth, onExpandChange]);

    // Reset auto-expand state when conversation changes
    useEffect(() => {
        if (conversationId !== prevConversationId.current) {
            setHasAutoExpanded(false);
            setIsExpanded(false);
            prevConversationId.current = conversationId;
        }
    }, [conversationId]);

    // Auto-expand when first data arrives
    useEffect(() => {
        if (hasData && !hasAutoExpanded && conversationId) {
            setIsExpanded(true);
            setHasAutoExpanded(true);
        }
    }, [hasData, hasAutoExpanded, conversationId]);

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

    const handleToggle = useCallback(() => {
        setIsExpanded(prev => !prev);
    }, []);

    const handleOpenWindow = useCallback(() => {
        onOpenWindow?.();
    }, [onOpenWindow]);

    // Don't render anything if no conversation is selected
    if (!conversationId) {
        return null;
    }

    return (
        <div 
            className={cn(
                "relative flex-shrink-0 h-full",
                !isResizing && "transition-all duration-300 ease-in-out",
                className
            )}
            style={{ width: isExpanded ? sidebarWidth : COLLAPSED_WIDTH }}
        >
            {/* Collapsed state - show toggle tab */}
            {!isExpanded && (
                <div 
                    className="absolute inset-0 flex items-center justify-center cursor-pointer hover:bg-muted/50 transition-colors rounded-l-lg border-l border-border"
                    onClick={handleToggle}
                >
                    <div className="flex flex-col items-center gap-2">
                        <ChevronLeft className="h-4 w-4 text-muted-foreground" />
                        {hasData && (
                            <span className="text-xs text-muted-foreground writing-mode-vertical">
                                {dataCount}
                            </span>
                        )}
                    </div>
                </div>
            )}

            {/* Expanded state */}
            {isExpanded && (
                <div className="h-full flex flex-col bg-background border-l border-border rounded-l-lg overflow-hidden">
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
                    <div className="flex items-center justify-between p-2 border-b border-border">
                        <span className="text-sm font-medium">详情</span>
                        <div className="flex items-center gap-1">
                            <Button
                                variant="ghost"
                                size="icon"
                                className="h-7 w-7"
                                onClick={handleOpenWindow}
                                title="在新窗口打开"
                            >
                                <ExternalLink className="h-4 w-4" />
                            </Button>
                            <Button
                                variant="ghost"
                                size="icon"
                                className="h-7 w-7"
                                onClick={handleToggle}
                                title="收起"
                            >
                                <PanelRightClose className="h-4 w-4" />
                            </Button>
                        </div>
                    </div>

                    {/* Content */}
                    <div className="flex-1 min-h-0 overflow-hidden">
                        <ChatSidebarContent
                            className="h-full"
                            todos={todos}
                            artifacts={artifacts}
                            contextItems={contextItems}
                            onArtifactClick={onArtifactClick}
                            onContextClick={onContextClick}
                        />
                    </div>
                </div>
            )}
        </div>
    );
};

export default React.memo(ChatSidebar);
