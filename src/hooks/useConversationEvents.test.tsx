import React from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useConversationEvents } from "@/hooks/useConversationEvents";
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

        await act(async () => {
            staleAcpSync.resolve(null);
            await Promise.resolve();
        });

        expect(screen.getByTestId("session-id")).toHaveTextContent("session-resume");
        expect(screen.getByTestId("config-count")).toHaveTextContent("1");
    });
});
