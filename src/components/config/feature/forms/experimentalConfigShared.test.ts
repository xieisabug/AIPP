import { describe, expect, it, vi } from "vitest";
import { buildExperimentalConfigFormValues, saveExperimentalConfigValues } from "./experimentalConfigShared";

describe("butler capability rollout settings", () => {
    it("should preserve explicit disabled capabilities while defaulting old installs to enabled", () => {
        const values = buildExperimentalConfigFormValues(new Map([
            ["experimental", new Map([["butler_peer_collaboration_enabled", "false"]])],
        ]));
        expect(values.butler_peer_collaboration_enabled).toBe("false");
        expect(values.butler_task_dispatch_enabled).toBe("true");
        expect(values.butler_work_continuation_enabled).toBe("true");
    });
    it("should persist disabled capabilities without changing unrelated configuration", async () => {
        const save = vi.fn(async () => undefined);
        const values = buildExperimentalConfigFormValues(new Map());
        await saveExperimentalConfigValues(save, { ...values, butler_task_dispatch_enabled: false });
        expect(save).toHaveBeenCalledWith("experimental", expect.objectContaining({
            butler_task_dispatch_enabled: "false",
            butler_peer_collaboration_enabled: "true",
            butler_work_continuation_enabled: "true",
        }), true);
    });
});

describe("conversation summary model selection", () => {
    it.each(["experimental", "conversation_summary"])("should load split and combined selections from %s", (group) => {
        for (const [model, provider] of [["shared", "7"], ["shared%%7", ""]]) {
            const config = new Map([[group, new Map([
                ["conversation_summary_model", model], ["conversation_summary_provider_id", provider],
            ])]]);
            expect(buildExperimentalConfigFormValues(config).conversation_summary_model).toBe("shared%%7");
        }
    });

    it("should require a new selection when the active configuration has lost its provider", () => {
        const config = new Map([
            ["experimental", new Map([["conversation_summary_model", "shared"]])],
            ["conversation_summary", new Map([["conversation_summary_model", "shared%%9"]])],
        ]);
        expect(buildExperimentalConfigFormValues(config).conversation_summary_model).toBe("");
    });

    it("should reload the selected provider when the experimental form saves a model", async () => {
        const save = vi.fn();
        const form = buildExperimentalConfigFormValues(new Map());
        await saveExperimentalConfigValues(save, { ...form, conversation_summary_model: "shared%%7" });
        const [group, changes] = save.mock.calls[0];
        expect(changes.conversation_summary_model).toBe("shared");
        expect(changes.conversation_summary_provider_id).toBe("7");
        const saved = new Map([[group, new Map(Object.entries(changes as Record<string, string>))]]);
        expect(buildExperimentalConfigFormValues(saved).conversation_summary_model).toBe("shared%%7");
    });

    it("should omit the summary selection when saving Butler settings", async () => {
        const save = vi.fn();
        await saveExperimentalConfigValues(save, buildExperimentalConfigFormValues(new Map()), "butler");
        const [group, changes, partial] = save.mock.calls[0];
        expect(group).toBe("experimental");
        expect(partial).toBe(true);
        expect(changes).not.toHaveProperty("conversation_summary_model");
        expect(changes).not.toHaveProperty("conversation_summary_provider_id");
    });
});
