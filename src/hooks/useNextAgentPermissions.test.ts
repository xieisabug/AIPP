import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { AcpConversationSessionState } from "@/data/Conversation";
import { useNextAgentPermissions } from "./useNextAgentPermissions";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

it("shows the CLI-confirmed Claude mode even if an earlier selection is stored", () => {
    invoke.mockReset().mockResolvedValue({ permission_mode: "auto" });
    const snapshot: AcpConversationSessionState = {
        conversation_id: 7, agent_kind: "claude_sdk", session_id: "live", has_active_prompt: true,
        prompt_capabilities: { image: false, audio: false, embedded_context: false },
        config_options: [{ id: "permission_mode", name: "权限", current_value: "default", options: [] }],
        modes: [], plan: [], available_commands: [],
    };
    const { result } = renderHook(() => useNextAgentPermissions(snapshot));
    expect(result.current).toBe(snapshot);
    expect(invoke).not.toHaveBeenCalled();
});

it.each([
    ["kiro", "autopilot", "off", "on"],
    ["pi", "pi_approval", "all", "read_only"],
])("should show the latest %s selection during a reply and discard it when the native session changes", async (driver, key, initial, next) => {
    invoke.mockReset();
    let onChange: (event: { payload: number }) => void = () => {};
    listen.mockImplementation(async (_, handler) => { onChange = handler; return () => {}; });
    invoke.mockResolvedValueOnce({}).mockResolvedValueOnce({ [key]: next }).mockResolvedValueOnce({});
    const snapshot: AcpConversationSessionState = {
        conversation_id: 7, session_id: "first", agent_kind: driver, has_active_prompt: true,
        prompt_capabilities: { image: false, audio: false, embedded_context: false },
        config_options: [{ id: key, name: key, current_value: initial, options: [] }],
        modes: [], plan: [], available_commands: [],
    };
    const { result, rerender } = renderHook(useNextAgentPermissions, { initialProps: snapshot });
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    await act(async () => onChange({ payload: 7 }));
    expect(result.current?.config_options[0].current_value).toBe(next);
    rerender({ ...snapshot, has_active_prompt: false });
    expect(result.current?.config_options[0].current_value).toBe(next);
    expect(snapshot.config_options[0].current_value).toBe(initial);
    rerender({ ...snapshot, session_id: "second" });
    expect(result.current?.config_options[0].current_value).toBe(initial);
    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("get_agent_next_permissions", {
        conversationId: 7, driver, sessionId: "second",
    }));
});
