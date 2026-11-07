import React from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { normalizeAcpSessionState, useConversationEvents } from "@/hooks/useConversationEvents";
import type { AcpConversationSessionState } from "@/data/Conversation";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

function createDeferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((promiseResolve) => {
        resolve = promiseResolve;
    });
    return { promise, resolve };
}

function baseAcpSessionState(
    overrides: Partial<AcpConversationSessionState> = {},
): AcpConversationSessionState {
    return {
        conversation_id: 42,
        session_id: "session-resume",
        title: null,
        updated_at: null,
        load_session_supported: false,
        session_resume_supported: true,
        restored_session_method: "resume",
        prompt_capabilities: {
            image: true,
            audio: false,
            embedded_context: true,
        },
        current_mode_id: null,
        modes: [],
        config_options: [
            {
                id: "model",
                name: "Model",
                category: "model",
                current_value: "sonnet",
                options: [{ value: "sonnet", name: "Sonnet" }],
            },
        ],
        plan: [],
        available_commands: [],
        has_active_prompt: false,
        context_tokens_used: 2048,
        context_window_size: 8192,
        session_cost_amount: 1.25,
        session_cost_currency: "USD",
        ...overrides,
    };
}

const HookHarness = () => {
    const { acpSessionState, applyAcpSessionState } = useConversationEvents({
        conversationId: 42,
    });

    return (
        <div>
            <div data-testid="session-id">{acpSessionState?.session_id ?? "none"}</div>
            <div data-testid="config-count">
                {acpSessionState?.config_options.length ?? 0}
            </div>
            <div data-testid="context-used">
                {acpSessionState?.context_tokens_used ?? "none"}
            </div>
            <div data-testid="context-size">
                {acpSessionState?.context_window_size ?? "none"}
            </div>
            <button
                type="button"
                onClick={() => applyAcpSessionState(baseAcpSessionState())}
            >
                apply session
            </button>
        </div>
    );
};

describe("useConversationEvents ACP session state", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("keeps auto-connected ACP state when an older null sync resolves later", async () => {
        const staleAcpSync = createDeferred<AcpConversationSessionState | null>();

        mockInvokeHandler("get_conversation_runtime_state", () => ({
            conversation_id: 42,
            is_running: false,
            phase: "idle",
            epoch: 0,
            revision: 0,
        }));
        mockInvokeHandler("get_conversation_shine_state", () => ({
            conversation_id: 42,
            epoch: 0,
            revision: 0,
            primary_target: { target_type: "none" },
        }));
        mockInvokeHandler("get_acp_session_state", () => staleAcpSync.promise);

        render(<HookHarness />);

        fireEvent.click(screen.getByText("apply session"));
        expect(screen.getByTestId("session-id")).toHaveTextContent("session-resume");
        expect(screen.getByTestId("config-count")).toHaveTextContent("1");
        expect(screen.getByTestId("context-used")).toHaveTextContent("2048");
        expect(screen.getByTestId("context-size")).toHaveTextContent("8192");

        await act(async () => {
            staleAcpSync.resolve(null);
            await Promise.resolve();
        });

        expect(screen.getByTestId("session-id")).toHaveTextContent("session-resume");
        expect(screen.getByTestId("config-count")).toHaveTextContent("1");
        expect(screen.getByTestId("context-used")).toHaveTextContent("2048");
        expect(screen.getByTestId("context-size")).toHaveTextContent("8192");
    });
});


describe("normalizeAcpSessionState capability compatibility", () => {
    it("should keep capability absence unknown when reading a legacy snapshot", () => {
        const normalized = normalizeAcpSessionState(baseAcpSessionState());
        expect(normalized?.agent_capabilities).toBeNull();
        expect(normalized?.prompt_capabilities.image).toBe(true);
        expect(normalized?.config_options[0].current_value).toBe("sonnet");
    });

    it("should preserve live capability evidence and opaque choices when normalizing", () => {
        const capabilities = {
            schema_version: 1,
            driver_id: "vendor-agent",
            discovery: "session",
            configuration: {
                source: "session",
                complete: true,
                options: [{ id: "vendor/model:v2", current_value: "", options: [{ value: " opaque value " }] }],
            },
            operations: { set_config_option: { support: "unknown", source: "unknown" } },
        };
        const normalized = normalizeAcpSessionState({
            conversationId: 42,
            sessionId: "native-session",
            agentCapabilities: capabilities,
            configOptions: [{ id: "vendor/model:v2", currentValue: "", options: [{ value: " opaque value ", groupName: "vendor" }] }],
        });
        expect(normalized?.agent_capabilities).toBe(capabilities);
        expect(normalized?.agent_capabilities?.operations.set_config_option.support).toBe("unknown");
        expect(normalized?.config_options[0].options[0].value).toBe(" opaque value ");
        expect(normalized?.config_options[0].options[0].group_name).toBe("vendor");
        expect(normalized?.config_options[0].current_value).toBe("");
    });

    it("should replace capability data when a new session snapshot arrives", () => {
        const first = normalizeAcpSessionState({ ...baseAcpSessionState(), agent_capabilities: { driver_id: "provider-a" } });
        const second = normalizeAcpSessionState({ ...baseAcpSessionState(), agent_capabilities: { driver_id: "provider-b" } });
        expect(first?.agent_capabilities?.driver_id).toBe("provider-a");
        expect(second?.agent_capabilities?.driver_id).toBe("provider-b");
        expect(normalizeAcpSessionState(null)).toBeNull();
    });
});
