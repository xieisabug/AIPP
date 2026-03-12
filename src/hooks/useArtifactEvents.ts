import { useEffect, useRef, useCallback, useState } from 'react';
import { listen, emit } from '@tauri-apps/api/event';

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
    const readyIntervalRef = useRef<NodeJS.Timeout | null>(null);
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
            console.log(`🔧 [${windowType}] 停止发送 ready 信号`);
        }
    }, [windowType]);

    // 发送数据接收确认
    const sendDataReceivedConfirmation = useCallback(() => {
        const confirmEvent = `${eventPrefix}-data-received`;
        emit(confirmEvent, { timestamp: Date.now(), windowType });
        console.log(`🔧 [${windowType}] 发送数据接收确认: ${confirmEvent}`);
    }, [eventPrefix, windowType]);

    useEffect(() => {
        let isCancelled = false;

        const registerListeners = async () => {
            // 防止重复注册
            if (isRegisteredRef.current || isCancelled) {
                console.log(`🔧 [${windowType}] 监听器已注册或已取消，跳过`);
                return;
            }
            isRegisteredRef.current = true;

            console.log(`🔧 [${windowType}] 开始注册事件监听器...`);

            // 创建日志处理函数
            const createLogHandler = (type: LogLine['type']) => (event: { payload: any }) => {
                const { message, requestId } = parseMessagePayload(event.payload);
                if (shouldIgnoreEvent(requestId)) {
                    console.log(`🔧 [${windowType}] 忽略过期日志: ${requestId ?? 'no-request'}`);
                    return;
                }
                ensureRequestId(requestId);
                console.log(`🔧 [${windowType}] 收到日志[${type}]: ${message}`);
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
                    console.log(`🔧 [${windowType}] 忽略过期 artifact 数据: ${requestId ?? 'no-request'}`);
                    return;
                }
                ensureRequestId(requestId);
                const data = payload as ArtifactData;
                console.log(`🔧 [${windowType}] 收到 artifact 数据:`, data);

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
                    console.log(`🔧 [${windowType}] 忽略过期重定向: ${requestId ?? 'no-request'}`);
                    return;
                }
                ensureRequestId(requestId);

                console.log(`🔧 [${windowType}] 收到重定向: ${url}`);

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
                    console.log(`🔧 [${windowType}] 忽略过期环境检查: ${requestId ?? 'no-request'}`);
                    return;
                }
                ensureRequestId(requestId);
                const data = event.payload as EnvironmentCheckData;
                console.log(`🔧 [${windowType}] 收到环境检查:`, data);
                onEnvironmentCheck?.(data);
            };

            // 处理环境安装开始
            const handleEnvironmentInstallStarted = (event: { payload: any }) => {
                const data = event.payload;
                console.log(`🔧 [${windowType}] 环境安装开始:`, data);
                onEnvironmentInstallStarted?.(data);
            };

            // 处理 Bun 安装完成
            const handleBunInstallFinished = (event: { payload: any }) => {
                const success = event.payload as boolean;
                console.log(`🔧 [${windowType}] Bun 安装完成:`, success);
                onBunInstallFinished?.(success);
            };

            // 处理 uv 安装完成
            const handleUvInstallFinished = (event: { payload: any }) => {
                const success = event.payload as boolean;
                console.log(`🔧 [${windowType}] uv 安装完成:`, success);
                onUvInstallFinished?.(success);
            };

            // 处理重置事件（切换 artifact 时）
            const handleReset = (event: { payload: any }) => {
                const nextRequestId = typeof event.payload?.request_id === 'string'
                    ? event.payload.request_id
                    : null;
                currentRequestIdRef.current = nextRequestId;
                console.log(`🔧 [${windowType}] 收到 reset 事件，重置状态`);
                reset();  // 清除内部状态
                onReset?.();  // 调用外部回调

                // Reset 后需要重新发送 ready 信号，因为后端在等待
                // 立即发送一次，然后重新启动 interval
                console.log(`🔧 [${windowType}] reset 后重新发送 ready 信号`);
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
                            console.log(`🔧 [${windowType}] 发送 ready 信号: ${readyEvent}`);
                        } else {
                            stopReadySignal();
                        }
                    }, 200);
                }
            };

            try {
                // 1. 先注册所有监听器
                const unlisteners = await Promise.all([
                    listen(dataEvent, handleArtifactData),
                    listen(logEvent, createLogHandler('log')),
                    listen(errorEvent, createLogHandler('error')),
                    listen(successEvent, createLogHandler('success')),
                    listen(redirectEvent, handleRedirect),
                    listen('environment-check', handleEnvironmentCheck),
                    listen('environment-install-started', handleEnvironmentInstallStarted),
                    listen('bun-install-finished', handleBunInstallFinished),
                    listen('uv-install-finished', handleUvInstallFinished),
                    listen('artifact-preview-reset', handleReset),
                ]);

                console.log(`🔧 [${windowType}] 所有监听器注册成功`);

                // 检查是否已被取消
                if (isCancelled) {
                    console.log(`🔧 [${windowType}] 初始化已取消，清理监听器`);
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
                    console.log(`🔧 [${windowType}] 发送 ready 信号: ${readyEvent}`);
                };

                // 立即发送第一次 ready 信号
                sendReadySignal();

                // 持续发送 ready 信号，间隔 200ms，直到收到数据
                readyIntervalRef.current = setInterval(sendReadySignal, 200);

                // 设置最大发送时间 30 秒，避免无限发送
                setTimeout(() => {
                    if (readyIntervalRef.current && !hasReceivedDataRef.current) {
                        console.log(`🔧 [${windowType}] ready 信号发送超时 (30s)，停止发送`);
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
