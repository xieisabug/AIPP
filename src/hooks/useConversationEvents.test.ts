import { act, renderHook } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MCPToolCall } from "@/data/MCPToolCall";
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
});
