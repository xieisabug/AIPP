import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type {
    ArtifactBridgeConfig,
    DbQueryResult,
    DbExecuteResult,
    DbTableInfo,
    AssistantBasicInfo,
    AiAskResponse,
} from '@/data/ArtifactCollection';

export interface BridgeMessage {
    id: string;
    type: 'db_query' | 'db_execute' | 'db_batch_execute' | 'db_get_tables' | 'db_get_columns' | 'ai_ask' | 'get_config' | 'get_assistants';
    payload: unknown;
}

export interface BridgeResponse {
    id: string;
    success: boolean;
    data?: unknown;
    error?: string;
}

export interface UseArtifactBridgeOptions {
    /** iframe 引用 */
    iframeRef: React.RefObject<HTMLIFrameElement | null>;
    /** artifact 配置 */
    config: ArtifactBridgeConfig;
    /** 允许的 origin（默认 localhost） */
    allowedOrigins?: string[];
}

export interface UseArtifactBridgeReturn {
    /** 是否已连接 */
    isConnected: boolean;
    /** 发送配置到 iframe */
    sendConfig: () => void;
}

/**
 * 解析本次请求实际使用的绑定资源。
 *
 * 数据库和助手只认宿主侧绑定（保存/运行配置里用户选定的值）；页面代码可以重复传入同一个值，
 * 但不能借 payload 换成别的数据库或助手。
 */
export function resolveBoundTarget<T extends string | number>(
    bound: T | null | undefined,
    requested: unknown,
    kind: 'db' | 'assistant',
): T {
    const hasRequest = requested !== undefined && requested !== null && requested !== '';
    if (bound === undefined || bound === null || bound === '') {
        if (hasRequest) {
            throw new Error(kind === 'db'
                ? '此 Artifact 未绑定数据库，不能访问其他数据库'
                : '此 Artifact 未绑定 AI 助手，不能调用其他助手');
        }
        throw new Error(kind === 'db'
            ? 'No database ID configured'
            : '未配置 AI 助手，请在 Artifact 设置中选择一个助手');
    }
    if (hasRequest && String(requested) !== String(bound)) {
        throw new Error(kind === 'db'
            ? `Artifact 只能访问绑定的数据库「${bound}」`
            : 'Artifact 只能调用绑定的 AI 助手');
    }
    return bound;
}

/** iframe 当前地址的 origin；取不到时返回 null（不广播配置）。 */
export function iframeTargetOrigin(iframe: HTMLIFrameElement | null, allowedOrigins: string[]): string | null {
    const src = iframe?.src;
    if (!src) return null;
    try {
        const origin = new URL(src).origin;
        return allowedOrigins.some((allowed) => origin.startsWith(allowed)) ? origin : null;
    } catch {
        return null;
    }
}

/**
 * Artifact Bridge Hook
 * 
 * 负责处理 iframe 内的 artifact 与主应用之间的通信
 * 通过 postMessage 接收请求，调用 Tauri 命令，返回结果
 */
export function useArtifactBridge(options: UseArtifactBridgeOptions): UseArtifactBridgeReturn {
    const { iframeRef, config, allowedOrigins = ['http://localhost', 'https://localhost'] } = options;
    const [isConnected, setIsConnected] = useState(false);
    const configRef = useRef(config);

    // 保持 config 最新
    useEffect(() => {
        configRef.current = config;
    }, [config]);

    // 验证消息来源
    const isValidOrigin = useCallback((origin: string): boolean => {
        return allowedOrigins.some(allowed => origin.startsWith(allowed));
    }, [allowedOrigins]);

    // 处理数据库查询
    const handleDbQuery = useCallback(async (dbId: string, sql: string, params: unknown[]): Promise<DbQueryResult> => {
        return invoke<DbQueryResult>('artifact_db_query', {
            request: { db_id: dbId, sql, params }
        });
    }, []);

    // 处理数据库执行
    const handleDbExecute = useCallback(async (dbId: string, sql: string, params: unknown[]): Promise<DbExecuteResult> => {
        return invoke<DbExecuteResult>('artifact_db_execute', {
            request: { db_id: dbId, sql, params }
        });
    }, []);

    // 处理批量执行
    const handleDbBatchExecute = useCallback(async (dbId: string, sql: string): Promise<void> => {
        return invoke<void>('artifact_db_batch_execute', {
            request: { db_id: dbId, sql }
        });
    }, []);

    // 获取表列表
    const handleDbGetTables = useCallback(async (dbId: string): Promise<DbTableInfo[]> => {
        return invoke<DbTableInfo[]>('artifact_db_get_tables', { dbId });
    }, []);

    // 获取列信息
    const handleDbGetColumns = useCallback(async (dbId: string, tableName: string): Promise<string[]> => {
        return invoke<string[]>('artifact_db_get_columns', { dbId, tableName });
    }, []);

    // 调用 AI 助手
    const handleAiAsk = useCallback(async (
        assistantId: number,
        prompt: string,
        context?: string,
        systemPrompt?: string
    ): Promise<AiAskResponse> => {
        return invoke<AiAskResponse>('artifact_ai_ask', {
            request: {
                assistant_id: assistantId,
                prompt,
                context,
                system_prompt: systemPrompt
            }
        });
    }, []);

    // 获取助手列表
    const handleGetAssistants = useCallback(async (): Promise<AssistantBasicInfo[]> => {
        return invoke<AssistantBasicInfo[]>('artifact_get_assistants');
    }, []);

    // 处理 Bridge 消息
    const handleMessage = useCallback(async (event: MessageEvent) => {
        // 只处理绑定 iframe 发来的桥接请求；同窗口里其他 iframe（Draw.io、诊断脚本等）的消息一律忽略
        const iframeWindow = iframeRef.current?.contentWindow;
        if (!iframeWindow || event.source !== iframeWindow) {
            return;
        }
        if (!isValidOrigin(event.origin)) {
            return;
        }

        const message = event.data as BridgeMessage;
        if (!message || !message.id || !message.type) {
            return;
        }

        let response: BridgeResponse;

        try {
            const currentConfig = configRef.current;
            const dbId = currentConfig.db_id;
            const assistantId = currentConfig.assistant_id;

            switch (message.type) {
                case 'db_query': {
                    const payload = message.payload as { sql: string; params?: unknown[]; db_id?: string };
                    const targetDbId = resolveBoundTarget(dbId, payload.db_id, 'db');
                    const result = await handleDbQuery(targetDbId, payload.sql, payload.params || []);
                    response = { id: message.id, success: true, data: result };
                    break;
                }

                case 'db_execute': {
                    const payload = message.payload as { sql: string; params?: unknown[]; db_id?: string };
                    const targetDbId = resolveBoundTarget(dbId, payload.db_id, 'db');
                    const result = await handleDbExecute(targetDbId, payload.sql, payload.params || []);
                    response = { id: message.id, success: true, data: result };
                    break;
                }

                case 'db_batch_execute': {
                    const payload = message.payload as { sql: string; db_id?: string };
                    const targetDbId = resolveBoundTarget(dbId, payload.db_id, 'db');
                    await handleDbBatchExecute(targetDbId, payload.sql);
                    response = { id: message.id, success: true };
                    break;
                }

                case 'db_get_tables': {
                    const payload = message.payload as { db_id?: string } | undefined;
                    const targetDbId = resolveBoundTarget(dbId, payload?.db_id, 'db');
                    const result = await handleDbGetTables(targetDbId);
                    response = { id: message.id, success: true, data: result };
                    break;
                }

                case 'db_get_columns': {
                    const payload = message.payload as { table_name: string; db_id?: string };
                    const targetDbId = resolveBoundTarget(dbId, payload.db_id, 'db');
                    const result = await handleDbGetColumns(targetDbId, payload.table_name);
                    response = { id: message.id, success: true, data: result };
                    break;
                }

                case 'ai_ask': {
                    const payload = message.payload as {
                        prompt: string;
                        context?: string;
                        system_prompt?: string;
                        assistant_id?: number;
                    };
                    const targetAssistantId = resolveBoundTarget(assistantId, payload.assistant_id, 'assistant');
                    const result = await handleAiAsk(
                        targetAssistantId,
                        payload.prompt,
                        payload.context,
                        payload.system_prompt
                    );
                    response = { id: message.id, success: true, data: result };
                    break;
                }

                case 'get_assistants': {
                    const result = await handleGetAssistants();
                    response = { id: message.id, success: true, data: result };
                    break;
                }

                case 'get_config': {
                    response = { id: message.id, success: true, data: currentConfig };
                    break;
                }

                default:
                    response = { id: message.id, success: false, error: `Unknown message type: ${message.type}` };
            }
        } catch (error) {
            console.error('[ArtifactBridge] Error handling message:', error);
            response = {
                id: message.id,
                success: false,
                error: error instanceof Error ? error.message : String(error)
            };
        }

        // 发送响应回 iframe
        if (iframeRef.current?.contentWindow) {
            iframeRef.current.contentWindow.postMessage(response, event.origin);
        }
    }, [isValidOrigin, handleDbQuery, handleDbExecute, handleDbBatchExecute, handleDbGetTables, handleDbGetColumns, handleAiAsk, handleGetAssistants, iframeRef]);

    // 发送配置到 iframe：只发给 iframe 当前地址的 origin，页面跳到外站后不会再收到绑定信息
    const sendConfig = useCallback(() => {
        const iframe = iframeRef.current;
        const targetOrigin = iframeTargetOrigin(iframe, allowedOrigins);
        if (!iframe?.contentWindow || !targetOrigin) {
            return;
        }
        iframe.contentWindow.postMessage({ type: 'aipp_config', config: configRef.current }, targetOrigin);
        setIsConnected(true);
    }, [iframeRef, allowedOrigins]);

    // 监听消息
    useEffect(() => {
        window.addEventListener('message', handleMessage);
        return () => {
            window.removeEventListener('message', handleMessage);
        };
    }, [handleMessage]);

    // iframe 加载时发送配置
    useEffect(() => {
        const iframe = iframeRef.current;
        if (iframe) {
            const handleLoad = () => {
                // 延迟发送以确保 iframe 内容已加载
                setTimeout(sendConfig, 100);
            };
            iframe.addEventListener('load', handleLoad);
            return () => {
                iframe.removeEventListener('load', handleLoad);
            };
        }
    }, [iframeRef, sendConfig]);

    return {
        isConnected,
        sendConfig
    };
}
