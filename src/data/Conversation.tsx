export interface Conversation {
    id: number;
    name: string;
    assistant_id: number | null;
    assistant_name: string;
    created_time: Date;
}

// 新增：用于 get_conversation_with_messages API 的响应结构
export interface ConversationWithMessages {
    conversation: Conversation;
    messages: Array<Message>;
}

export interface Message {
    id: number;
    conversation_id: number;
    message_type: string;
    content: string;
    llm_model_id: number | null;
    created_time: Date;
    start_time: Date | null;
    finish_time: Date | null;
    token_count: number;
    input_token_count: number;
    output_token_count: number;
    generation_group_id?: string | null;
    parent_group_id?: string | null; // 添加 parent_group_id 字段
    parent_id?: number | null; // 添加 parent_id 字段
    regenerate: Array<Message> | null;
    attachment_list?: Array<any>; // 添加附件列表字段
    tool_calls_json?: string | null; // 添加工具调用 JSON 字段
    // 性能指标
    first_token_time?: Date | null;
    ttft_ms?: number | null;
    tps?: number | null;
}

// 流式事件数据类型
export interface StreamEvent {
    message_id: number;
    message_type: 'reasoning' | 'response' | 'error';
    content: string;
    is_done: boolean;
    duration_ms?: number; // 后端提供的持续时间
    end_time?: Date; // 后端提供的结束时间
    // Token 计数（可选，仅在 is_done=true 时有值）
    token_count?: number;
    input_token_count?: number;
    output_token_count?: number;
    // 性能指标（可选，仅在 is_done=true 时有值）
    ttft_ms?: number;
    tps?: number;
}

// 新增：Conversation 事件类型
export interface ConversationEvent {
    type: string;
    data: any;
}

export interface MessageAddEvent {
    message_id: number;
    message_type: string;
    temp_message_id: number; // 用于取消操作的临时ID
}

export interface MessageUpdateEvent {
    message_id: number;
    message_type: string;
    content: string;
    is_done: boolean;
    // Token 计数（可选，仅在 is_done=true 时有值）
    token_count?: number;
    input_token_count?: number;
    output_token_count?: number;
    // 性能指标（可选，仅在 is_done=true 时有值）
    ttft_ms?: number;
    tps?: number;
}

export interface MessageTypeEndEvent {
    message_id: number;
    message_type: string;
    duration_ms: number;
    end_time: Date;
}

export interface GroupMergeEvent {
    original_group_id: string;
    new_group_id: string;
    is_regeneration: boolean;
    first_message_id?: number;
    conversation_id?: number;
}

export interface MCPToolCallUpdateEvent {
    call_id: number;
    conversation_id: number;
    status: 'pending' | 'executing' | 'success' | 'failed';
    server_name?: string;
    tool_name?: string;
    parameters?: string;
    result?: string;
    error?: string;
    started_time?: Date;
    finished_time?: Date;
}

export interface ConversationCancelEvent {
    conversation_id: number;
    cancelled_at: Date;
}

export interface StreamCompleteEvent {
    conversation_id: number;
    response_message_id?: number | null;
    reasoning_message_id?: number | null;
    has_response: boolean;
    has_reasoning: boolean;
    response_length?: number;
    reasoning_length?: number;
}

// 活动焦点类型 - 用于控制闪亮边框的显示
export type ActivityFocus =
    | { focus_type: 'none' }
    | { focus_type: 'user_pending'; message_id: number }
    | { focus_type: 'assistant_streaming'; message_id: number }
    | { focus_type: 'mcp_executing'; call_id: number };

// 活动焦点变化事件
export interface ActivityFocusChangeEvent {
    conversation_id: number;
    focus: ActivityFocus;
}

// 消息类型枚举
export type MessageType = 'system' | 'user' | 'assistant' | 'reasoning' | 'response' | 'error';

export interface AddAttachmentResponse {
    attachment_id: number;
}

export interface FileInfo {
    id: number;
    name: string;
    path: string;
    type: AttachmentType;
    thumbnail?: string;
}

export enum AttachmentType { // 添加AttachmentType枚举
    Image = 1,
    Text = 2,
    PDF = 3,
    Word = 4,
    PowerPoint = 5,
    Excel = 6,
}

// Token统计相关类型
export interface ConversationTokenStats {
    total_tokens: number;
    input_tokens: number;
    output_tokens: number;
    by_model: ModelTokenBreakdown[];
    message_count: number;
    // 按消息类型统计
    system_message_count: number;
    user_message_count: number;
    response_message_count: number;
    reasoning_message_count: number;
    tool_result_message_count: number;
    // 性能指标统计
    avg_ttft_ms?: number;
    avg_tps?: number;
}

export interface ModelTokenBreakdown {
    model_id: number | null;
    model_name: string;
    total_tokens: number;
    input_tokens: number;
    output_tokens: number;
    message_count: number;
    percentage?: number; // 用于UI显示的百分比
    // 性能指标统计
    avg_ttft_ms?: number;
    avg_tps?: number;
}

export interface MessageTokenStats {
    message_id: number;
    total_tokens: number;
    input_tokens: number;
    output_tokens: number;
    model_name: string | null;
    // 性能指标
    ttft_ms?: number;
    tps?: number;
}

// ============ 对话导出相关类型 ============

// 导出选项接口
export interface ConversationExportOptions {
    includeSystemPrompt: boolean;    // 是否导出 system prompt
    includeReasoning: boolean;       // 是否导出 reasoning
    includeToolParams: boolean;      // 是否导出工具使用参数
    includeToolResults: boolean;     // 是否导出工具使用结果
}

// 工具调用数据接口（从 tool_calls_json 解析）
export interface ToolCallData {
    call_id: string;
    fn_name: string;
    fn_arguments: Record<string, unknown>;
}

// MCP 工具调用接口
export interface MCPToolCall {
    id: number;
    conversation_id: number;
    message_id?: number;
    server_id: number;
    server_name: string;
    tool_name: string;
    parameters: string;
    status: 'pending' | 'executing' | 'success' | 'failed';
    result?: string;
    error?: string;
    created_time: string;
    started_time?: string;
    finished_time?: string;
}

export interface ConversationSearchHit {
    conversation_id: number;
    conversation_name: string;
    assistant_name: string;
    message_id: number | null;
    message_type: string | null;
    created_time: Date;
    snippet: string;
    hit_type: "title" | "summary" | "message";
}
