import { useEffect, useRef, useCallback, useState } from 'react';
import { listen, emit, type EventCallback, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

/**
 * 只收发给本窗口的预览事件。后端按目标窗口 `emit_to`，这里按本窗口标签监听：
 * 侧栏、独立预览窗口、聊天窗口分栏各自的预览互不串内容；全局广播的事件（如安装完成）照常收到。
 */
function listenHere<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
    let label: string | undefined;
    try {
        label = getCurrentWebviewWindow().label;
    } catch {
        label = undefined;
    }
    return label ? listen<T>(event, handler, { target: label }) : listen<T>(event, handler);
}

export interface LogLine {
    type: 'log' | 'error' | 'success';
    message: string;
}

export interface ArtifactData {
    id?: number;
    message_id?: number;
    name?: string;
    icon?: string;
    description?: string;
    type: string;
    original_code: string;
    tags?: string;
    created_time?: string;
    last_used_time?: string;
    use_count?: number;
    db_id?: string;
    assistant_id?: number;
    conversation_id?: number;
    request_id?: string;
    /** 会话 Artifact 的身份（由后端按版本快照附上）。 */
    artifact_key?: string;
    version?: number;
    entry_file?: string;
    multi_file?: boolean;
    language?: string;
    /** 合集里从会话收藏的小工具：来源会话与收藏时的版本。 */
    source_conversation_id?: number;
    source_version?: number;
}

export interface EnvironmentCheckData {
    tool: string;
    message: string;
    lang: string;
    input_str: string;
}

export interface UseArtifactEventsOptions {
    /** 窗口类型: 'preview' 用于 artifact_preview 窗口, 'artifact' 用于 artifact 窗口 */
    windowType: 'preview' | 'artifact';
    /** 可选的预览窗口名，用于区分 sidebar 和 artifact_preview 的 ready 信号 */
    readyWindow?: string;
    /** 处理 artifact 数据的回调 */
    onArtifactData?: (data: ArtifactData) => void;
    /** 处理重定向 URL 的回调 */
    onRedirect?: (url: string) => void;
    /** 处理环境检查的回调 */
    onEnvironmentCheck?: (data: EnvironmentCheckData) => void;
    /** 处理环境安装开始的回调 */
    onEnvironmentInstallStarted?: (data: { tool: string; lang: string; input_str: string }) => void;
    /** 处理 Bun 安装完成的回调 */
    onBunInstallFinished?: (success: boolean) => void;
    /** 处理 uv 安装完成的回调 */
    onUvInstallFinished?: (success: boolean) => void;
    /** 处理重置事件的回调（切换 artifact 时调用） */
    onReset?: () => void;
}

export interface UseArtifactEventsReturn {
    /** 日志列表 */
    logs: LogLine[];
    /** 添加日志 */
    addLog: (type: LogLine['type'], message: string) => void;
    /** 清空日志 */
    clearLogs: () => void;
    /** 是否已接收到数据 */
    hasReceivedData: boolean;
    /** 重置状态（用于窗口重用时） */
    reset: () => void;
}

/**
 * 统一的 Artifact 事件处理 hook
 * 
 * 核心改进：
 * 1. 确保监听器先注册，再发送 ready 信号
 * 2. 持续发送 ready 信号直到收到数据
 * 3. 收到数据后发送确认信号
 */
export function useArtifactEvents(options: UseArtifactEventsOptions): UseArtifactEventsReturn {
    const {
        windowType,
        readyWindow,
        onArtifactData,
        onRedirect,
        onEnvironmentCheck,
        onEnvironmentInstallStarted,
        onBunInstallFinished,
        onUvInstallFinished,
        onReset,
    } = options;

    // 事件名称前缀
    const eventPrefix = windowType === 'preview' ? 'artifact-preview' : 'artifact';
    const readyEvent =
        windowType === 'preview' && readyWindow
            ? `${eventPrefix}-ready-${readyWindow}`
            : `${eventPrefix}-ready`;
    const dataEvent = `${eventPrefix}-data`;
    const logEvent = `${eventPrefix}-log`;
    const errorEvent = `${eventPrefix}-error`;
    const successEvent = `${eventPrefix}-success`;
    const redirectEvent = `${eventPrefix}-redirect`;

    const [logs, setLogs] = useState<LogLine[]>([]);
    const [hasReceivedData, setHasReceivedData] = useState(false);

    const unlistenersRef = useRef<(() => void)[]>([]);
    const isRegisteredRef = useRef(false);
    const readyIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
    const hasReceivedDataRef = useRef(false);
    const currentRequestIdRef = useRef<string | null>(null);

    const getRequestId = useCallback((payload: unknown): string | undefined => {
        if (!payload || typeof payload !== 'object') {
            return undefined;
        }
        const requestId = (payload as { request_id?: unknown }).request_id;
        return typeof requestId === 'string' ? requestId : undefined;
    }, []);

    const shouldIgnoreEvent = useCallback((requestId?: string) => {
        const currentRequestId = currentRequestIdRef.current;
        if (!currentRequestId) {
            return false;
        }
        if (!requestId) {
            return true;
        }
        return requestId !== currentRequestId;
    }, []);

    const ensureRequestId = useCallback((requestId?: string) => {
        if (!requestId || currentRequestIdRef.current) {
            return;
        }
        currentRequestIdRef.current = requestId;
    }, []);

    const parseMessagePayload = useCallback((payload: unknown): { message: string; requestId?: string } => {
        if (typeof payload === 'string') {
            return { message: payload };
        }
        if (payload && typeof payload === 'object') {
            const messageField = (payload as { message?: unknown }).message;
            const message = typeof messageField === 'string' ? messageField : JSON.stringify(payload);
            return { message, requestId: getRequestId(payload) };
        }
        return { message: String(payload) };
    }, [getRequestId]);

    // 同步 hasReceivedData 到 ref
    useEffect(() => {
        hasReceivedDataRef.current = hasReceivedData;
    }, [hasReceivedData]);

    const addLog = useCallback((type: LogLine['type'], message: string) => {
        setLogs(prev => [...prev, { type, message }]);
    }, []);

    const clearLogs = useCallback(() => {
        setLogs([]);
    }, []);

    const reset = useCallback(() => {
        setLogs([]);
        setHasReceivedData(false);
        hasReceivedDataRef.current = false;
    }, []);

    // 停止发送 ready 信号
    const stopReadySignal = useCallback(() => {
        if (readyIntervalRef.current) {
            clearInterval(readyIntervalRef.current);
            readyIntervalRef.current = null;
        }
    }, [windowType]);

    // 发送数据接收确认
    const sendDataReceivedConfirmation = useCallback(() => {
        const confirmEvent = `${eventPrefix}-data-received`;
        emit(confirmEvent, { timestamp: Date.now(), windowType });
    }, [eventPrefix, windowType]);

    useEffect(() => {
        let isCancelled = false;

        const registerListeners = async () => {
            // 防止重复注册
            if (isRegisteredRef.current || isCancelled) {
                return;
            }
            isRegisteredRef.current = true;


            // 创建日志处理函数
            const createLogHandler = (type: LogLine['type']) => (event: { payload: any }) => {
                const { message, requestId } = parseMessagePayload(event.payload);
                if (shouldIgnoreEvent(requestId)) {
                    return;
                }
                ensureRequestId(requestId);
                setLogs(prev => [...prev, { type, message }]);
            };

            // 处理 artifact 数据
            const handleArtifactData = (event: { payload: any }) => {
                const payload = event.payload as ArtifactData | null;
                if (!payload || typeof payload !== 'object') {
                    console.warn(`🔧 [${windowType}] 收到无效 artifact 数据:`, event.payload);
                    return;
                }
                const requestId = getRequestId(payload);
                if (shouldIgnoreEvent(requestId)) {
                    return;
                }
                ensureRequestId(requestId);
                const data = payload as ArtifactData;

                // 标记已接收数据
                setHasReceivedData(true);
                hasReceivedDataRef.current = true;

                // 停止发送 ready 信号
                stopReadySignal();

                // 发送确认
                sendDataReceivedConfirmation();

                // 调用回调
                onArtifactData?.(data);
            };

            // 处理重定向
            const handleRedirect = (event: { payload: any }) => {
                const payload = event.payload as { url?: string; request_id?: string } | string;
                const url = typeof payload === 'string' ? payload : payload?.url;
                const requestId = typeof payload === 'string' ? undefined : payload?.request_id;

                if (!url) {
                    console.warn(`🔧 [${windowType}] 收到无效重定向 payload:`, payload);
                    return;
                }

                if (shouldIgnoreEvent(requestId)) {
                    return;
                }
                ensureRequestId(requestId);


                // 标记已接收数据
                setHasReceivedData(true);
                hasReceivedDataRef.current = true;

                // 停止发送 ready 信号
                stopReadySignal();

                // 发送确认
                sendDataReceivedConfirmation();

                onRedirect?.(url);
            };

            // 处理环境检查
            const handleEnvironmentCheck = (event: { payload: any }) => {
                const requestId = getRequestId(event.payload);
                if (shouldIgnoreEvent(requestId)) {
                    return;
                }
                ensureRequestId(requestId);
                const data = event.payload as EnvironmentCheckData;
                onEnvironmentCheck?.(data);
            };

            // 处理环境安装开始
            const handleEnvironmentInstallStarted = (event: { payload: any }) => {
                const data = event.payload;
                onEnvironmentInstallStarted?.(data);
            };

            // 处理 Bun 安装完成
            const handleBunInstallFinished = (event: { payload: any }) => {
                const success = event.payload as boolean;
                onBunInstallFinished?.(success);
            };

            // 处理 uv 安装完成
            const handleUvInstallFinished = (event: { payload: any }) => {
                const success = event.payload as boolean;
                onUvInstallFinished?.(success);
            };

            // 处理重置事件（切换 artifact 时）
            const handleReset = (event: { payload: any }) => {
                const nextRequestId = typeof event.payload?.request_id === 'string'
                    ? event.payload.request_id
                    : null;
                currentRequestIdRef.current = nextRequestId;
                reset();  // 清除内部状态
                onReset?.();  // 调用外部回调

                // Reset 后需要重新发送 ready 信号，因为后端在等待
                // 立即发送一次，然后重新启动 interval
                emit(readyEvent, {
                    windowType,
                    timestamp: Date.now(),
                    listenersRegistered: true
                });

                // 重新启动 interval（如果已停止）
                if (!readyIntervalRef.current) {
                    readyIntervalRef.current = setInterval(() => {
                        if (!hasReceivedDataRef.current) {
                            emit(readyEvent, {
                                windowType,
                                timestamp: Date.now(),
                                listenersRegistered: true
                            });
                        } else {
                            stopReadySignal();
                        }
                    }, 200);
                }
            };

            try {
                // 1. 先注册所有监听器
                const unlisteners = await Promise.all([
                    listenHere(dataEvent, handleArtifactData),
                    listenHere(logEvent, createLogHandler('log')),
                    listenHere(errorEvent, createLogHandler('error')),
                    listenHere(successEvent, createLogHandler('success')),
                    listenHere(redirectEvent, handleRedirect),
                    listenHere('environment-check', handleEnvironmentCheck),
                    listenHere('environment-install-started', handleEnvironmentInstallStarted),
                    listenHere('bun-install-finished', handleBunInstallFinished),
                    listenHere('uv-install-finished', handleUvInstallFinished),
                    listenHere('artifact-preview-reset', handleReset),
                ]);


                // 检查是否已被取消
                if (isCancelled) {
                    unlisteners.forEach(fn => fn());
                    return;
                }

                unlistenersRef.current = unlisteners;

                // 2. 监听器注册完成后，开始发送 ready 信号
                const sendReadySignal = () => {
                    // 如果已经收到数据，不再发送
                    if (hasReceivedDataRef.current) {
                        stopReadySignal();
                        return;
                    }
                    
                    emit(readyEvent, { 
                        windowType, 
                        timestamp: Date.now(),
                        listenersRegistered: true 
                    });
                };

                // 立即发送第一次 ready 信号
                sendReadySignal();

                // 持续发送 ready 信号，间隔 200ms，直到收到数据
                readyIntervalRef.current = setInterval(sendReadySignal, 200);

                // 设置最大发送时间 30 秒，避免无限发送
                setTimeout(() => {
                    if (readyIntervalRef.current && !hasReceivedDataRef.current) {
                        stopReadySignal();
                    }
                }, 30000);

            } catch (error) {
                console.error(`🔧 [${windowType}] 注册监听器失败:`, error);
                isRegisteredRef.current = false;
            }
        };

        registerListeners();

        return () => {
            isCancelled = true;
            stopReadySignal();
            unlistenersRef.current.forEach(fn => fn());
            unlistenersRef.current = [];
            isRegisteredRef.current = false;
        };
    }, [
        windowType,
        eventPrefix,
        readyEvent,
        dataEvent,
        logEvent,
        errorEvent,
        successEvent,
        redirectEvent,
        onArtifactData,
        onRedirect,
        onEnvironmentCheck,
        onEnvironmentInstallStarted,
        onBunInstallFinished,
        onUvInstallFinished,
        onReset,
        stopReadySignal,
        sendDataReceivedConfirmation,
        ensureRequestId,
        getRequestId,
        parseMessagePayload,
        shouldIgnoreEvent,
    ]);

    return {
        logs,
        addLog,
        clearLogs,
        hasReceivedData,
        reset,
    };
}
