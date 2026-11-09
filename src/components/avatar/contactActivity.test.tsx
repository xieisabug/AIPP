import { describe, expect, it } from "vitest";
import { contactVisual, toolVisual, type ContactActivity } from "./contactActivity";

export const activity = (overrides: Partial<ContactActivity> = {}): ContactActivity => ({
    assistant_id: 8, conversation_id: 3, status: null, waiting_peer: false, permission: false, question: false,
    runtime: { conversation_id: 3, is_running: false, phase: "idle", epoch: 1, revision: 1 }, ...overrides,
});
const running = (phase: ContactActivity["runtime"]["phase"], detail?: ContactActivity["detail"]) =>
    activity({ runtime: { ...activity().runtime, is_running: true, phase }, detail });
const stateOf = (rows: ContactActivity[]) => contactVisual(rows, undefined, false, 0).state;

describe("contact activity priority", () => {
    it("should retain running work when another task completes", () => {
        expect(contactVisual([running("mcp_executing"), activity({ status: "succeeded" })], { state: "finished", until: 4000 }, false, 1000).state).toBe("working");
    });
    it("should prioritize permission and questions over errors and working", () => {
        expect(contactVisual([activity({ permission: true, question: true })], undefined, true, 0).label).toBe("待审批");
        expect(contactVisual([activity({ question: true })], undefined, true, 0).label).toBe("等你回答");
    });
    it("should distinguish pending, reasoning, planning and answering", () => {
        expect(stateOf([running("user_pending")])).toBe("listening");
        expect(stateOf([running("assistant_streaming")])).toBe("streaming");
        expect(stateOf([running("assistant_streaming", { stream: "reasoning" })])).toBe("thinking");
        expect(stateOf([running("assistant_streaming", { stream: "plan" })])).toBe("writing");
    });
    it("should show auto review above the executing tool", () => {
        expect(stateOf([running("mcp_executing", { tool: "execute_bash", reviewing: true })])).toBe("suspicious");
    });
    it("should map executing tools to matching actions", () => {
        expect(stateOf([running("mcp_executing", { tool: "search_web" })])).toBe("searching");
        expect(stateOf([running("mcp_executing", { tool: "read_file" })])).toBe("reading");
        expect(stateOf([running("mcp_executing", { tool: "execute_bash" })])).toBe("running");
        expect(stateOf([running("mcp_executing", { tool: "some_third_party_tool" })])).toBe("working");
        expect(toolVisual("fetch_url").state).toBe("receiving");
        expect(toolVisual("load_skill").state).toBe("loading");
        expect(toolVisual("superadmin_execute").state).toBe("controlling");
        expect(toolVisual("message_bot").state).toBe("sending");
        expect(toolVisual("show_artifact").state).toBe("spawning");
        expect(toolVisual("todo_write").state).toBe("writing");
    });
    it("should treat edits to source files as coding and other files as writing", () => {
        expect(toolVisual("edit_file", "src/app.tsx").state).toBe("coding");
        expect(toolVisual("write_file", "notes/plan.md").state).toBe("writing");
        expect(toolVisual("write_file").state).toBe("writing");
    });
    it("should separate accepted tasks from running tasks", () => {
        expect(stateOf([activity({ status: "accepted" })])).toBe("receiving");
        expect(stateOf([activity({ status: "running" })])).toBe("working");
    });
    it("should retain an error indicator after its one-shot expression", () => {
        expect(contactVisual([], { state: "failed", until: 100 }, true, 200)).toEqual({ state: "idle", label: "执行出错" });
    });
    it("should play celebration and interruption feedback once, then rest", () => {
        expect(contactVisual([], { state: "celebrate", until: 100 }, false, 50)).toEqual({ state: "celebrate", label: "全部完成" });
        expect(contactVisual([], { state: "blocked", until: 100 }, false, 50)).toEqual({ state: "blocked", label: "已中断" });
        expect(contactVisual([], { state: "blocked", until: 100 }, false, 200).state).toBe("idle");
    });
    it("should distinguish peer waiting and avoid replaying historical terminal tasks", () => {
        expect(contactVisual([activity({ waiting_peer: true })], undefined, false, 0).label).toBe("等待协作");
        for (const status of ["succeeded", "failed", "cancelled", "interrupted"]) {
            expect(contactVisual([activity({ status })], undefined, false, 0).state).toBe("idle");
        }
    });
});
