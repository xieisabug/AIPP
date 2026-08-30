import { describe, expect, it } from "vitest";
import type { AcpPlanEntry } from "@/data/Conversation";
import { getAgentPlanStatus } from "./AgentPlanCard";

const plan = (statuses: string[]): AcpPlanEntry[] =>
    statuses.map((status, index) => ({
        content: `步骤 ${index + 1}`,
        priority: "medium",
        status,
    }));

describe("getAgentPlanStatus", () => {
    it("shows planning while a Plan turn is active", () => {
        expect(getAgentPlanStatus(plan(["pending"]), true, true)).toBe("planning");
    });

    it("waits for confirmation after Plan generation", () => {
        expect(getAgentPlanStatus(plan(["pending"]), false, true)).toBe("awaiting_confirmation");
    });

    it("shows execution outside Plan mode", () => {
        expect(getAgentPlanStatus(plan(["in_progress"]), true, false)).toBe("executing");
    });

    it("shows completion when every step is complete", () => {
        expect(getAgentPlanStatus(plan(["completed", "completed"]), false, false)).toBe("completed");
    });
});
