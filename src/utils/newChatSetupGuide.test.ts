import { describe, expect, it } from "vitest";
import { resolveSetupGuide } from "./newChatSetupGuide";

const enabledProvider = { id: 1, is_enabled: true };
const model = { code: "gpt-x", llm_provider_id: 1 };

describe("resolveSetupGuide", () => {
    it("guides a fresh install to add a provider", () => {
        expect(resolveSetupGuide({ providers: [], models: [], assistantCount: 1 })?.kind).toBe("no_provider");
    });

    it("treats only-disabled providers as not sendable", () => {
        const guide = resolveSetupGuide({ providers: [{ id: 1, is_enabled: false }], models: [], assistantCount: 1 });
        expect(guide?.kind).toBe("providers_disabled");
        expect(guide?.primary).toBe("providers");
    });

    it("does not equate having a provider with being able to send", () => {
        expect(resolveSetupGuide({ providers: [enabledProvider], models: [], assistantCount: 1 })?.kind).toBe("no_models");
    });

    it("asks for an assistant when only providers exist", () => {
        const guide = resolveSetupGuide({ providers: [enabledProvider], models: [model], assistantCount: 0 });
        expect(guide?.kind).toBe("no_assistant");
        expect(guide?.primary).toBe("assistant");
    });

    it("flags an assistant bound to a deleted or disabled-provider model", () => {
        expect(resolveSetupGuide({
            providers: [enabledProvider], models: [model], assistantCount: 1,
            assistantModel: { provider_id: 2, model_code: "gpt-x" },
        })?.kind).toBe("assistant_model_invalid");
        expect(resolveSetupGuide({
            providers: [enabledProvider], models: [model], assistantCount: 1, assistantModel: null,
        })?.kind).toBe("assistant_model_invalid");
    });

    it("shows no guide when everything needed to send is ready", () => {
        expect(resolveSetupGuide({
            providers: [enabledProvider], models: [model], assistantCount: 1,
            assistantModel: { provider_id: 1, model_code: "gpt-x" },
        })).toBeNull();
        // 未检查助手绑定（例如 Agent 助手）时也不误报
        expect(resolveSetupGuide({ providers: [enabledProvider], models: [model], assistantCount: 1 })).toBeNull();
    });
});
