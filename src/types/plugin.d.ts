interface SystemApi { }

enum PluginType {
    AssistantType = 1,
    InterfaceType = 2,
    ApplicationType = 3,
}

interface Message {
    id: number;
    conversation_id: number;
    message_type: string;
    content: string;
    llm_model_id: number | null;
    created_time: Date;
    start_time: Date | null;
    finish_time: Date | null;
    token_count: number;
    generation_group_id?: string | null;
    parent_group_id?: string | null;
    parent_id?: number | null;
    regenerate: Array<Message> | null;
    attachment_list?: Array<any>;
}

interface AddFieldOptions {
    fieldName: string;
    label: string;
    type:
    | "select"
    | "textarea"
    | "input"
    | "password"
    | "checkbox"
    | "radio"
    | "static"
    | "custom"
    | "button"
    | "switch"
    | "model-select"
    | "mcp-select";
    fieldConfig?: FieldConfig;
}

interface AskAssistantOptions {
    question: string;
    assistantId: string;
    conversationId?: string;
    fileInfoList?: FileInfo[];
    overrideModelConfig?: Map<string, any>;
    overrideSystemPrompt?: string;
    overrideModelId?: string;
    overrideMcpConfig?: McpOverrideConfig;

    // 消息处理回调
    onCustomUserMessage?: (question: string, assistantId: string, conversationId?: string) => any;
    onCustomUserMessageComing?: (aiResponse: AiResponse) => void;
    onStreamMessageListener?: (
        payload: string,
        aiResponse: AiResponse,
        responseIsResponsingFunction: (isFinish: boolean) => void
    ) => void;
}

interface SubTaskRunResult {
    success: boolean;
    content?: string;
    error?: string;
    executionId: number;
}

// MCP 循环选项
interface McpLoopOptions {
    enabledServers: string[];
    enabledTools?: Record<string, string[]>;
    maxLoops?: number;
    toolTimeoutMs?: number;
    mcpPromptInjectionMode?: "append" | "prepend";
    continueOnToolError?: boolean;
    hardStopOnMaxLoops?: boolean;
    debug?: boolean;
}

// MCP 循环指标
interface McpLoopMetrics {
    totalCalls: number;
    successCalls: number;
    failedCalls: number;
    totalExecTimeMs: number;
    averageExecTimeMs: number;
}

// MCP 循环结果
interface McpLoopResult {
    finalText: string;
    rawModelOutput: string;
    calls: McpToolCall[];
    loops: number;
    reachedMaxLoops: boolean;
    abortReason?: string;
    metrics: McpLoopMetrics;
    debugLog?: string[];
}

// 扩展的子任务运行结果，包含 MCP 执行信息
interface SubTaskRunWithMcpResult {
    success: boolean;
    content?: string;
    error?: string;
    executionId: number;
    mcpResult?: McpLoopResult;
}

interface SubTaskRegistOptions {
    code: string;
    name: string;
    description: string;
    systemPrompt: string;
    // 子任务的图标组件（仅前端运行期使用，不会持久化到后端），例如 lucide-react 的图标组件
    // 注意：该字段不会通过 Tauri 传输，仅用于前端 UI 渲染
    iconComponent?: React.ReactNode | React.ComponentType<{ className?: string; size?: number }>;
}

interface AssistantTypeApi {
    typeRegist(pluginType: PluginType, code: number, label: string, plugin: AippAssistantTypePlugin): void;
    subTaskRegist(options: SubTaskRegistOptions): void;
    markdownRemarkRegist(component: any): void;
    changeFieldLabel(fieldName: string, label: string): void;
    addField(options: AddFieldOptions): void;
    hideField(fieldName: string): void;
    forceFieldValue(fieldName: string, value: string): void;
    addFieldTips(fieldName: string, tips: string): void;
    runLogic(callback: (assistantRunApi: AssistantRunApi) => void): void;
}

interface AssistantConfigApi {
    clearFieldValue(fieldName: string): void;
    changeFieldValue(fieldName: string, value: string | boolean, valueType: string): void;
}

interface FieldConfig {
    // default none
    position?: "query" | "body" | "header" | "none";
    // default false
    required?: boolean;
    // default false
    hidden?: boolean;
    options?: { value: string; label: string; tooltip?: string }[];
    tips?: string;
    disabled?: boolean;
    onClick?: () => void;
}

interface AssistantRunApi {
    askAssistant(options: AskAssistantOptions): Promise<AiResponse>;
    getUserInput(): string;
    getModelId(): string;
    getAssistantId(): string;
    getConversationId(): string;
    getField(assistantId: string, fieldName: string): Promise<string>;
    appendAiResponse(messageId: number, response: string): void;
    setAiResponse(messageId: number, response: string): void;
    getMcpProvider(providerId: string): Promise<McpProviderInfo | null>;
    buildMcpPrompt(providerIds: string[]): Promise<string>;
    createMessage(markdownText: string, conversationId: number): Promise<Message>;
    updateAssistantMessage(messageId: number, markdownText: string): Promise<void>;

    getMcpToolCalls(conversationId?: number): Promise<McpToolCall[]>;
    getMcpToolCall(callId: number): Promise<McpToolCall | null>;

    createConversation(systemPrompt: string, userPrompt: string): Promise<CreateConversationResponse>;
    runSubTask(code: string, taskPrompt: string): Promise<SubTaskRunResult>;
    runSubTaskWithMcpLoop(code: string, taskPrompt: string, options: McpLoopOptions): Promise<SubTaskRunWithMcpResult>;
}

interface AiResponse {
    conversation_id: number;
    request_prompt_result_with_context: string;
}

interface CreateConversationResponse {
    conversation_id: number;
    user_message_id: number | null;
    system_message_id: number | null;
}

interface McpToolInfo {
    name: string;
    description: string;
    parameters: string;
    isEnabled: boolean;
    isAutoRun: boolean;
}

interface McpProviderInfo {
    id: string;
    name: string;
    description?: string;
    transportType: string;
    isEnabled: boolean;
    tools: McpToolInfo[];
}

// MCP配置覆盖
interface McpOverrideConfig {
    // 覆盖所有工具的自动运行配置（优先级高于toolAutoRun）
    allToolAutoRun?: boolean;
    // 覆盖特定工具的自动运行配置
    toolAutoRun?: Record<string, boolean>;  // "serverId/toolName" -> autoRun
    // 覆盖是否使用原生工具调用
    useNativeToolcall?: boolean;
    // 自定义MCP工具调用超时时间
    toolCallTimeout?: number;
}

// 保留原有的McpToolCall接口用于查询
interface McpToolCall {
    id: number;
    conversation_id: number;
    message_id?: number;
    server_name: string;
    tool_name: string;
    parameters: string;
    status: "pending" | "running" | "success" | "failed";
    result?: string;
    error?: string;
    created_time: Date;
    started_time?: Date;
    finished_time?: Date;
}

declare class Config {
    name: string;
    type: string[];
}

declare class AippPlugin {
    onPluginLoad(systemApi: SystemApi): void;
    renderComponent?(): React.ReactNode;
    config(): Config;
}

declare class AippAssistantTypePlugin {
    onAssistantTypeInit(assistantTypeApi: AssistantTypeApi): void;
    onAssistantTypeSelect(assistantTypeApi: AssistantTypeApi): void;
    onAssistantTypeRun(assistantRunApi: AssistantRunApi): void;
}
