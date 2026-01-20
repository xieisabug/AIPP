import { useCallback, useEffect, useRef, useState, startTransition } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
    StreamEvent,
    ConversationEvent,
    MessageUpdateEvent,
    MessageTypeEndEvent,
    GroupMergeEvent,
    MCPToolCallUpdateEvent,
    ConversationCancelEvent,
    StreamCompleteEvent,
    ActivityFocusChangeEvent,
    ActivityFocus,
} from "../data/Conversation";
import { MCPToolCall } from "@/data/MCPToolCall";

export interface UseConversationEventsOptions {
    conversationId: string | number;
    onMessageAdd?: (messageData: any) => void;
    onMessageUpdate?: (streamEvent: StreamEvent) => void;
    onGroupMerge?: (groupMergeData: GroupMergeEvent) => void;
    onMCPToolCallUpdate?: (mcpUpdateData: MCPToolCallUpdateEvent) => void;
    onConversationCancel?: (cancelData: ConversationCancelEvent) => void;
    onAiResponseStart?: () => void;
    onAiResponseComplete?: () => void;
    onError?: (errorMessage: string) => void;
}

export function useConversationEvents(options: UseConversationEventsOptions) {
    // 流式消息状态管理，存储正在流式传输的消息
    const [streamingMessages, setStreamingMessages] = useState<
        Map<number, StreamEvent>
    >(new Map());

    // ShineBorder 动画状态管理
    const [shiningMessageIds, setShiningMessageIds] = useState<Set<number>>(
        new Set(),
    );

    // MCP工具调用状态管理
    const [mcpToolCallStates, setMCPToolCallStates] = useState<
        Map<number, MCPToolCallUpdateEvent>
    >(new Map());

    // 活跃的 MCP 工具调用 ID 集合（正在执行的）
    const [activeMcpCallIds, setActiveMcpCallIds] = useState<Set<number>>(
        new Set(),
    );

    // 正在输出的 assistant 消息 ID 集合
    const [streamingAssistantMessageIds, setStreamingAssistantMessageIds] = useState<Set<number>>(
        new Set(),
    );

    // 等待回复的用户消息 ID（只有一个）- 保留 setter 用于事件处理中清理状态
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const [_pendingUserMessageId, setPendingUserMessageId] = useState<number | null>(null);

    // 活动焦点状态 - 由后端统一管理，优先使用这个状态来控制闪亮边框
    const [activityFocus, setActivityFocus] = useState<ActivityFocus>({ focus_type: 'none' });

    // 事件监听取消订阅引用
    const unsubscribeRef = useRef<Promise<() => void> | null>(null);
    const hasUnsubscribedRef = useRef<boolean>(false);
    const focusSyncRequestIdRef = useRef<number>(0);
    const hasSyncedAfterMessageAddRef = useRef<boolean>(false);

    // 使用 ref 存储最新的回调函数，避免依赖项变化
    const callbacksRef = useRef(options);

    // 使用 ref 存储最新的 functionMap，避免频繁变化
    const functionMapRef = useRef<Map<number, any>>(new Map());

    // 更新 ref 中的回调函数
    useEffect(() => {
        callbacksRef.current = options;
    }, [options]);

    // 基于 activityFocus 计算闪亮边框状态
    // 这是新的逻辑：优先使用后端发送的活动焦点状态
    const updateShiningMessagesFromFocus = useCallback(() => {
        setShiningMessageIds(() => {
            const newShining = new Set<number>();

            switch (activityFocus.focus_type) {
                case 'none':
                    // 没有活动焦点，清空所有边框
                    return newShining;
                case 'user_pending':
                case 'assistant_streaming':
                    // 显示消息边框
                    newShining.add(activityFocus.message_id);
                    console.log("✨ [ActivityFocus] Shining message:", activityFocus.message_id, "-", activityFocus.focus_type);
                    return newShining;
                case 'mcp_executing':
                    // MCP 执行时不显示消息边框（MCP 组件自己控制）
                    console.log("🔧 [ActivityFocus] MCP executing:", activityFocus.call_id);
                    return newShining;
                default:
                    return newShining;
            }
        });

        // 同步更新活跃的 MCP 调用 ID
        setActiveMcpCallIds(() => {
            const newActiveSet = new Set<number>();
            if (activityFocus.focus_type === 'mcp_executing') {
                newActiveSet.add(activityFocus.call_id);
            }
            return newActiveSet;
        });
    }, [activityFocus]);

    // 当 activityFocus 变化时，更新边框显示
    useEffect(() => {
        updateShiningMessagesFromFocus();
    }, [updateShiningMessagesFromFocus]);

    // 智能边框控制辅助函数 - 用于外部组件手动触发边框更新
    // 注意：现在 shiningMessageIds 主要由 updateShiningMessagesFromFocus 根据 activityFocus 管理
    // 这个函数保留用于兼容性，但不再自动触发
    const updateShiningMessages = useCallback(() => {
        // 不再自动更新 shiningMessageIds，而是触发 updateShiningMessagesFromFocus
        updateShiningMessagesFromFocus();
    }, [updateShiningMessagesFromFocus]);

    // 主动从后端同步当前活动焦点，避免在监听尚未建立时丢失状态
    const syncActivityFocus = useCallback((conversationIdNum: number) => {
        if (!conversationIdNum || Number.isNaN(conversationIdNum)) {
            return;
        }

        const requestId = focusSyncRequestIdRef.current + 1;
        focusSyncRequestIdRef.current = requestId;

        invoke<ActivityFocus>("get_activity_focus", { conversationId: conversationIdNum })
            .then((focus) => {
                if (focusSyncRequestIdRef.current !== requestId) return;
                console.log("[ActivityFocus] Synced initial focus from backend:", focus);
                setActivityFocus(focus);
            })
            .catch((error) => {
                if (focusSyncRequestIdRef.current !== requestId) return;
                console.warn("[ActivityFocus] Failed to sync focus state", error);
            });
    }, []);

    const applyMcpToolCalls = useCallback((calls: MCPToolCall[]) => {
        const stateMap = new Map<number, MCPToolCallUpdateEvent>();
        const activeSet = new Set<number>();

        calls.forEach((call) => {
            const update: MCPToolCallUpdateEvent = {
                call_id: call.id,
                conversation_id: call.conversation_id,
                status: call.status,
                server_name: call.server_name,
                tool_name: call.tool_name,
                parameters: call.parameters,
                result: call.result,
                error: call.error,
                started_time: call.started_time ? new Date(call.started_time) : undefined,
                finished_time: call.finished_time ? new Date(call.finished_time) : undefined,
            };
            stateMap.set(call.id, update);
            if (call.status === "executing" || call.status === "pending") {
                activeSet.add(call.id);
            }
        });

        console.log(
            `[MCP] applyMcpToolCalls -> total=${calls.length}, active=${activeSet.size}`,
            { ids: calls.map((c) => c.id) },
        );
        setMCPToolCallStates(stateMap);
        setActiveMcpCallIds(activeSet);
    }, []);

    const refreshMcpToolCalls = useCallback(
        (cancelRef?: { cancelled: boolean }) => {
            if (!options.conversationId) {
                return;
            }

            const conversationIdNum = Number(options.conversationId);
            if (Number.isNaN(conversationIdNum)) {
                return;
            }

            invoke<MCPToolCall[]>("get_mcp_tool_calls_by_conversation", {
                conversationId: conversationIdNum,
            })
                .then((calls) => {
                    if (cancelRef?.cancelled) return;
                    console.log(
                        `[MCP] refreshMcpToolCalls success for conversation ${conversationIdNum}`,
                        { callIds: calls.map((c) => c.id), statuses: calls.map((c) => c.status) },
                    );
                    applyMcpToolCalls(calls);
                })
                .catch((error) => {
                    if (cancelRef?.cancelled) return;
                    console.warn("Failed to preload MCP tool calls", error);
                });
        },
        [options.conversationId, applyMcpToolCalls],
    );

    // 统一的事件处理函数
    const handleConversationEvent = useCallback(
        (event: any) => {
            const conversationEvent = event.payload as ConversationEvent;

            // ACP DEBUG: 记录所有接收到的事件
            console.log("[ACP DEBUG] Received event:", conversationEvent.type, conversationEvent.data);

            if (conversationEvent.type === "message_add") {
                // 处理消息添加事件
                const messageAddData = conversationEvent.data as any;
                console.log("Received message_add event:", messageAddData);

                // 如果是用户消息，设置为等待回复的消息，而不是直接设置边框
                if (messageAddData.message_type === "user") {
                    setPendingUserMessageId(messageAddData.message_id);
                }

                if (!hasSyncedAfterMessageAddRef.current) {
                    const conversationIdNum = Number(callbacksRef.current.conversationId);
                    if (!Number.isNaN(conversationIdNum)) {
                        syncActivityFocus(conversationIdNum);
                        hasSyncedAfterMessageAddRef.current = true;
                    }
                }

                // 调用外部的消息添加处理函数
                callbacksRef.current.onMessageAdd?.(messageAddData);
            } else if (conversationEvent.type === "message_update") {
                const messageUpdateData =
                    conversationEvent.data as MessageUpdateEvent;

                const streamEvent: StreamEvent = {
                    message_id: messageUpdateData.message_id,
                    message_type: messageUpdateData.message_type as any,
                    content: messageUpdateData.content,
                    is_done: messageUpdateData.is_done,
                    // 如果事件中包含 Token 计数，则添加到 StreamEvent 中
                    token_count: messageUpdateData.token_count,
                    input_token_count: messageUpdateData.input_token_count,
                    output_token_count: messageUpdateData.output_token_count,
                    // 性能指标
                    ttft_ms: messageUpdateData.ttft_ms,
                    tps: messageUpdateData.tps,
                };

                // 检查是否是错误消息
                if (messageUpdateData.message_type === "error") {
                    // 对于错误消息，立即触发错误处理和状态清理
                    console.error("Received error message:", messageUpdateData.content);

                    // 清理所有边框相关状态
                    setPendingUserMessageId(null);
                    setStreamingAssistantMessageIds(new Set());

                    // 调用错误处理回调
                    callbacksRef.current.onError?.(messageUpdateData.content);
                    callbacksRef.current.onAiResponseComplete?.(); // 错误也算作响应完成

                    // 对于错误消息，处理完成状态并延长显示时间
                    if (messageUpdateData.is_done) {
                        setStreamingMessages((prev) => {
                            const newMap = new Map(prev);
                            const completedEvent = {
                                ...streamEvent,
                                is_done: true,
                            };
                            newMap.set(streamEvent.message_id, completedEvent);
                            return newMap;
                        });

                        // 错误消息保留更长时间，让用户能看到完整的错误信息
                        setTimeout(() => {
                            setStreamingMessages((prev) => {
                                const newMap = new Map(prev);
                                newMap.delete(streamEvent.message_id);
                                return newMap;
                            });
                        }, 8000); // 8秒后清理错误消息，给用户更多时间阅读
                    }
                } else {
                    // 正常消息处理逻辑

                    // 处理 assistant 消息的流式输出边框
                    if (messageUpdateData.message_type === "response" || messageUpdateData.message_type === "assistant") {
                        if (messageUpdateData.is_done) {
                            // Assistant 消息完成，从流式消息集合中移除
                            console.log("✅ [DEBUG] Assistant message COMPLETED:", messageUpdateData.message_id);
                            setStreamingAssistantMessageIds((prev) => {
                                const newSet = new Set(prev);
                                newSet.delete(messageUpdateData.message_id);
                                return newSet;
                            });
                        } else if (messageUpdateData.content) {
                            // Assistant 消息开始输出，清除等待回复的用户消息，添加到流式消息集合
                            console.log("🚀 [DEBUG] Assistant message STARTING:", messageUpdateData.message_id);
                            setPendingUserMessageId(null); // 清除等待回复的用户消息
                            setStreamingAssistantMessageIds((prev) => {
                                const newSet = new Set(prev);
                                newSet.add(messageUpdateData.message_id);
                                return newSet;
                            });
                        }
                    }

                    // 当开始收到新的AI响应时（不是is_done时），清除用户消息的shine-border
                    if (
                        !messageUpdateData.is_done &&
                        messageUpdateData.content
                    ) {
                        if (messageUpdateData.message_type !== "user") {
                            // 不直接清空，而是移除用户消息的边框，通过 updateShiningMessages 来智能控制
                            callbacksRef.current.onAiResponseStart?.();
                        }
                    }

                    if (messageUpdateData.is_done) {
                        if (messageUpdateData.message_type === "response") {
                            callbacksRef.current.onAiResponseComplete?.();
                        }

                        // 标记流式消息为完成状态，但不立即删除，让消息能正常显示
                        setStreamingMessages((prev) => {
                            const newMap = new Map(prev);
                            const completedEvent = {
                                ...streamEvent,
                                is_done: true,
                            };
                            newMap.set(streamEvent.message_id, completedEvent);
                            return newMap;
                        });

                        // 延迟清理已完成的流式消息，给足够时间让消息保存到 messages 中
                        setTimeout(() => {
                            setStreamingMessages((prev) => {
                                const newMap = new Map(prev);
                                newMap.delete(streamEvent.message_id);
                                return newMap;
                            });
                        }, 1000); // 1秒后清理
                    } else {
                        // 使用 startTransition 将流式消息更新标记为低优先级，保持界面响应性
                        startTransition(() => {
                            setStreamingMessages((prev) => {
                                const newMap = new Map(prev);
                                newMap.set(streamEvent.message_id, streamEvent);
                                return newMap;
                            });
                        });
                    }
                }

                // 处理插件兼容性
                const functionMap = functionMapRef.current;
                const streamMessageListener = functionMap.get(
                    streamEvent.message_id,
                )?.onStreamMessageListener;
                if (streamMessageListener) {
                    streamMessageListener(
                        streamEvent.content,
                        { conversation_id: +callbacksRef.current.conversationId, request_prompt_result_with_context: "" },
                        () => { }, // 空的 setAiIsResponsing 函数，实际应该从外部传入
                    );
                }

                // 调用外部的消息更新处理函数
                callbacksRef.current.onMessageUpdate?.(streamEvent);
            } else if (conversationEvent.type === "group_merge") {
                // 处理组合并事件
                const groupMergeData =
                    conversationEvent.data as GroupMergeEvent;
                console.log("Received group merge event:", groupMergeData);

                // 调用外部的组合并处理函数
                callbacksRef.current.onGroupMerge?.(groupMergeData);
            } else if (conversationEvent.type === "message_type_end") {
                const typeEndData = conversationEvent.data as MessageTypeEndEvent;
                if (
                    typeEndData.message_type === "response" ||
                    typeEndData.message_type === "reasoning"
                ) {
                    refreshMcpToolCalls();
                }
            } else if (conversationEvent.type === "mcp_tool_call_update") {
                // 处理MCP工具调用状态更新事件
                const mcpUpdateData = conversationEvent.data as MCPToolCallUpdateEvent;
                console.log("Received mcp_tool_call_update event:", mcpUpdateData);
                console.log(
                    `[MCP] current map size=${mcpToolCallStates.size}, active=${activeMcpCallIds.size}`,
                );

                // 更新MCP工具调用状态
                setMCPToolCallStates((prev) => {
                    const newMap = new Map(prev);
                    const existing = newMap.get(mcpUpdateData.call_id);
                    const merged: MCPToolCallUpdateEvent = {
                        ...(existing || mcpUpdateData),
                        ...mcpUpdateData,
                        server_name: mcpUpdateData.server_name ?? existing?.server_name,
                        tool_name: mcpUpdateData.tool_name ?? existing?.tool_name,
                        parameters: mcpUpdateData.parameters ?? existing?.parameters,
                        result: mcpUpdateData.result ?? existing?.result,
                        error: mcpUpdateData.error ?? existing?.error,
                        started_time: mcpUpdateData.started_time ?? existing?.started_time,
                        finished_time: mcpUpdateData.finished_time ?? existing?.finished_time,
                    };
                    newMap.set(mcpUpdateData.call_id, merged);
                    return newMap;
                });

                // 更新活跃的 MCP 调用状态
                setActiveMcpCallIds((prev) => {
                    const newSet = new Set(prev);
                    const mergedStatus = mcpUpdateData.status;

                    if (mergedStatus === "executing" || mergedStatus === "pending") {
                        // MCP 开始执行，添加到活跃集合
                        newSet.add(mcpUpdateData.call_id);
                    } else if (mergedStatus === "success" || mergedStatus === "failed") {
                        // MCP 执行完成，从活跃集合中移除
                        newSet.delete(mcpUpdateData.call_id);
                    } else {
                        // 兜底：任何其他终态都认为不再活跃
                        console.log(`[MCP] Treating status '${mergedStatus}' as inactive for call ${mcpUpdateData.call_id}`);
                        newSet.delete(mcpUpdateData.call_id);
                    }

                    return newSet;
                });

                // 调用外部的MCP状态更新处理函数
                callbacksRef.current.onMCPToolCallUpdate?.(mcpUpdateData);
            } else if (conversationEvent.type === "conversation_cancel") {
                // 处理对话取消事件
                const cancelData = conversationEvent.data as ConversationCancelEvent;
                console.log("Received conversation_cancel event:", cancelData);

                // 立即清理所有流式状态，停止显示闪亮边框
                setPendingUserMessageId(null);
                setStreamingAssistantMessageIds(new Set());
                setActiveMcpCallIds(new Set());
                setMCPToolCallStates(new Map());

                // 调用 AI 响应完成回调，确保状态重置
                callbacksRef.current.onAiResponseComplete?.();

                // 调用外部的取消处理函数
                callbacksRef.current.onConversationCancel?.(cancelData);
            } else if (conversationEvent.type === "stream_complete") {
                // 处理流式完成事件（包括空响应场景）
                const completionData = conversationEvent.data as StreamCompleteEvent;
                console.log("Received stream_complete event:", completionData);

                // 清理流式与闪烁状态，避免 UI 长时间处于接收中
                setStreamingMessages(new Map());
                setShiningMessageIds(new Set());
                setStreamingAssistantMessageIds(new Set());
                setActiveMcpCallIds(new Set());
                setPendingUserMessageId(null);
                setMCPToolCallStates(new Map());
                setActivityFocus({ focus_type: 'none' });

                // 通知外部响应已完成（即便没有 response chunk）
                callbacksRef.current.onAiResponseComplete?.();
            } else if (conversationEvent.type === "activity_focus_change") {
                // 处理活动焦点变化事件 - 由后端统一管理闪亮边框状态
                const focusEvent = conversationEvent.data as ActivityFocusChangeEvent;
                console.log("[ActivityFocus] Received focus change:", focusEvent.focus);
                setActivityFocus(focusEvent.focus);
            }
        },
        [refreshMcpToolCalls],
    );

    // 设置和清理事件监听
    useEffect(() => {
        if (!options.conversationId) {
            // 清理状态
            focusSyncRequestIdRef.current += 1; // 使之前的同步请求失效
            setStreamingMessages(new Map());
            setShiningMessageIds(new Set());
            setMCPToolCallStates(new Map());
            setActiveMcpCallIds(new Set());
            setStreamingAssistantMessageIds(new Set());
            setPendingUserMessageId(null);
            setActivityFocus({ focus_type: 'none' });
            hasSyncedAfterMessageAddRef.current = false;
            return;
        }

        const conversationIdNum = Number(options.conversationId);
        if (Number.isNaN(conversationIdNum)) {
            focusSyncRequestIdRef.current += 1; // 避免旧同步影响
            console.warn("[ActivityFocus] Invalid conversationId for event subscription:", options.conversationId);
            return;
        }

        hasSyncedAfterMessageAddRef.current = false;

        const eventName = `conversation_event_${conversationIdNum}`;
        console.log(
            `[ACP DEBUG] Setting up conversation event listener for: ${eventName}`,
        );

        // 取消之前的事件监听（只执行一次）
        if (unsubscribeRef.current && !hasUnsubscribedRef.current) {
            console.log("Unsubscribing from previous event listener");
            const p = unsubscribeRef.current;
            unsubscribeRef.current = null;
            hasUnsubscribedRef.current = true;
            p.then((f) => {
                try { f(); } catch (e) { console.warn('unlisten failed (previous):', e); }
            }).catch((e) => console.warn('unlisten rejected (previous):', e));
        }

        // 设置新的事件监听
        hasUnsubscribedRef.current = false;
        console.log(`[ACP DEBUG] Listening to event: ${eventName}`);
        unsubscribeRef.current = listen(
            eventName,
            handleConversationEvent,
        );

        // 主动同步一次当前焦点，避免在订阅前发生的事件导致闪烁状态缺失
        syncActivityFocus(conversationIdNum);

        return () => {
            if (unsubscribeRef.current && !hasUnsubscribedRef.current) {
                console.log("[ACP DEBUG] Unsubscribing from events");
                const p = unsubscribeRef.current;
                unsubscribeRef.current = null;
                hasUnsubscribedRef.current = true;
                p.then((f) => {
                    try { f(); } catch (e) { console.warn('unlisten failed (cleanup):', e); }
                }).catch((e) => console.warn('unlisten rejected (cleanup):', e));
            }
        };
    }, [options.conversationId]); // 只依赖 conversationId

    // 初始化获取已存在的 MCP 调用状态
    useEffect(() => {
        if (!options.conversationId) {
            return;
        }

        const cancelRef = { cancelled: false };
        refreshMcpToolCalls(cancelRef);

        return () => {
            cancelRef.cancelled = true;
        };
    }, [options.conversationId, refreshMcpToolCalls]);

    // 清理函数
    const clearStreamingMessages = useCallback(() => {
        setStreamingMessages(new Map());
    }, []);

    const clearShiningMessages = useCallback(() => {
        console.log("[DEBUG] Clearing shining/MCP state (manual reset)");
        setShiningMessageIds(new Set());
        setStreamingAssistantMessageIds(new Set());
        setPendingUserMessageId(null);
        setActiveMcpCallIds(new Set());
        setMCPToolCallStates(new Map());
        setActivityFocus({ focus_type: 'none' });
    }, []);

    const setPendingUserMessage = useCallback((messageId: number | null) => {
        setPendingUserMessageId(messageId);
    }, []);

    const handleError = useCallback((errorMessage: string) => {
        console.error("Global error handler called:", errorMessage);

        // 清理所有流式消息状态
        setStreamingMessages(new Map());
        setShiningMessageIds(new Set());
        setMCPToolCallStates(new Map());
        setActiveMcpCallIds(new Set());
        setStreamingAssistantMessageIds(new Set());
        setPendingUserMessageId(null); // 清理等待回复的用户消息
        setActivityFocus({ focus_type: 'none' });

        // 调用外部错误处理，确保状态重置
        callbacksRef.current.onError?.(errorMessage);
        callbacksRef.current.onAiResponseComplete?.();
    }, []);

    // 提供稳定的 functionMap 更新接口
    const updateFunctionMap = useCallback((functionMap: Map<number, any>) => {
        functionMapRef.current = functionMap;
    }, []);

    return {
        streamingMessages,
        shiningMessageIds,
        setShiningMessageIds,
        mcpToolCallStates,
        activeMcpCallIds, // 导出活跃的 MCP 调用状态
        streamingAssistantMessageIds, // 导出正在流式输出的 assistant 消息状态
        activityFocus, // 导出活动焦点状态（后端驱动）
        clearStreamingMessages,
        clearShiningMessages,
        handleError,
        updateShiningMessages, // 导出智能边框更新函数
        updateFunctionMap, // 导出 functionMap 更新函数
        setPendingUserMessage,
    };
}
