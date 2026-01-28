import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { TodoItem, TodoUpdateEvent } from '../components/chat-sidebar/types';

interface UseTodoListOptions {
    conversationId: number | null;
}

interface UseTodoListReturn {
    todos: TodoItem[];
    loading: boolean;
    error: string | null;
    refresh: () => Promise<void>;
}

interface BackendTodoItem {
    content: string;
    status: string;
    active_form: string;
}

function mapBackendTodo(item: BackendTodoItem): TodoItem {
    let status: TodoItem['status'] = 'pending';
    if (item.status === 'in_progress') {
        status = 'in_progress';
    } else if (item.status === 'completed') {
        status = 'completed';
    }
    return {
        content: item.content,
        status,
        activeForm: item.active_form,
    };
}

export function useTodoList({ conversationId }: UseTodoListOptions): UseTodoListReturn {
    const [todos, setTodos] = useState<TodoItem[]>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const refresh = useCallback(async () => {
        if (conversationId === null || conversationId <= 0) {
            setTodos([]);
            return;
        }

        setLoading(true);
        setError(null);

        try {
            const result = await invoke<BackendTodoItem[]>('get_todos', {
                conversationId,
            });
            setTodos(result.map(mapBackendTodo));
        } catch (err) {
            console.error('Failed to fetch todos:', err);
            setError(err instanceof Error ? err.message : String(err));
        } finally {
            setLoading(false);
        }
    }, [conversationId]);

    // Initial fetch
    useEffect(() => {
        refresh();
    }, [refresh]);

    // Listen for todo_update events
    useEffect(() => {
        if (conversationId === null || conversationId <= 0) {
            return;
        }

        const unlistenPromise = listen<TodoUpdateEvent>('todo_update', (event) => {
            if (event.payload.conversation_id === conversationId) {
                setTodos(event.payload.todos);
            }
        });

        return () => {
            unlistenPromise.then((unlisten) => unlisten());
        };
    }, [conversationId]);

    return { todos, loading, error, refresh };
}
