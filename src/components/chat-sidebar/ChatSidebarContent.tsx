import React, { createContext, useContext, useMemo } from 'react';
import { cn } from '@/utils/utils';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { ChevronDown } from 'lucide-react';
import TodoList from './TodoList';
import ArtifactList from './ArtifactList';
import ContextList from './ContextList';
import { TodoItem, CodeArtifact, ContextItem } from './types';

interface ChatSidebarContentProps {
    todos: TodoItem[];
    artifacts: CodeArtifact[];
    contextItems: ContextItem[];
    className?: string;
    onArtifactClick?: (artifact: CodeArtifact) => void;
    onContextClick?: (item: ContextItem) => void;
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
    title: string;
    count: number;
    isOpen: boolean;
    onOpenChange: (open: boolean) => void;
    children: React.ReactNode;
}

const CollapsibleSection: React.FC<CollapsibleSectionProps> = ({
    title,
    count,
    isOpen,
    onOpenChange,
    children,
}) => {
    const { openCount } = useContext(SectionsContext);

    return (
        <Collapsible 
            open={isOpen} 
            onOpenChange={onOpenChange}
            className={cn(
                "flex flex-col min-h-0",
                isOpen && openCount > 0 && "flex-1"
            )}
        >
            <CollapsibleTrigger className="flex items-center justify-between w-full p-2 hover:bg-muted/50 rounded-md transition-colors flex-shrink-0">
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
            <CollapsibleContent className="flex-1 min-h-0 overflow-hidden data-[state=open]:animate-collapsible-down data-[state=closed]:animate-collapsible-up">
                <div className="h-full overflow-y-auto">
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
    className,
    onArtifactClick,
    onContextClick,
}) => {
    const [todoOpen, setTodoOpen] = React.useState(todos.length > 0);
    const [artifactOpen, setArtifactOpen] = React.useState(artifacts.length > 0);
    const [contextOpen, setContextOpen] = React.useState(contextItems.length > 0);

    // Update open state when data changes
    React.useEffect(() => {
        if (todos.length > 0 && !todoOpen) setTodoOpen(true);
    }, [todos.length]);

    React.useEffect(() => {
        if (artifacts.length > 0 && !artifactOpen) setArtifactOpen(true);
    }, [artifacts.length]);

    React.useEffect(() => {
        if (contextItems.length > 0 && !contextOpen) setContextOpen(true);
    }, [contextItems.length]);

    const openCount = useMemo(() => {
        return [todoOpen, artifactOpen, contextOpen].filter(Boolean).length;
    }, [todoOpen, artifactOpen, contextOpen]);

    const contextValue = useMemo(() => ({
        sections: [],
        openCount,
    }), [openCount]);

    return (
        <SectionsContext.Provider value={contextValue}>
            <div className={cn("flex flex-col gap-1 h-full", className)}>
                {/* Todo/Plan Section */}
                <CollapsibleSection 
                    title="计划" 
                    count={todos.length} 
                    isOpen={todoOpen}
                    onOpenChange={setTodoOpen}
                >
                    <TodoList todos={todos} />
                </CollapsibleSection>

                {/* Artifacts Section */}
                <CollapsibleSection 
                    title="Artifact" 
                    count={artifacts.length} 
                    isOpen={artifactOpen}
                    onOpenChange={setArtifactOpen}
                >
                    <ArtifactList artifacts={artifacts} onArtifactClick={onArtifactClick} />
                </CollapsibleSection>

                {/* Context Section */}
                <CollapsibleSection 
                    title="上下文" 
                    count={contextItems.length} 
                    isOpen={contextOpen}
                    onOpenChange={setContextOpen}
                >
                    <ContextList items={contextItems} onItemClick={onContextClick} />
                </CollapsibleSection>
            </div>
        </SectionsContext.Provider>
    );
};

export default React.memo(ChatSidebarContent);
