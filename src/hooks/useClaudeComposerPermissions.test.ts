import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { AcpConversationSessionState } from "@/data/Conversation";
import { useClaudeComposerPermissions } from "./useClaudeComposerPermissions";

const option = { id: "permission_mode", name: "权限", current_value: "default", options: [] };
const session: AcpConversationSessionState = {
    conversation_id: 7, agent_kind: "claude_sdk", session_id: "original", has_active_prompt: true,
    prompt_capabilities: { image: false, audio: false, embedded_context: false },
    modes: [], plan: [], available_commands: [], config_options: [option],
};

it("should save each connected selection for dispatch and keep plan separate from the underlying mode", async () => {
    const save = vi.fn().mockResolvedValue(true);
    const { result, rerender } = renderHook((s: AcpConversationSessionState) =>
        useClaudeComposerPermissions("7", "default", s, save), { initialProps: session });
    for (const mode of ["auto", "dontAsk"]) {
        await act(async () => { await result.current.choose(mode, false); });
        expect(save).toHaveBeenLastCalledWith(option, mode);
        expect(result.current.initialOverride).toBeUndefined();
    }
    await act(async () => { await result.current.choose("dontAsk", true); });
    expect(save).toHaveBeenLastCalledWith(option, "plan");
    rerender({ ...session, config_options: [{ ...option, current_value: "plan" }] });
    expect(result.current.plan).toBe(true);
    expect(result.current.mode).toBe("dontAsk");
});

it("should carry the first selection until the session can save it", async () => {
    const save = vi.fn().mockResolvedValue(true);
    const { result, rerender } = renderHook((s: AcpConversationSessionState | null) =>
        useClaudeComposerPermissions("7", "default", s, save), { initialProps: null as AcpConversationSessionState | null });
    await act(async () => { await result.current.choose("acceptEdits", false); });
    expect(result.current.initialOverride).toBe("acceptEdits");
    expect(save).not.toHaveBeenCalled();
    rerender(session);
    await waitFor(() => expect(save).toHaveBeenCalledWith(option, "acceptEdits"));
    expect(result.current.initialOverride).toBeUndefined();
});
