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

    it.each(["claude_sdk", "anthropic", "acp"])("should persist AIPP approval mode for %s", (apiType) => {
        const set = vi.fn();
        const field = buildNativeAgentApprovalFields(apiType, getter({}), set, true).find(f => f.key === "tool_review_mode")!;
        expect(field.key).toBe("tool_review_mode");
        expect(field.config.value).toBe("auto_model");
        field.config.onChange?.("auto_model");
        expect(set).toHaveBeenCalledWith("tool_review_mode", "auto_model", "string");
    });

    it("stores explicit Claude default separately from inheriting the provider", () => {
        const set = vi.fn();
        const [field] = buildNativeAgentApprovalFields("claude_sdk", getter({}), set, false);
        expect(field.config.value).toBe("inherit");
        for (const [choice, stored] of [["auto", "auto"], ["default", "default"], ["inherit", ""]]) {
            field.config.onChange?.(choice);
            expect(set).toHaveBeenLastCalledWith("claude_permission_mode", stored, "string");
        }
    });

    it.each([
        [true, "", "auto_model"], [false, "", "off"],
        [true, "off", "off"], [false, "auto_model", "auto_model"],
    ] as const)("should resolve Antigravity review with model configured=%s and saved mode '%s'", (configured, saved, expected) => {
        const [, field] = buildNativeAgentApprovalFields("antigravity", getter({ tool_review_mode: saved }), vi.fn(), configured);
        expect(field.config.value).toBe(expected);
    });

    it("should retain saved Antigravity auto-approval and offer the native execution modes", () => {
        const set = vi.fn();
        const [field] = buildNativeAgentApprovalFields("antigravity", getter({ antigravity_mode: "yolo" }), set, true);
        expect(field.config.value).toBe("yolo");
        expect(field.config.options?.map(option => option.value)).toEqual(["default", "auto_edit", "auto", "plan", "yolo"]);
        field.config.onChange?.("auto");
        expect(set).toHaveBeenCalledWith("antigravity_mode", "auto", "string");
    });

    it("should inherit explicit provider approval before the auxiliary model default", () => {
        const [, field] = buildNativeAgentApprovalFields("kiro", getter({}), vi.fn(), true, undefined, "off");
        expect(field.config.inheritedValue).toBe("off");
        expect(field.config.value).toBe("off");
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
