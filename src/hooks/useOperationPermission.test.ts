import { act, renderHook, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useAcpPermission, useOperationPermission } from "@/hooks/useOperationPermission";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

const flushEffects = async () => {
    await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
    });
};

describe("useOperationPermission", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("closes stale operation dialogs when backend reports the request was already resolved", async () => {
        mockInvokeHandler("confirm_operation_permission", () => {
            throw new Error("Permission request not found or already resolved");
        });

        const { result } = renderHook(() => useOperationPermission({ conversationId: 7 }));
        await flushEffects();

        await act(async () => {
            await emit("operation-permission-request", {
                request_id: "op-1",
                conversation_id: 7,
                operation: "read_file",
                path: "/tmp/demo.txt",
            });
        });

        await waitFor(() => {
            expect(result.current.pendingRequest?.request_id).toBe("op-1");
        });

        await act(async () => {
            await result.current.handleDecision("op-1", "allow_for_conversation");
        });

        await waitFor(() => {
            expect(result.current.pendingRequest).toBeNull();
        });
        expect(result.current.isDialogOpen).toBe(false);
        expect(result.current.decisionError).toBeNull();
    });

    it("keeps the current operation request open when confirmation fails", async () => {
        let attempts = 0;
        mockInvokeHandler("confirm_operation_permission", () => {
            attempts += 1;
            if (attempts === 1) {
                throw new Error("temporary failure");
            }
            return true;
        });

        const { result } = renderHook(() => useOperationPermission({ conversationId: 7 }));
        await flushEffects();

        await act(async () => {
            await emit("operation-permission-request", {
                request_id: "op-1",
                conversation_id: 7,
                operation: "read_file",
                path: "/tmp/demo.txt",
            });
        });

        await waitFor(() => {
            expect(result.current.pendingRequest?.request_id).toBe("op-1");
        });

        await act(async () => {
            await result.current.handleDecision("op-1", "allow_for_conversation");
        });

        expect(result.current.pendingRequest?.request_id).toBe("op-1");
        expect(result.current.isDialogOpen).toBe(true);
        expect(result.current.decisionError).toBe("temporary failure");

        await act(async () => {
            await result.current.handleDecision("op-1", "allow_for_conversation");
        });

        await waitFor(() => {
            expect(result.current.pendingRequest).toBeNull();
        });
        expect(result.current.decisionError).toBeNull();
    });

    it("removes stale operation dialogs when another window resolves the request", async () => {
        const { result } = renderHook(() => useOperationPermission({ conversationId: 7 }));
        await flushEffects();

        await act(async () => {
            await emit("operation-permission-request", {
                request_id: "op-1",
                conversation_id: 7,
                operation: "read_file",
                path: "/tmp/one.txt",
            });
            await emit("operation-permission-request", {
                request_id: "op-2",
                conversation_id: 7,
                operation: "read_file",
                path: "/tmp/two.txt",
            });
        });

        await waitFor(() => {
            expect(result.current.pendingRequest?.request_id).toBe("op-1");
        });

        await act(async () => {
            await emit("operation-permission-resolved", {
                request_id: "op-1",
                conversation_id: 7,
            });
        });

        await waitFor(() => {
            expect(result.current.pendingRequest?.request_id).toBe("op-2");
        });
        expect(result.current.isDialogOpen).toBe(true);
    });
});

describe("useAcpPermission", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("closes stale ACP dialogs when backend reports the request was already resolved", async () => {
        mockInvokeHandler("confirm_acp_permission", () => {
            throw new Error("ACP permission request not found or already resolved");
        });

        const { result } = renderHook(() => useAcpPermission({ conversationId: 9 }));
        await flushEffects();

        await act(async () => {
            await emit("acp-permission-request", {
                request_id: "acp-1",
                conversation_id: 9,
                tool_call_id: "tool-1",
                options: [],
            });
        });

        await waitFor(() => {
            expect(result.current.pendingRequest?.request_id).toBe("acp-1");
        });

        await act(async () => {
            await result.current.handleDecision("acp-1", "allow_once");
        });

        await waitFor(() => {
            expect(result.current.pendingRequest).toBeNull();
        });
        expect(result.current.isDialogOpen).toBe(false);
        expect(result.current.decisionError).toBeNull();
    });

    it("removes stale ACP dialogs when another window resolves the request", async () => {
        const { result } = renderHook(() => useAcpPermission({ conversationId: 9 }));
        await flushEffects();

        await act(async () => {
            await emit("acp-permission-request", {
                request_id: "acp-1",
                conversation_id: 9,
                tool_call_id: "tool-1",
                options: [],
            });
        });

        await waitFor(() => {
            expect(result.current.pendingRequest?.request_id).toBe("acp-1");
        });

        await act(async () => {
            await emit("acp-permission-resolved", {
                request_id: "acp-1",
                conversation_id: 9,
            });
        });

        await waitFor(() => {
            expect(result.current.pendingRequest).toBeNull();
        });
        expect(result.current.isDialogOpen).toBe(false);
    });
});
