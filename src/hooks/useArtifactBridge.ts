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
        // 验证来源
        if (!isValidOrigin(event.origin)) {
            console.warn('[ArtifactBridge] Invalid origin:', event.origin);
            return;
        }

        const message = event.data as BridgeMessage;
        if (!message || !message.id || !message.type) {
            return;
        }

        console.log('[ArtifactBridge] Received message:', message.type);

        let response: BridgeResponse;

        try {
            const currentConfig = configRef.current;
            const dbId = currentConfig.db_id;
            const assistantId = currentConfig.assistant_id;

            switch (message.type) {
                case 'db_query': {
                    const payload = message.payload as { sql: string; params?: unknown[]; db_id?: string };
                    const targetDbId = payload.db_id || dbId;
                    if (!targetDbId) {
                        throw new Error('No database ID configured');
                    }
                    const result = await handleDbQuery(targetDbId, payload.sql, payload.params || []);
                    response = { id: message.id, success: true, data: result };
                    break;
                }

                case 'db_execute': {
                    const payload = message.payload as { sql: string; params?: unknown[]; db_id?: string };
                    const targetDbId = payload.db_id || dbId;
                    if (!targetDbId) {
                        throw new Error('No database ID configured');
                    }
                    const result = await handleDbExecute(targetDbId, payload.sql, payload.params || []);
                    response = { id: message.id, success: true, data: result };
                    break;
                }

                case 'db_batch_execute': {
                    const payload = message.payload as { sql: string; db_id?: string };
                    const targetDbId = payload.db_id || dbId;
                    if (!targetDbId) {
                        throw new Error('No database ID configured');
                    }
                    await handleDbBatchExecute(targetDbId, payload.sql);
                    response = { id: message.id, success: true };
                    break;
                }

                case 'db_get_tables': {
                    const payload = message.payload as { db_id?: string } | undefined;
                    const targetDbId = payload?.db_id || dbId;
                    if (!targetDbId) {
                        throw new Error('No database ID configured');
                    }
                    const result = await handleDbGetTables(targetDbId);
                    response = { id: message.id, success: true, data: result };
                    break;
                }

                case 'db_get_columns': {
                    const payload = message.payload as { table_name: string; db_id?: string };
                    const targetDbId = payload.db_id || dbId;
                    if (!targetDbId) {
                        throw new Error('No database ID configured');
                    }
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
                    const targetAssistantId = payload.assistant_id || assistantId;
                    if (!targetAssistantId) {
                        throw new Error('未配置 AI 助手，请在 Artifact 设置中选择一个助手');
                    }
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

    // 发送配置到 iframe
    const sendConfig = useCallback(() => {
        if (iframeRef.current?.contentWindow) {
            const configMessage = {
                type: 'aipp_config',
                config: configRef.current
            };
            // 尝试多个可能的 origin
            for (const origin of allowedOrigins) {
                try {
                    iframeRef.current.contentWindow.postMessage(configMessage, origin + ':*');
                } catch {
                    // 忽略错误，尝试下一个
                }
            }
            // 也尝试 *（不推荐，但作为后备）
            try {
                iframeRef.current.contentWindow.postMessage(configMessage, '*');
            } catch {
                // 忽略
            }
            setIsConnected(true);
        }
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
