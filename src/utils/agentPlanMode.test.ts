import { describe, expect, it } from "vitest";

import { agentSupportsPlanToggle } from "./agentPlanMode";

describe("agentSupportsPlanToggle", () => {
    it("shows Plan for Codex and Claude Code even before a session exists", () => {
        expect(agentSupportsPlanToggle("codex_app_server", false)).toBe(true);
        expect(agentSupportsPlanToggle("claude_sdk", false)).toBe(true);
    });

    it("shows Plan when the session exposes a plan mode option", () => {
        expect(agentSupportsPlanToggle("acp", true)).toBe(true);
        expect(agentSupportsPlanToggle(null, true)).toBe(true);
    });

    it("hides Plan for ordinary assistants and ACP sessions without a plan option", () => {
        expect(agentSupportsPlanToggle("acp", false)).toBe(false);
        expect(agentSupportsPlanToggle(null, false)).toBe(false);
        expect(agentSupportsPlanToggle("openai", false)).toBe(false);
    });
});
