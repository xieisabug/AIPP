import React, { createContext, useContext, useMemo } from 'react';
import { cn } from '@/utils/utils';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { ChevronDown } from 'lucide-react';
import TodoList from './TodoList';
import ArtifactList from './ArtifactList';
import ContextList from './ContextList';
import { TodoItem, CodeArtifact, ContextItem, type ChatSidebarSectionId, type ChatSidebarViewState } from './types';
import PluginViewHost from '@/components/plugin/PluginViewHost';
import type { LoadedPlugin } from '@/services/PluginRuntime';

interface ChatSidebarContentProps {
    todos: TodoItem[];
    artifacts: CodeArtifact[];
    contextItems: ContextItem[];
    pluginList?: LoadedPlugin[];
    conversationId?: string;
    className?: string;
    mobile?: boolean;
    restoreState?: ChatSidebarViewState | null;
    focusedContextId?: string | null;
    selectedContextId?: string | null;
    onArtifactClick?: (artifact: CodeArtifact) => void;
    onContextClick?: (item: ContextItem) => void;
    onPreviewFileClick?: (item: ContextItem) => void;
    /** 规划标题上方画分隔线，用来和上方的工作区面板分开 */
    separatePlan?: boolean;
}

interface SectionState {
    id: string;
    isOpen: boolean;
    setIsOpen: (open: boolean) => void;
}

interface SectionsContextValue {
    sections: SectionState[];
    openCount: number;
}

const SectionsContext = createContext<SectionsContextValue>({ sections: [], openCount: 0 });

interface CollapsibleSectionProps {
    id: ChatSidebarSectionId;
    restoredScrollTop?: number;
    title: string;
    count: number;
    isOpen: boolean;
    mobile?: boolean;
    onOpenChange: (open: boolean) => void;
    children: React.ReactNode;
}

const CollapsibleSection: React.FC<CollapsibleSectionProps> = ({
    id,
    restoredScrollTop,
    title,
    count,
    isOpen,
    mobile = false,
    onOpenChange,
    children,
}) => {
    const { openCount } = useContext(SectionsContext);
    const scrollRef = React.useRef<HTMLDivElement>(null);
    // Restore the actual section scroller, not just the surrounding Sheet.
    // The second layout pass follows Radix's initial content measurement.
    React.useLayoutEffect(() => {
        if (!mobile || !isOpen || restoredScrollTop == null) return;
        const node = scrollRef.current;
        if (!node) return;
        node.scrollTop = restoredScrollTop;
        const frame = requestAnimationFrame(() => { node.scrollTop = restoredScrollTop; });
        return () => cancelAnimationFrame(frame);
    }, [mobile, restoredScrollTop]);

    return (
        <Collapsible 
            data-chat-sidebar-section={mobile ? id : undefined}
            open={isOpen} 
            onOpenChange={onOpenChange}
            className={cn(
                "flex flex-col",
                // Desktop splits the column height between open sections; mobile lets the
                // Sheet scroll as one page so a 360px screen is not carved into tiny scrollers.
                !mobile && "min-h-0",
                !mobile && isOpen && openCount > 0 && "flex-1"
            )}
        >
            <CollapsibleTrigger className={cn(
                "flex items-center justify-between w-full p-2 hover:bg-muted/50 rounded-md transition-colors flex-shrink-0",
                mobile && "sticky top-0 z-10 min-h-11 bg-background px-3 active:bg-muted"
            )}>
                <div className="flex items-center gap-2">
                    <span className="text-sm font-medium">{title}</span>
                    {count > 0 && (
                        <span className="text-xs bg-muted px-1.5 py-0.5 rounded-full">
                            {count}
                        </span>
                    )}
                </div>
                <ChevronDown
                    className={cn(
                        "h-4 w-4 text-muted-foreground transition-transform",
                        isOpen && "rotate-180"
                    )}
                />
            </CollapsibleTrigger>
            <CollapsibleContent className={cn(
                "overflow-hidden data-[state=open]:animate-collapsible-down data-[state=closed]:animate-collapsible-up",
                !mobile && "flex-1 min-h-0"
            )}>
                <div ref={scrollRef} data-chat-sidebar-scroll={mobile ? id : undefined} className={cn(!mobile && "h-full overflow-y-auto")}>
                    {children}
                </div>
            </CollapsibleContent>
        </Collapsible>
    );
};

const ChatSidebarContent: React.FC<ChatSidebarContentProps> = ({
    todos,
    artifacts,
    contextItems,
    pluginList = [],
    conversationId,
    className,
    mobile = false,
    restoreState,
    focusedContextId,
    selectedContextId,
    onArtifactClick,
    onContextClick,
    onPreviewFileClick,
    separatePlan = false,
}) => {
    const restored = mobile ? restoreState : null;
    const previousCounts = React.useRef({ todo: todos.length, artifact: artifacts.length, context: contextItems.length, plugin: -1 });
    const [todoOpen, setTodoOpen] = React.useState(restored?.todo?.isOpen ?? todos.length > 0);
    const [artifactOpen, setArtifactOpen] = React.useState(restored?.artifact?.isOpen ?? artifacts.length > 0);
    const [contextOpen, setContextOpen] = React.useState(restored?.context?.isOpen ?? contextItems.length > 0);

    // Update open state when data changes
    React.useEffect(() => {
        if ((!restored || previousCounts.current.todo !== todos.length) && todos.length > 0 && !todoOpen) setTodoOpen(true);
        previousCounts.current.todo = todos.length;
    }, [todos.length]);

    React.useEffect(() => {
        if ((!restored || previousCounts.current.artifact !== artifacts.length) && artifacts.length > 0 && !artifactOpen) setArtifactOpen(true);
        previousCounts.current.artifact = artifacts.length;
    }, [artifacts.length]);

    React.useEffect(() => {
        if ((!restored || previousCounts.current.context !== contextItems.length) && contextItems.length > 0 && !contextOpen) setContextOpen(true);
        previousCounts.current.context = contextItems.length;
    }, [contextItems.length]);

    // Ensure context section is expanded when a focus request arrives so the
    // target item is visible and scrollable.
    React.useEffect(() => {
        if (focusedContextId) {
            setContextOpen(true);
        }
    }, [focusedContextId]);

    const sidebarPluginViews = useMemo(
        () =>
            pluginList.flatMap((plugin) =>
                (plugin.contributions?.views ?? []).filter((view) => view.location === "conversation.sidebar")
            ),
        [pluginList]
    );
    if (previousCounts.current.plugin === -1) previousCounts.current.plugin = sidebarPluginViews.length;
    const [pluginViewsOpen, setPluginViewsOpen] = React.useState(restored?.plugin?.isOpen ?? (mobile || sidebarPluginViews.length > 0));

    React.useEffect(() => {
        if ((!restored || previousCounts.current.plugin !== sidebarPluginViews.length) && sidebarPluginViews.length > 0 && !pluginViewsOpen) setPluginViewsOpen(true);
        previousCounts.current.plugin = sidebarPluginViews.length;
    }, [sidebarPluginViews.length]);

    const openCount = useMemo(() => {
        return [todoOpen, artifactOpen, contextOpen, pluginViewsOpen].filter(Boolean).length;
    }, [todoOpen, artifactOpen, contextOpen, pluginViewsOpen]);

    const contextValue = useMemo(() => ({
        sections: [],
        openCount,
    }), [openCount]);

    return (
        <SectionsContext.Provider value={contextValue}>
            <div className={cn("flex flex-col gap-1", mobile ? "px-1 pb-2" : "h-full", className)}>
                {separatePlan ? (
                    <div className="border-t border-border" role="separator" data-aipp-slot="chat-sidebar-plan-separator" />
                ) : null}
                {/* Planning Section */}
                <CollapsibleSection
                    id="todo"
                    restoredScrollTop={restored?.todo?.scrollTop}
                    title="规划"
                    count={todos.length}
                    isOpen={todoOpen}
                    mobile={mobile}
                    onOpenChange={setTodoOpen}
                >
                    <TodoList todos={todos} />
                </CollapsibleSection>

                {/* Artifacts Section */}
                <CollapsibleSection 
                    id="artifact"
                    restoredScrollTop={restored?.artifact?.scrollTop}
                    title="Artifact" 
                    count={artifacts.length} 
                    isOpen={artifactOpen}
                    mobile={mobile}
                    onOpenChange={setArtifactOpen}
                >
                    <ArtifactList artifacts={artifacts} mobile={mobile} onArtifactClick={onArtifactClick} />
                </CollapsibleSection>

                {/* Context Section */}
                <CollapsibleSection 
                    id="context"
                    restoredScrollTop={restored?.context?.scrollTop}
                    title="上下文" 
                    count={contextItems.length} 
                    isOpen={contextOpen}
                    mobile={mobile}
                    onOpenChange={setContextOpen}
                >
                    <ContextList
                        items={contextItems}
                        mobile={mobile}
                        focusedItemId={focusedContextId}
                        selectedItemId={selectedContextId}
                        onItemClick={onContextClick}
                        onPreviewFileClick={onPreviewFileClick}
                    />
                </CollapsibleSection>

                {(mobile || sidebarPluginViews.length > 0) && (
                    <CollapsibleSection
                        id="plugin"
                        restoredScrollTop={restored?.plugin?.scrollTop}
                        title="插件视图"
                        count={sidebarPluginViews.length}
                        isOpen={pluginViewsOpen}
                        mobile={mobile}
                        onOpenChange={setPluginViewsOpen}
                    >
                        <div className="p-2">
                            <PluginViewHost
                                pluginList={pluginList}
                                location="conversation.sidebar"
                                context={{ conversationId }}
                                emptyDescription="当前没有插件提供会话侧栏视图。插件仅可使用已通过移动校验的能力；加载或执行失败会在此显示原因。"
                            />
                        </div>
                    </CollapsibleSection>
                )}
            </div>
        </SectionsContext.Provider>
    );
};

export default React.memo(ChatSidebarContent);
