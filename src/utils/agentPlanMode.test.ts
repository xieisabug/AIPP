import { describe, expect, it } from "vitest";

import { agentSupportsPlanToggle, shouldShowPlanToggle } from "./agentPlanMode";

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

describe("shouldShowPlanToggle", () => {
    const base = {
        isAgentAssistant: true as boolean | null,
        agentKind: "claude_sdk" as string | null,
        hasSessionPlanOption: false,
        isMobilePlatform: false,
    };

    it("should show Plan for a Claude Code or Codex agent assistant", () => {
        expect(shouldShowPlanToggle(base)).toBe(true);
        expect(shouldShowPlanToggle({ ...base, agentKind: "codex_app_server" })).toBe(true);
    });

    it("should hide Plan when the current assistant is an ordinary assistant even if stale agent info remains", () => {
        expect(shouldShowPlanToggle({ ...base, isAgentAssistant: false })).toBe(false);
        expect(shouldShowPlanToggle({ ...base, isAgentAssistant: false, hasSessionPlanOption: true })).toBe(false);
    });

    it("should rely on session info when the assistant list is not loaded", () => {
        expect(shouldShowPlanToggle({ ...base, isAgentAssistant: null, agentKind: "acp", hasSessionPlanOption: true })).toBe(true);
        expect(shouldShowPlanToggle({ ...base, isAgentAssistant: null, agentKind: null })).toBe(false);
    });

    it("should hide Plan for an agent without a plan mode", () => {
        expect(shouldShowPlanToggle({ ...base, agentKind: "acp" })).toBe(false);
    });

    it("should hide Plan on the mobile platform", () => {
        expect(shouldShowPlanToggle({ ...base, isMobilePlatform: true })).toBe(false);
    });
});
