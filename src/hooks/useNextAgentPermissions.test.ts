import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { AcpConversationSessionState } from "@/data/Conversation";
import { useNextAgentPermissions } from "./useNextAgentPermissions";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

it("should show the latest selection during a reply and discard it when the native session changes", async () => {
    let onChange: (event: { payload: number }) => void = () => {};
    listen.mockImplementation(async (_, handler) => { onChange = handler; return () => {}; });
    invoke.mockResolvedValueOnce({}).mockResolvedValueOnce({ autopilot: "on" }).mockResolvedValueOnce({});
    const snapshot: AcpConversationSessionState = {
        conversation_id: 7, session_id: "first", agent_kind: "kiro", has_active_prompt: true,
        prompt_capabilities: { image: false, audio: false, embedded_context: false },
        config_options: [{ id: "autopilot", name: "Autopilot", current_value: "off", options: [] }],
        modes: [], plan: [], available_commands: [],
    };
    const { result, rerender } = renderHook(useNextAgentPermissions, { initialProps: snapshot });
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    await act(async () => onChange({ payload: 7 }));
    expect(result.current?.config_options[0].current_value).toBe("on");
    rerender({ ...snapshot, has_active_prompt: false });
    expect(result.current?.config_options[0].current_value).toBe("on");
    expect(snapshot.config_options[0].current_value).toBe("off");
    rerender({ ...snapshot, session_id: "second" });
    expect(result.current?.config_options[0].current_value).toBe("off");
    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("get_agent_next_permissions", {
        conversationId: 7, driver: "kiro", sessionId: "second",
    }));
});
