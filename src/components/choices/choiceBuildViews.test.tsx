import { afterEach, expect, it, vi } from "vitest";
import { acquireChoiceBuild } from "./choiceBuildViews";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); });

it("should transfer the active lease when moving the preview into and out of fullscreen", async () => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockResolvedValue({ url: "http://test.localhost", lease: "one" });
    const ref = { conversationId: 1, requestId: "r", candidateId: "c" };
    const inline = acquireChoiceBuild(ref);
    inline.release();
    const full = acquireChoiceBuild(ref);
    await vi.runAllTimersAsync();
    full.release();
    const back = acquireChoiceBuild(ref);
    await vi.runAllTimersAsync();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(back.ready).toBe(inline.ready);
    back.release();
    await vi.runAllTimersAsync();
    expect(invoke).toHaveBeenLastCalledWith("close_choice_build", { lease: "one" });
});
