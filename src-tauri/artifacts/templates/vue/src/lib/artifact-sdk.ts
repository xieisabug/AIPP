/**
 * AIPP Artifact SDK
 * 
 * 这个 SDK 用于在 Artifact 中与 AIPP 主应用通信
 * 提供数据库操作和 AI 助手调用能力
 * 
 * 使用方法：
 * 1. 在 artifact 代码中导入此 SDK
 * 2. 使用 AIPP.db 进行数据库操作
 * 3. 使用 AIPP.ai 调用 AI 助手
 * 
 * 示例：
 * ```typescript
 * import { AIPP } from '@/lib/artifact-sdk';
 * 
 * // 数据库操作
 * await AIPP.db.execute('CREATE TABLE IF NOT EXISTS todos (id INTEGER PRIMARY KEY, title TEXT)');
 * await AIPP.db.execute('INSERT INTO todos (title) VALUES (?)', ['Buy milk']);
 * const result = await AIPP.db.query('SELECT * FROM todos');
 * 
 * // AI 助手调用
 * const response = await AIPP.ai.ask('分析这些数据', JSON.stringify(result.rows));
 * ```
 */

export interface ArtifactConfig {
    db_id?: string;
    assistant_id?: number;
    artifact_id?: number;
    artifact_name?: string;
}

export interface QueryResult {
    columns: string[];
    rows: unknown[][];
    row_count: number;
}

export interface ExecuteResult {
    rows_affected: number;
    last_insert_rowid: number;
}

export interface TableInfo {
    name: string;
    sql: string;
}

export interface AssistantInfo {
    id: number;
    name: string;
    description: string;
    icon: string;
}

export interface AiResponse {
    content: string;
    model: string;
    usage?: {
        prompt_tokens?: number;
        completion_tokens?: number;
        total_tokens?: number;
    };
}

interface BridgeMessage {
    id: string;
    type: string;
    payload: unknown;
}

interface BridgeResponse {
    id: string;
    success: boolean;
    data?: unknown;
    error?: string;
}

// 生成唯一 ID
function generateId(): string {
    return `${Date.now()}-${Math.random().toString(36).slice(2, 11)}`;
}

// 等待响应的 Promise 映射
const pendingRequests = new Map<string, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();

// 当前配置
let currentConfig: ArtifactConfig = {};

// 初始化标志
let isInitialized = false;

// 监听来自父窗口的消息
function initMessageListener() {
    if (isInitialized) return;
    isInitialized = true;

    window.addEventListener('message', (event) => {
        const data = event.data;

        // 处理配置消息
        if (data?.type === 'aipp_config' && data.config) {
            currentConfig = data.config;
            console.log('[AIPP SDK] Received config:', currentConfig);
            // 触发配置更新事件
            window.dispatchEvent(new CustomEvent('aipp:config', { detail: currentConfig }));
            return;
        }

        // 处理响应消息
        const response = data as BridgeResponse;
        if (response?.id && pendingRequests.has(response.id)) {
            const { resolve, reject } = pendingRequests.get(response.id)!;
            pendingRequests.delete(response.id);

            if (response.success) {
                resolve(response.data);
            } else {
                reject(new Error(response.error || 'Unknown error'));
            }
        }
    });
}

// 发送消息到父窗口并等待响应
async function sendMessage<T>(type: string, payload: unknown): Promise<T> {
    initMessageListener();

    return new Promise((resolve, reject) => {
        const id = generateId();
        const message: BridgeMessage = { id, type, payload };

        // 设置超时
        const timeout = setTimeout(() => {
            if (pendingRequests.has(id)) {
                pendingRequests.delete(id);
                reject(new Error(`Request timeout: ${type}`));
            }
        }, 30000); // 30 秒超时

        pendingRequests.set(id, {
            resolve: (value) => {
                clearTimeout(timeout);
                resolve(value as T);
            },
            reject: (error) => {
                clearTimeout(timeout);
                reject(error);
            }
        });

        // 发送到父窗口
        if (window.parent && window.parent !== window) {
            window.parent.postMessage(message, '*');
        } else {
            pendingRequests.delete(id);
            clearTimeout(timeout);
            reject(new Error('Not running in an iframe'));
        }
    });
}

/**
 * 数据库操作 API
 */
export const db = {
    /**
     * 执行 SQL 查询语句 (SELECT)
     * @param sql SQL 语句
     * @param params 参数数组
     * @param dbId 可选的数据库 ID（默认使用配置的 db_id）
     */
    async query(sql: string, params: unknown[] = [], dbId?: string): Promise<QueryResult> {
        return sendMessage<QueryResult>('db_query', { sql, params, db_id: dbId });
    },

    /**
     * 执行 SQL 修改语句 (INSERT/UPDATE/DELETE/CREATE/DROP)
     * @param sql SQL 语句
     * @param params 参数数组
     * @param dbId 可选的数据库 ID
     */
    async execute(sql: string, params: unknown[] = [], dbId?: string): Promise<ExecuteResult> {
        return sendMessage<ExecuteResult>('db_execute', { sql, params, db_id: dbId });
    },

    /**
     * 批量执行 SQL 语句（用于初始化表结构等）
     * @param sql 多条 SQL 语句
     * @param dbId 可选的数据库 ID
     */
    async batchExecute(sql: string, dbId?: string): Promise<void> {
        return sendMessage<void>('db_batch_execute', { sql, db_id: dbId });
    },

    /**
     * 获取数据库中所有表的信息
     * @param dbId 可选的数据库 ID
     */
    async getTables(dbId?: string): Promise<TableInfo[]> {
        return sendMessage<TableInfo[]>('db_get_tables', { db_id: dbId });
    },

    /**
     * 获取指定表的列信息
     * @param tableName 表名
     * @param dbId 可选的数据库 ID
     */
    async getColumns(tableName: string, dbId?: string): Promise<string[]> {
        return sendMessage<string[]>('db_get_columns', { table_name: tableName, db_id: dbId });
    },
};

/**
 * 尝试解析可能被转义的 JSON 字符串
 * AI 返回的 content 可能是一个 JSON 字符串，需要解析
 */
function tryParseJson<T>(content: string): T | string {
    if (!content || typeof content !== 'string') {
        return content;
    }
    const trimmed = content.trim();
    // 检查是否看起来像 JSON（以 { 或 [ 开头）
    if ((trimmed.startsWith('{') && trimmed.endsWith('}')) || 
        (trimmed.startsWith('[') && trimmed.endsWith(']'))) {
        try {
            return JSON.parse(trimmed) as T;
        } catch {
            // 解析失败，返回原字符串
            return content;
        }
    }
    // 检查是否是被引号包裹的 JSON 字符串（如 "\"[...]\"" 或 "\"...\"")
    if (trimmed.startsWith('"') && trimmed.endsWith('"')) {
        try {
            const unquoted = JSON.parse(trimmed);
            if (typeof unquoted === 'string') {
                return tryParseJson<T>(unquoted);
            }
            return unquoted as T;
        } catch {
            return content;
        }
    }
    return content;
}

/**
 * AI 助手 API
 */
export const ai = {
    /**
     * 调用 AI 助手
     * @param prompt 用户提示
     * @param context 可选的上下文
     * @param options 可选配置
     */
    async ask(
        prompt: string,
        context?: string,
        options?: { systemPrompt?: string; assistantId?: number }
    ): Promise<AiResponse> {
        return sendMessage<AiResponse>('ai_ask', {
            prompt,
            context,
            system_prompt: options?.systemPrompt,
            assistant_id: options?.assistantId
        });
    },

    /**
     * 调用 AI 助手并自动解析 JSON 响应
     * @param prompt 用户提示
     * @param context 可选的上下文
     * @param options 可选配置
     * @returns 解析后的内容，如果无法解析则返回原始字符串
     */
    async askJson<T = unknown>(
        prompt: string,
        context?: string,
        options?: { systemPrompt?: string; assistantId?: number }
    ): Promise<{ content: T | string; model: string; usage?: AiResponse['usage'] }> {
        const response = await this.ask(prompt, context, options);
        return {
            content: tryParseJson<T>(response.content),
            model: response.model,
            usage: response.usage,
        };
    },

    /**
     * 解析 AI 返回的内容为 JSON
     * 可用于手动解析 ask() 返回的 content
     * @param content AI 返回的原始内容
     * @returns 解析后的对象或原始字符串
     */
    parseContent<T = unknown>(content: string): T | string {
        return tryParseJson<T>(content);
    },

    /**
     * 获取可用的助手列表
     */
    async getAssistants(): Promise<AssistantInfo[]> {
        return sendMessage<AssistantInfo[]>('get_assistants', {});
    },
};

/**
 * 配置 API
 */
export const config = {
    /**
     * 获取当前配置
     */
    get(): ArtifactConfig {
        return { ...currentConfig };
    },

    /**
     * 从主应用获取最新配置
     */
    async fetch(): Promise<ArtifactConfig> {
        const result = await sendMessage<ArtifactConfig>('get_config', {});
        currentConfig = result;
        return result;
    },

    /**
     * 监听配置更新
     * @param callback 配置更新回调
     * @returns 取消监听函数
     */
    onUpdate(callback: (config: ArtifactConfig) => void): () => void {
        const handler = (event: Event) => {
            callback((event as CustomEvent<ArtifactConfig>).detail);
        };
        window.addEventListener('aipp:config', handler);
        return () => window.removeEventListener('aipp:config', handler);
    },
};

/**
 * AIPP SDK 主入口
 */
export const AIPP = {
    db,
    ai,
    config,
};

// 也导出为默认
export default AIPP;

// 在全局 window 对象上暴露（方便不使用模块的场景）
if (typeof window !== 'undefined') {
    (window as unknown as { AIPP: typeof AIPP }).AIPP = AIPP;
}
