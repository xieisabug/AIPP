import { describe, expect, it } from "vitest";
import { contactVisual, type ContactActivity } from "./contactActivity";

export const activity = (overrides: Partial<ContactActivity> = {}): ContactActivity => ({
    assistant_id: 8, conversation_id: 3, status: null, waiting_peer: false, permission: false, question: false,
    runtime: { conversation_id: 3, is_running: false, phase: "idle", epoch: 1, revision: 1 }, ...overrides,
});
describe("contact activity priority", () => {
    it("should retain running work when another task completes", () => {
        const working = activity({ runtime: { ...activity().runtime, is_running: true, phase: "mcp_executing" } });
        expect(contactVisual([working, activity({ status: "succeeded" })], { state: "finished", until: 4000 }, false, 1000).state).toBe("working");
    });
    it("should prioritize permission and questions over errors and working", () => {
        expect(contactVisual([activity({ permission: true, question: true })], undefined, true, 0).label).toBe("待审批");
        expect(contactVisual([activity({ question: true })], undefined, true, 0).label).toBe("等你回答");
    });
    it("should distinguish model thinking from streaming", () => {
        expect(contactVisual([activity({ runtime: { ...activity().runtime, is_running: true, phase: "user_pending" } })], undefined, false, 0).state).toBe("thinking");
        expect(contactVisual([activity({ runtime: { ...activity().runtime, is_running: true, phase: "assistant_streaming" } })], undefined, false, 0).state).toBe("streaming");
    });
    it("should retain an error indicator after its one-shot expression", () => {
        expect(contactVisual([], { state: "failed", until: 100 }, true, 200)).toEqual({ state: "idle", label: "执行出错" });
    });
    it("should distinguish peer waiting and avoid replaying historical terminal tasks", () => {
        expect(contactVisual([activity({ waiting_peer: true })], undefined, false, 0).label).toBe("等待协作");
        for (const status of ["succeeded", "failed", "cancelled", "interrupted"]) {
            expect(contactVisual([activity({ status })], undefined, false, 0).state).toBe("idle");
        }
    });
});
