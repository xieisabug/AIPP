import React from 'react';
import { CheckCircle2, Circle, Loader2 } from 'lucide-react';
import { TodoItem } from './types';
import { cn } from '@/utils/utils';

interface TodoListProps {
    todos: TodoItem[];
    className?: string;
}

const TodoStatusIcon: React.FC<{ status: TodoItem['status'] }> = ({ status }) => {
    switch (status) {
        case 'completed':
            return <CheckCircle2 className="h-4 w-4 text-green-500 flex-shrink-0" />;
        case 'in_progress':
            return <Loader2 className="h-4 w-4 text-blue-500 animate-spin flex-shrink-0" />;
        case 'pending':
        default:
            return <Circle className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
    }
};

const TodoList: React.FC<TodoListProps> = ({ todos, className }) => {
    if (todos.length === 0) {
        return (
            <div className={cn("p-3 text-sm text-muted-foreground text-center", className)}>
                暂无计划
            </div>
        );
    }

    // Calculate progress
    const completed = todos.filter(t => t.status === 'completed').length;
    const total = todos.length;
    const progressPercent = total > 0 ? Math.round((completed / total) * 100) : 0;

    return (
        <div className={cn("flex flex-col gap-2", className)}>
            {/* Progress bar */}
            <div className="flex items-center gap-2 px-3 pt-2">
                <div className="flex-1 h-1.5 bg-muted rounded-full overflow-hidden">
                    <div 
                        className="h-full bg-green-500 transition-all duration-300"
                        style={{ width: `${progressPercent}%` }}
                    />
                </div>
                <span className="text-xs text-muted-foreground whitespace-nowrap">
                    {completed}/{total}
                </span>
            </div>

            {/* Todo items */}
            <div className="flex flex-col gap-1 px-2 pb-2">
                {todos.map((todo, index) => (
                    <div 
                        key={index}
                        className={cn(
                            "flex items-start gap-2 p-2 rounded-md transition-colors",
                            "hover:bg-muted/50",
                            todo.status === 'in_progress' && "bg-blue-50 dark:bg-blue-950/30"
                        )}
                    >
                        <TodoStatusIcon status={todo.status} />
                        <div className="flex-1 min-w-0">
                            <p className={cn(
                                "text-sm leading-tight",
                                todo.status === 'completed' && "line-through text-muted-foreground"
                            )}>
                                {todo.content}
                            </p>
                            {todo.status === 'in_progress' && todo.activeForm && (
                                <p className="text-xs text-blue-600 dark:text-blue-400 mt-0.5">
                                    {todo.activeForm}
                                </p>
                            )}
                        </div>
                    </div>
                ))}
            </div>
        </div>
    );
};

export default React.memo(TodoList);
