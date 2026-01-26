import React from 'react';
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

interface CollapsibleSectionProps {
    title: string;
    count: number;
    defaultOpen?: boolean;
    children: React.ReactNode;
}

const CollapsibleSection: React.FC<CollapsibleSectionProps> = ({
    title,
    count,
    defaultOpen = true,
    children,
}) => {
    const [isOpen, setIsOpen] = React.useState(defaultOpen);

    return (
        <Collapsible open={isOpen} onOpenChange={setIsOpen}>
            <CollapsibleTrigger className="flex items-center justify-between w-full p-2 hover:bg-muted/50 rounded-md transition-colors">
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
            <CollapsibleContent className="overflow-hidden data-[state=open]:animate-collapsible-down data-[state=closed]:animate-collapsible-up">
                {children}
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
    return (
        <div className={cn("flex flex-col gap-1", className)}>
            {/* Todo/Plan Section */}
            <CollapsibleSection title="计划" count={todos.length} defaultOpen={todos.length > 0}>
                <TodoList todos={todos} />
            </CollapsibleSection>

            {/* Artifacts Section */}
            <CollapsibleSection title="Artifact" count={artifacts.length} defaultOpen={artifacts.length > 0}>
                <ArtifactList artifacts={artifacts} onArtifactClick={onArtifactClick} />
            </CollapsibleSection>

            {/* Context Section */}
            <CollapsibleSection title="上下文" count={contextItems.length} defaultOpen={contextItems.length > 0}>
                <ContextList items={contextItems} onItemClick={onContextClick} />
            </CollapsibleSection>
        </div>
    );
};

export default React.memo(ChatSidebarContent);
