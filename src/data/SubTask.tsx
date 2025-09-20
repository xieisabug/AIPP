// Sub Task 相关的 TypeScript 类型定义

export interface SubTaskDefinition {
    id: number;
    name: string;
    code: string;
    description: string;
    // 仅前端使用：注册期可绑定的图标组件，不会从后端返回
    iconComponent?: React.ReactNode | React.ComponentType<{ className?: string; size?: number }>;
    system_prompt: string;
    plugin_source: "mcp" | "plugin";
    source_id: number;
    is_enabled: boolean;
    created_time: Date;
    updated_time: Date;
}

export interface SubTaskExecutionSummary {
    id: number;
    task_code: string;
    task_name: string;
    task_prompt: string;
    status: "pending" | "running" | "success" | "failed" | "cancelled";
    created_time: Date;
    token_count: number;
}

export interface SubTaskExecutionDetail extends SubTaskExecutionSummary {
    result_content?: string;
    error_message?: string;
    llm_model_name?: string;
    input_token_count: number;
    output_token_count: number;
    started_time?: Date;
    finished_time?: Date;
    mcp_result_json?: string; // raw JSON string persisted from backend (optional)
}

export interface MCPToolCallUI {
    id: number;
    conversation_id: number;
    message_id?: number;
    subtask_id?: number;
    server_id: number;
    server_name: string;
    tool_name: string;
    parameters: string;
    status: "pending" | "executing" | "success" | "failed";
    result?: string;
    error?: string;
    created_time: Date;
    started_time?: Date;
    finished_time?: Date;
    llm_call_id?: string;
    assistant_message_id?: number;
}

export interface CreateSubTaskRequest {
    task_code: string;
    task_prompt: string;
    parent_conversation_id: number;
    parent_message_id?: number;
    source_id: number;
    ai_params?: SubTaskExecutionParams;
}

export interface SubTaskExecutionParams {
    temperature?: number;
    top_p?: number;
    max_tokens?: number;
    custom_model_id?: number;
}

export interface RegisterSubTaskDefinitionRequest {
    name: string;
    code: string;
    description: string;
    system_prompt: string;
    plugin_source: "mcp" | "plugin";
    source_id: number;
}

export interface UpdateSubTaskDefinitionRequest {
    id: number;
    name?: string;
    description?: string;
    system_prompt?: string;
    is_enabled?: boolean;
    source_id: number; // 用于鉴权
}

// 运行期的子任务图标组件注册表（全局单例，避免 HMR/多实例导致订阅与注册不一致）
import * as React from "react";

export type SubTaskIconComponent = React.ReactNode | React.ComponentType<{ className?: string; size?: number }>;

type Listener = () => void;
type SubTaskIconStore = {
    map: Map<string, SubTaskIconComponent>;
    listeners: Set<Listener>;
    version: number;
};

const STORE_KEY = "__aipp_subtask_icon_registry__" as const;

function normalizeCode(code: string): string {
    return (code || "").trim().toLowerCase();
}

function getIconStore(): SubTaskIconStore {
    const g = globalThis as any;
    if (!g[STORE_KEY]) {
        g[STORE_KEY] = {
            map: new Map<string, SubTaskIconComponent>(),
            listeners: new Set<Listener>(),
            version: 0,
        } as SubTaskIconStore;
    }
    return g[STORE_KEY] as SubTaskIconStore;
}

function notifyIconRegistryChanged() {
    const store = getIconStore();
    store.version++;
    store.listeners.forEach((l) => {
        try {
            l();
        } catch (e) {
            console.warn("[SubTaskIconStore] listener error", e);
        }
    });
}

export function subscribeSubTaskIconRegistry(listener: Listener) {
    const store = getIconStore();
    store.listeners.add(listener);
    return () => {
        const s = getIconStore();
        s.listeners.delete(listener);
    };
}

export function registerSubTaskIconComponent(code: string, iconComp: SubTaskIconComponent) {
    const store = getIconStore();
    if (!code) return;
    const key = normalizeCode(code);
    const prev = store.map.get(key);
    // 仅在引用变化时才更新与通知，避免无谓的重渲染
    if (prev !== iconComp) {
        store.map.set(key, iconComp);
        notifyIconRegistryChanged();
    }
}

export function getSubTaskIconComponent(code: string): SubTaskIconComponent | undefined {
    const key = normalizeCode(code);
    return getIconStore().map.get(key);
}

export function useSubTaskIcon(code: string): SubTaskIconComponent | undefined {
    // 订阅版本号变更，确保注册后触发重渲染
    const version = React.useSyncExternalStore(
        subscribeSubTaskIconRegistry,
        () => getIconStore().version,
    );
    void version;
    const key = normalizeCode(code);
    return getIconStore().map.get(key);
}

// 调试辅助：列出所有已注册的子任务图标 code（规范化后的 key）
export function debugListRegisteredSubTaskIcons(): string[] {
    return Array.from(getIconStore().map.keys());
}

// 可选：将调试方法暴露到全局，便于在 DevTools 中快速调用
try {
    (globalThis as any).AIPP_DEBUG_LIST_SUBTASK_ICONS = debugListRegisteredSubTaskIcons;
} catch { }

// 调试辅助：返回规范化 code、是否存在、已注册 keys 与当前版本
export function debugGetSubTaskIconState(code: string): { normalized: string; has: boolean; keys: string[]; version: number } {
    const store = getIconStore();
    const key = normalizeCode(code);
    return { normalized: key, has: store.map.has(key), keys: Array.from(store.map.keys()), version: store.version };
}
try {
    (globalThis as any).AIPP_DEBUG_GET_SUBTASK_ICON_STATE = debugGetSubTaskIconState;
} catch { }

export interface SubTaskStatusUpdateEvent {
    execution_id: number;
    task_code: string;
    task_name: string;
    parent_conversation_id: number;
    parent_message_id?: number;
    status: "pending" | "running" | "success" | "failed" | "cancelled";
    result_content?: string;
    error_message?: string;
    token_count?: number;
    started_time?: Date;
    finished_time?: Date;
}

export interface ListSubTaskDefinitionsParams {
    plugin_source?: "mcp" | "plugin";
    source_id?: number;
    is_enabled?: boolean;
}

export interface ListSubTaskExecutionsParams {
    parent_conversation_id: number;
    parent_message_id?: number;
    status?: "pending" | "running" | "success" | "failed" | "cancelled";
    source_id?: number;
    page?: number;
    page_size?: number;
}

// Hook 和组件相关的类型
export interface UseSubTaskManagerOptions {
    conversation_id: number;
    message_id?: number;
    // source_id 在UI层面不需要，只在MCP/plugin开发时需要
}

export interface UseSubTaskEventsOptions {
    conversation_id: number;
    onStatusUpdate?: (event: SubTaskStatusUpdateEvent) => void;
    onTaskCompleted?: (execution: SubTaskExecutionDetail) => void;
    onTaskFailed?: (execution: SubTaskExecutionDetail) => void;
}

export interface SubTaskListProps {
    conversation_id: number;
    message_id?: number;
    source_id?: number;
    className?: string;
}

export interface SubTaskItemProps {
    execution: SubTaskExecutionSummary;
    onViewDetail?: (execution: SubTaskExecutionSummary) => void;
    onCancel?: (execution_id: number) => void;
}

export interface CreateSubTaskDialogProps {
    isOpen: boolean;
    onClose: () => void;
    conversation_id: number;
    message_id?: number;
    source_id: number;
    availableDefinitions: SubTaskDefinition[];
    onTaskCreated?: (execution_id: number) => void;
}

export interface SubTaskDetailDialogProps {
    isOpen: boolean;
    onClose: () => void;
    execution_id: number;
    source_id: number;
}

export interface SubTaskStatusIndicatorProps {
    status: "pending" | "running" | "success" | "failed" | "cancelled";
    size?: "sm" | "md" | "lg";
}

// 服务层接口
export interface SubTaskService {
    // 任务定义管理
    registerDefinition: (request: RegisterSubTaskDefinitionRequest) => Promise<number>;
    listDefinitions: (params?: ListSubTaskDefinitionsParams) => Promise<SubTaskDefinition[]>;
    getDefinition: (code: string, source_id: number) => Promise<SubTaskDefinition | null>;
    updateDefinition: (request: UpdateSubTaskDefinitionRequest) => Promise<void>;
    deleteDefinition: (id: number, source_id: number) => Promise<void>;

    // 任务执行管理
    createExecution: (request: CreateSubTaskRequest) => Promise<number>;
    listExecutions: (params: ListSubTaskExecutionsParams) => Promise<SubTaskExecutionSummary[]>;
    getExecutionDetail: (execution_id: number, source_id: number) => Promise<SubTaskExecutionDetail | null>;
    cancelExecution: (execution_id: number, source_id: number) => Promise<void>;
    // UI helpers
    getMcpCallsForExecution?: (execution_id: number) => Promise<MCPToolCallUI[]>;
}
