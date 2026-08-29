import { describe, expect, it } from "vitest";
import {
    buildAgentConfigFormValues,
    serializeAgentConfigFormValues,
} from "./agentConfigFormValues";

describe("agentConfigFormValues", () => {
    it("defaults all Agent completion notifications to disabled", () => {
        expect(buildAgentConfigFormValues()).toEqual({
            codex_notification_on_success: false,
            claude_code_notification_on_success: false,
            acp_notification_on_success: false,
        });
    });

    it("loads and serializes each Agent switch independently", () => {
        const values = buildAgentConfigFormValues(
            new Map([
                ["codex_notification_on_success", "true"],
                ["claude_code_notification_on_success", "false"],
                ["acp_notification_on_success", "true"],
            ])
        );

        expect(serializeAgentConfigFormValues(values)).toEqual({
            codex_notification_on_success: "true",
            claude_code_notification_on_success: "false",
            acp_notification_on_success: "true",
        });
    });
});
