import React, { useState, useEffect, useCallback, useRef } from 'react';
import { cn } from '@/utils/utils';
import { Button } from '@/components/ui/button';
import { PanelRightClose, ExternalLink, ChevronRight } from 'lucide-react';
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
    onExpandChange?: (isExpanded: boolean) => void;
}

export const SIDEBAR_WIDTH = 280;
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
    const prevConversationId = useRef(conversationId);

    // Check if there's any data to display
    const hasData = todos.length > 0 || artifacts.length > 0 || contextItems.length > 0;
    const dataCount = todos.length + artifacts.length + contextItems.length;

    // Notify parent when expansion state changes
    useEffect(() => {
        onExpandChange?.(isExpanded);
    }, [isExpanded, onExpandChange]);

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
                "relative flex-shrink-0 h-full transition-all duration-300 ease-in-out",
                className
            )}
            style={{ width: isExpanded ? SIDEBAR_WIDTH : COLLAPSED_WIDTH }}
        >
            {/* Collapsed state - show toggle tab */}
            {!isExpanded && (
                <div 
                    className="absolute inset-0 flex items-center justify-center cursor-pointer hover:bg-muted/50 transition-colors rounded-l-lg border-l border-border"
                    onClick={handleToggle}
                >
                    <div className="flex flex-col items-center gap-2">
                        <ChevronRight className="h-4 w-4 text-muted-foreground" />
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
                    <div className="flex-1 overflow-y-auto">
                        <ChatSidebarContent
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
