import { act, renderHook } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MCPToolCall } from "@/data/MCPToolCall";
import type { ConversationRuntimePhase, ShineTarget } from "@/data/Conversation";
import { useConversationEvents } from "@/hooks/useConversationEvents";
import {
    clearAllMockHandlers,
    mockInvokeHandler,
} from "@/__tests__/mocks/tauri";

const flushEffects = async () => {
    await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
    });
};

const advanceAndFlush = async (ms: number) => {
    await act(async () => {
        vi.advanceTimersByTime(ms);
        await Promise.resolve();
        await Promise.resolve();
    });
};

function createActiveCall(conversationId: number, callId: number): MCPToolCall {
    return {
        id: callId,
        conversation_id: conversationId,
        server_id: 1,
        server_name: "demo-server",
        tool_name: "demo-tool",
        parameters: "{\"query\":\"hello\"}",
        status: "executing",
        created_time: "2024-01-01T00:00:00.000Z",
    };
}

function createShineState(
    conversationId: number,
    revision: number,
    primaryTarget: ShineTarget,
    epoch = 1,
) {
    return {
        conversation_id: conversationId,
        epoch,
        revision,
        primary_target: primaryTarget,
    };
}

function createRuntimeState(
    conversationId: number,
    revision: number,
    phase: ConversationRuntimePhase,
    epoch = 1,
) {
    return {
        conversation_id: conversationId,
        is_running: phase !== "idle",
        phase,
        epoch,
        revision,
    };
}

describe("useConversationEvents MCP completion reconciliation", () => {
    beforeEach(() => {
        vi.useFakeTimers();
    });

    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
        vi.clearAllTimers();
        vi.useRealTimers();
    });

    it("clears a stale MCP shine state after delayed backend sync", async () => {
        const conversationId = 7;
        const callId = 11;
        const activeCall = createActiveCall(conversationId, callId);

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => [activeCall]);

        let shineSyncCount = 0;
        mockInvokeHandler("get_shine_state", () => {
            shineSyncCount += 1;
            if (shineSyncCount < 3) {
                return {
                    conversation_id: conversationId,
                    epoch: 1,
                    revision: 1,
                    primary_target: {
                        target_type: "mcp_call",
                        call_id: callId,
                        reason: "mcp_executing",
                    },
                };
            }
            return {
                conversation_id: conversationId,
                epoch: 1,
                revision: 2,
                primary_target: {
                    target_type: "none",
                },
            };
        });

        let runtimeSyncCount = 0;
        mockInvokeHandler("get_conversation_runtime_state", () => {
            runtimeSyncCount += 1;
            if (runtimeSyncCount < 3) {
                return {
                    conversation_id: conversationId,
                    is_running: true,
                    phase: "mcp_executing",
                    epoch: 1,
                    revision: 1,
                };
            }
            return {
                conversation_id: conversationId,
                is_running: false,
                phase: "idle",
                epoch: 1,
                revision: 2,
            };
        });

        const { result } = renderHook(() =>
            useConversationEvents({
                conversationId,
            }),
        );

        await flushEffects();
        await flushEffects();

        expect(result.current.shiningMcpCallId).toBe(callId);

        await act(async () => {
            await emit(`conversation_event_${conversationId}`, {
                type: "mcp_tool_call_update",
                data: {
                    call_id: callId,
                    conversation_id: conversationId,
                    status: "success",
                    result: "{\"ok\":true}",
                },
            });
        });

        await flushEffects();
        expect(result.current.shiningMcpCallId).toBe(callId);

        await advanceAndFlush(180);
        await flushEffects();

        expect(shineSyncCount).toBeGreaterThanOrEqual(3);
        expect(runtimeSyncCount).toBeGreaterThanOrEqual(3);
        expect(result.current.shiningMcpCallId).toBeNull();
    });

    it("allows delayed MCP reconciliation to hand off shine state to assistant streaming", async () => {
        const conversationId = 8;
        const callId = 12;
        const responseMessageId = 99;
        const activeCall = createActiveCall(conversationId, callId);

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => [activeCall]);

        let shineSyncCount = 0;
        mockInvokeHandler("get_shine_state", () => {
            shineSyncCount += 1;
            if (shineSyncCount < 3) {
                return {
                    conversation_id: conversationId,
                    epoch: 2,
                    revision: 3,
                    primary_target: {
                        target_type: "mcp_call",
                        call_id: callId,
                        reason: "mcp_executing",
                    },
                };
            }
            return {
                conversation_id: conversationId,
                epoch: 2,
                revision: 4,
                primary_target: {
                    target_type: "message",
                    message_id: responseMessageId,
                    reason: "assistant_streaming",
                },
            };
        });

        mockInvokeHandler("get_conversation_runtime_state", () => ({
            conversation_id: conversationId,
            is_running: true,
            phase: shineSyncCount < 3 ? "mcp_executing" : "assistant_streaming",
            epoch: 2,
            revision: shineSyncCount < 3 ? 3 : 4,
        }));

        const { result } = renderHook(() =>
            useConversationEvents({
                conversationId,
            }),
        );

        await flushEffects();
        await flushEffects();

        expect(result.current.shiningMcpCallId).toBe(callId);
        expect(result.current.shiningMessageIds.size).toBe(0);

        await act(async () => {
            await emit(`conversation_event_${conversationId}`, {
                type: "mcp_tool_call_update",
                data: {
                    call_id: callId,
                    conversation_id: conversationId,
                    status: "success",
                    result: "{\"ok\":true}",
                },
            });
        });

        await flushEffects();
        await advanceAndFlush(180);
        await flushEffects();

        expect(result.current.shiningMcpCallId).toBeNull();
        expect(result.current.shiningMessageIds.has(responseMessageId)).toBe(true);
        expect(result.current.activityFocus).toEqual({
            focus_type: "assistant_streaming",
            message_id: responseMessageId,
        });
    });

    it("shows assistant streaming as running before any continuation token arrives", async () => {
        const conversationId = 18;
        const callId = 68;
        const responseMessageId = 109;
        let currentCalls = [createActiveCall(conversationId, callId)];
        let currentShine = createShineState(conversationId, 1, {
            target_type: "mcp_call",
            call_id: callId,
            reason: "mcp_executing",
        });
        let currentRuntime = createRuntimeState(conversationId, 1, "mcp_executing");

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => currentCalls);
        mockInvokeHandler("get_shine_state", () => currentShine);
        mockInvokeHandler("get_conversation_runtime_state", () => currentRuntime);

        const { result } = renderHook(() =>
            useConversationEvents({
                conversationId,
            }),
        );

        await flushEffects();
        await flushEffects();

        expect(result.current.shiningMcpCallId).toBe(callId);
        expect(result.current.runtimeState?.phase).toBe("mcp_executing");

        currentCalls = [];
        currentShine = createShineState(conversationId, 2, {
            target_type: "message",
            message_id: responseMessageId,
            reason: "assistant_streaming",
        });
        currentRuntime = createRuntimeState(conversationId, 2, "assistant_streaming");

        await act(async () => {
            await emit(`conversation_event_${conversationId}`, {
                type: "mcp_tool_call_update",
                data: {
                    call_id: callId,
                    conversation_id: conversationId,
                    status: "success",
                    result: "{\"ok\":true}",
                },
            });
        });

        await flushEffects();

        expect(result.current.shiningMcpCallId).toBeNull();
        expect(result.current.shiningMessageIds.has(responseMessageId)).toBe(true);
        expect(result.current.activityFocus).toEqual({
            focus_type: "assistant_streaming",
            message_id: responseMessageId,
        });
        expect(result.current.runtimeState?.phase).toBe("assistant_streaming");
        expect(result.current.runtimeState?.is_running).toBe(true);
    });

    it("allows failed MCP continuation to hand off shine state to assistant streaming", async () => {
        const conversationId = 10;
        const callId = 13;
        const responseMessageId = 100;
        const activeCall = createActiveCall(conversationId, callId);

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => [activeCall]);

        let shineSyncCount = 0;
        mockInvokeHandler("get_shine_state", () => {
            shineSyncCount += 1;
            if (shineSyncCount < 3) {
                return {
                    conversation_id: conversationId,
                    epoch: 4,
                    revision: 6,
                    primary_target: {
                        target_type: "mcp_call",
                        call_id: callId,
                        reason: "mcp_executing",
                    },
                };
            }
            return {
                conversation_id: conversationId,
                epoch: 4,
                revision: 7,
                primary_target: {
                    target_type: "message",
                    message_id: responseMessageId,
                    reason: "assistant_streaming",
                },
            };
        });

        mockInvokeHandler("get_conversation_runtime_state", () => ({
            conversation_id: conversationId,
            is_running: true,
            phase: shineSyncCount < 3 ? "mcp_executing" : "assistant_streaming",
            epoch: 4,
            revision: shineSyncCount < 3 ? 6 : 7,
        }));

        const { result } = renderHook(() =>
            useConversationEvents({
                conversationId,
            }),
        );

        await flushEffects();
        await flushEffects();

        expect(result.current.shiningMcpCallId).toBe(callId);

        await act(async () => {
            await emit(`conversation_event_${conversationId}`, {
                type: "mcp_tool_call_update",
                data: {
                    call_id: callId,
                    conversation_id: conversationId,
                    status: "failed",
                    error: "boom",
                },
            });
        });

        await flushEffects();
        await advanceAndFlush(180);
        await flushEffects();

        expect(result.current.shiningMcpCallId).toBeNull();
        expect(result.current.shiningMessageIds.has(responseMessageId)).toBe(true);
        expect(result.current.activityFocus).toEqual({
            focus_type: "assistant_streaming",
            message_id: responseMessageId,
        });
    });

    it("keeps backend-driven shine when temporary manual highlights are cleared", async () => {
        const conversationId = 9;
        const responseMessageId = 77;
        const manualHighlightMessageId = 123;

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => []);
        mockInvokeHandler("get_shine_state", () => ({
            conversation_id: conversationId,
            epoch: 3,
            revision: 5,
            primary_target: {
                target_type: "message",
                message_id: responseMessageId,
                reason: "assistant_streaming",
            },
        }));
        mockInvokeHandler("get_conversation_runtime_state", () => ({
            conversation_id: conversationId,
            is_running: true,
            phase: "assistant_streaming",
            epoch: 3,
            revision: 5,
        }));

        const { result } = renderHook(() =>
            useConversationEvents({
                conversationId,
            }),
        );

        await flushEffects();
        await flushEffects();

        expect(result.current.shiningMessageIds.has(responseMessageId)).toBe(true);

        await act(async () => {
            result.current.setShiningMessageIds(new Set([manualHighlightMessageId]));
        });

        expect(result.current.shiningMessageIds.has(responseMessageId)).toBe(true);
        expect(result.current.shiningMessageIds.has(manualHighlightMessageId)).toBe(true);

        await act(async () => {
            result.current.setShiningMessageIds(new Set());
        });

        expect(result.current.shiningMessageIds.has(responseMessageId)).toBe(true);
        expect(result.current.shiningMessageIds.has(manualHighlightMessageId)).toBe(false);
    });

    it("tracks the normal user to assistant to idle shine lifecycle without MCP", async () => {
        const conversationId = 14;
        const userMessageId = 201;
        const assistantMessageId = 202;

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => []);
        mockInvokeHandler("get_shine_state", () =>
            createShineState(conversationId, 0, { target_type: "none" }),
        );
        mockInvokeHandler("get_conversation_runtime_state", () =>
            createRuntimeState(conversationId, 0, "idle"),
        );

        const { result } = renderHook(() =>
            useConversationEvents({
                conversationId,
            }),
        );

        await flushEffects();
        await flushEffects();

        await act(async () => {
            await emit(`conversation_event_${conversationId}`, {
                type: "shine_state_snapshot",
                data: {
                    state: createShineState(conversationId, 1, {
                        target_type: "message",
                        message_id: userMessageId,
                        reason: "user_pending",
                    }),
                },
            });
            await emit(`conversation_event_${conversationId}`, {
                type: "runtime_state_snapshot",
                data: {
                    state: createRuntimeState(conversationId, 1, "user_pending"),
                },
            });
        });

        await flushEffects();

        expect(result.current.shiningMessageIds.has(userMessageId)).toBe(true);
        expect(result.current.activityFocus).toEqual({
            focus_type: "user_pending",
            message_id: userMessageId,
        });
        expect(result.current.runtimeState?.phase).toBe("user_pending");
        expect(result.current.runtimeState?.is_running).toBe(true);

        await act(async () => {
            await emit(`conversation_event_${conversationId}`, {
                type: "shine_state_snapshot",
                data: {
                    state: createShineState(conversationId, 2, {
                        target_type: "message",
                        message_id: assistantMessageId,
                        reason: "assistant_streaming",
                    }),
                },
            });
            await emit(`conversation_event_${conversationId}`, {
                type: "runtime_state_snapshot",
                data: {
                    state: createRuntimeState(conversationId, 2, "assistant_streaming"),
                },
            });
        });

        await flushEffects();

        expect(result.current.shiningMessageIds.has(userMessageId)).toBe(false);
        expect(result.current.shiningMessageIds.has(assistantMessageId)).toBe(true);
        expect(result.current.activityFocus).toEqual({
            focus_type: "assistant_streaming",
            message_id: assistantMessageId,
        });
        expect(result.current.runtimeState?.phase).toBe("assistant_streaming");
        expect(result.current.runtimeState?.is_running).toBe(true);

        await act(async () => {
            await emit(`conversation_event_${conversationId}`, {
                type: "shine_state_snapshot",
                data: {
                    state: createShineState(conversationId, 3, {
                        target_type: "none",
                    }),
                },
            });
            await emit(`conversation_event_${conversationId}`, {
                type: "runtime_state_snapshot",
                data: {
                    state: createRuntimeState(conversationId, 3, "idle"),
                },
            });
        });

        await flushEffects();

        expect(result.current.shiningMessageIds.size).toBe(0);
        expect(result.current.shiningMcpCallId).toBeNull();
        expect(result.current.activityFocus).toEqual({ focus_type: "none" });
        expect(result.current.runtimeState?.phase).toBe("idle");
        expect(result.current.runtimeState?.is_running).toBe(false);
    });

    it("ignores stale runtime and shine snapshots by epoch and revision", async () => {
        const conversationId = 15;
        const assistantMessageId = 303;

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => []);
        mockInvokeHandler("get_shine_state", () =>
            createShineState(conversationId, 5, {
                target_type: "message",
                message_id: assistantMessageId,
                reason: "assistant_streaming",
            }, 2),
        );
        mockInvokeHandler("get_conversation_runtime_state", () =>
            createRuntimeState(conversationId, 5, "assistant_streaming", 2),
        );

        const { result } = renderHook(() =>
            useConversationEvents({
                conversationId,
            }),
        );

        await flushEffects();
        await flushEffects();

        expect(result.current.shiningMessageIds.has(assistantMessageId)).toBe(true);
        expect(result.current.runtimeState?.phase).toBe("assistant_streaming");

        await act(async () => {
            await emit(`conversation_event_${conversationId}`, {
                type: "shine_state_snapshot",
                data: {
                    state: createShineState(conversationId, 4, {
                        target_type: "none",
                    }, 2),
                },
            });
            await emit(`conversation_event_${conversationId}`, {
                type: "runtime_state_snapshot",
                data: {
                    state: createRuntimeState(conversationId, 4, "idle", 2),
                },
            });
            await emit(`conversation_event_${conversationId}`, {
                type: "shine_state_snapshot",
                data: {
                    state: createShineState(conversationId, 10, {
                        target_type: "none",
                    }, 1),
                },
            });
            await emit(`conversation_event_${conversationId}`, {
                type: "runtime_state_snapshot",
                data: {
                    state: createRuntimeState(conversationId, 10, "idle", 1),
                },
            });
        });

        await flushEffects();

        expect(result.current.shiningMessageIds.has(assistantMessageId)).toBe(true);
        expect(result.current.shiningMessageIds.size).toBe(1);
        expect(result.current.activityFocus).toEqual({
            focus_type: "assistant_streaming",
            message_id: assistantMessageId,
        });
        expect(result.current.runtimeState?.phase).toBe("assistant_streaming");
        expect(result.current.runtimeState?.epoch).toBe(2);
        expect(result.current.runtimeState?.revision).toBe(5);
    });

    it("clears active shine and runtime state when conversation is cancelled", async () => {
        const conversationId = 16;
        const callId = 44;
        let currentCalls = [createActiveCall(conversationId, callId)];
        let currentShine = createShineState(conversationId, 1, {
            target_type: "mcp_call",
            call_id: callId,
            reason: "mcp_executing",
        });
        let currentRuntime = createRuntimeState(conversationId, 1, "mcp_executing");

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => currentCalls);
        mockInvokeHandler("get_shine_state", () => currentShine);
        mockInvokeHandler("get_conversation_runtime_state", () => currentRuntime);

        const { result } = renderHook(() =>
            useConversationEvents({
                conversationId,
            }),
        );

        await flushEffects();
        await flushEffects();

        expect(result.current.shiningMcpCallId).toBe(callId);
        expect(result.current.activeMcpCallIds.has(callId)).toBe(true);
        expect(result.current.runtimeState?.phase).toBe("mcp_executing");

        currentCalls = [];
        currentShine = createShineState(conversationId, 2, { target_type: "none" });
        currentRuntime = createRuntimeState(conversationId, 2, "idle");

        await act(async () => {
            await emit(`conversation_event_${conversationId}`, {
                type: "conversation_cancel",
                data: {
                    conversation_id: conversationId,
                    cancelled_at: new Date("2024-01-01T00:00:00.000Z"),
                },
            });
        });

        await flushEffects();
        await flushEffects();

        expect(result.current.shiningMcpCallId).toBeNull();
        expect(result.current.shiningMessageIds.size).toBe(0);
        expect(result.current.activeMcpCallIds.size).toBe(0);
        expect(result.current.activityFocus).toEqual({ focus_type: "none" });
        expect(result.current.runtimeState?.phase).toBe("idle");
        expect(result.current.runtimeState?.is_running).toBe(false);
    });

    it("keeps pending MCP visible in state without promoting runtime or shine", async () => {
        const conversationId = 17;
        const callId = 55;
        const pendingCall: MCPToolCall = {
            ...createActiveCall(conversationId, callId),
            status: "pending",
        };

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => [pendingCall]);
        mockInvokeHandler("get_shine_state", () =>
            createShineState(conversationId, 1, { target_type: "none" }),
        );
        mockInvokeHandler("get_conversation_runtime_state", () =>
            createRuntimeState(conversationId, 1, "idle"),
        );

        const { result } = renderHook(() =>
            useConversationEvents({
                conversationId,
            }),
        );

        await flushEffects();
        await flushEffects();

        expect(result.current.mcpToolCallStates.get(callId)?.status).toBe("pending");
        expect(result.current.activeMcpCallIds.has(callId)).toBe(true);
        expect(result.current.shiningMcpCallId).toBeNull();
        expect(result.current.shiningMessageIds.size).toBe(0);
        expect(result.current.activityFocus).toEqual({ focus_type: "none" });
        expect(result.current.runtimeState?.phase).toBe("idle");
        expect(result.current.runtimeState?.is_running).toBe(false);
    });

    it("clears user-message shine after a final stream error message arrives", async () => {
        const conversationId = 18;
        const userMessageId = 901;
        let runtimeSyncCount = 0;
        let shineSyncCount = 0;

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => []);
        mockInvokeHandler("get_shine_state", () => {
            shineSyncCount += 1;
            if (shineSyncCount === 1) {
                return createShineState(conversationId, 1, {
                    target_type: "message",
                    message_id: userMessageId,
                    reason: "user_pending",
                }, 5);
            }
            return createShineState(conversationId, 2, { target_type: "none" }, 5);
        });
        mockInvokeHandler("get_conversation_runtime_state", () => {
            runtimeSyncCount += 1;
            if (runtimeSyncCount === 1) {
                return createRuntimeState(conversationId, 1, "user_pending", 5);
            }
            return createRuntimeState(conversationId, 2, "idle", 5);
        });

        const { result } = renderHook(() =>
            useConversationEvents({
                conversationId,
            }),
        );

        await flushEffects();
        await flushEffects();

        expect(result.current.shiningMessageIds.has(userMessageId)).toBe(true);
        expect(result.current.runtimeState?.phase).toBe("user_pending");

        await act(async () => {
            await emit(`conversation_event_${conversationId}`, {
                type: "message_update",
                data: {
                    message_id: 999,
                    message_type: "error",
                    content: "AI stream failed after retries",
                    is_done: true,
                },
            });
        });

        await flushEffects();
        await flushEffects();

        expect(result.current.shiningMessageIds.size).toBe(0);
        expect(result.current.shiningMcpCallId).toBeNull();
        expect(result.current.activityFocus).toEqual({ focus_type: "none" });
        expect(result.current.runtimeState?.phase).toBe("idle");
        expect(result.current.runtimeState?.is_running).toBe(false);
    });
});
