import { describe, expect, it, vi } from "vitest";
import { buildNativeAgentApprovalFields, isAgentAssistantField } from "./nativeAgentApprovalFields";

const getter = (values: Record<string, string>) => (name: string, fallback: string) => values[name] ?? fallback;
const keys = (fields: ReturnType<typeof buildNativeAgentApprovalFields>) => fields.map((field) => field.key);

describe("buildNativeAgentApprovalFields", () => {
    it("should offer each agent's native control plus AIPP review when the agent has no reviewer of its own", () => {
        expect(keys(buildNativeAgentApprovalFields("pi", getter({}), vi.fn(), true))).toEqual(["pi_approval", "tool_review_mode"]);
        expect(keys(buildNativeAgentApprovalFields("kiro", getter({}), vi.fn(), true))).toEqual(["kiro_autopilot", "tool_review_mode"]);
        expect(keys(buildNativeAgentApprovalFields("antigravity", getter({}), vi.fn(), true))).toEqual(["antigravity_mode", "tool_review_mode"]);
        expect(keys(buildNativeAgentApprovalFields("opencode", getter({}), vi.fn(), true))).toEqual(["opencode_approval_notice", "tool_review_mode"]);
    });

    it("should only explain Cursor's built-in review and leave other agents untouched", () => {
        expect(keys(buildNativeAgentApprovalFields("cursor", getter({}), vi.fn(), true))).toEqual(["cursor_approval_notice"]);
        expect(buildNativeAgentApprovalFields("codex_app_server", getter({}), vi.fn(), true)).toEqual([]);
    });

    it("should store the safe default as empty and other choices verbatim when changed", () => {
        const set = vi.fn();
        const [scope] = buildNativeAgentApprovalFields("pi", getter({ pi_approval: "read_only" }), set, true);
        expect(scope.config.value).toBe("read_only");
        scope.config.onChange?.("all");
        expect(set).toHaveBeenLastCalledWith("pi_approval", "", "string");
        scope.config.onChange?.("none");
        expect(set).toHaveBeenLastCalledWith("pi_approval", "none", "string");
    });

    it("should point to the review model setting when model review is chosen but not configured", () => {
        const hint = () => null;
        const fields = buildNativeAgentApprovalFields("pi", getter({ tool_review_mode: "auto_model" }), vi.fn(), false, hint);
        expect(keys(fields)).toEqual(["pi_approval", "tool_review_mode", "tool_review_model_hint"]);
        expect(fields[1].config.value).toBe("auto_model");
        expect(keys(buildNativeAgentApprovalFields("pi", getter({ tool_review_mode: "auto_model" }), vi.fn(), true, hint))).not.toContain("tool_review_model_hint");
    });
});

describe("isAgentAssistantField", () => {
    it("should persist agent settings but not display-only notices", () => {
        for (const key of ["acp_working_directory", "codex_sandbox", "pi_approval", "kiro_autopilot", "antigravity_mode"]) expect(isAgentAssistantField(key)).toBe(true);
        for (const key of ["opencode_approval_notice", "cursor_approval_notice", "tool_review_model_hint"]) expect(isAgentAssistantField(key)).toBe(false);
    });
});
