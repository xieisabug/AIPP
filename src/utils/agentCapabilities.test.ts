import { describe, expect, it } from "vitest";
import type { AgentCapabilities } from "@/data/Conversation";
import { agentAllowsConfigMutation } from "./agentCapabilities";

const unknown = { support: "unknown", source: "unknown" } as const;

export function capabilityFixture(overrides: Partial<AgentCapabilities> = {}): AgentCapabilities {
    return {
        schema_version: 1,
        driver_id: "extension-agent",
        discovery: "baseline",
        adapter: {
            send_prompt: true, cancel_prompt: true, set_config_option: true,
            respond_permission: null, attachment_input: "structured", image_input: null,
            audio_input: null, embedded_context_input: null, load_session: null,
            resume_session: null, config_catalog_complete: false,
        },
        operations: { send_prompt: unknown, cancel_prompt: unknown, set_config_option: unknown, respond_permission: unknown },
        attachments: { input: "structured", structured: unknown, image: unknown, audio: unknown, embedded_context: unknown },
        sessions: { load: unknown, resume: unknown },
        configuration: { source: "unknown", complete: false, options: [] },
        ...overrides,
    };
}

describe("agentAllowsConfigMutation", () => {
    it("should preserve native validation when discovery is absent or unknown", () => {
        expect(agentAllowsConfigMutation()).toBe(true);
        expect(agentAllowsConfigMutation(null)).toBe(true);
        expect(agentAllowsConfigMutation(capabilityFixture())).toBe(true);
        // A persisted or future schema can omit fields this client understands.
        expect(agentAllowsConfigMutation({ schema_version: 2 } as AgentCapabilities)).toBe(true);
    });

    it("should disable configuration when the registered session reports unsupported", () => {
        const capabilities = capabilityFixture();
        capabilities.operations.set_config_option = { support: "unsupported", source: "session" };
        expect(agentAllowsConfigMutation(capabilities)).toBe(false);
    });

    it("should update gating when a live capability replaces an earlier declaration", () => {
        const capabilities = capabilityFixture();
        capabilities.operations.set_config_option = { support: "unsupported", source: "adapter" };
        expect(agentAllowsConfigMutation(capabilities)).toBe(false);
        capabilities.operations.set_config_option = { support: "supported", source: "session" };
        expect(agentAllowsConfigMutation(capabilities)).toBe(true);
    });
});
