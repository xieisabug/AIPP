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
        }));
    });
});
