import { describe, expect, it } from "vitest";

import { resolveAgentDefaultSelection } from "./agentDefaults";

const models = [
    {
        code: "gpt-first%%7",
        name: "First",
        provider_id: 7,
        efforts: ["low", "medium"],
        default_effort: "low",
        is_default: false,
    },
    {
        code: "gpt-configured%%7",
        name: "Configured",
        provider_id: 7,
        efforts: ["medium", "high"],
        default_effort: "high",
        is_default: true,
    },
];

describe("resolveAgentDefaultSelection", () => {
    it("uses the Codex effective default instead of the first model", () => {
        const selection = resolveAgentDefaultSelection(models);

        expect(selection.model?.code).toBe("gpt-configured%%7");
        expect(selection.effort).toBe("high");
    });

    it("uses an explicit effort when the selected model has no authoritative default", () => {
        const selection = resolveAgentDefaultSelection(
            [{ ...models[0], default_effort: null }, models[1]],
            "gpt-first",
            "medium",
        );

        expect(selection.model?.code).toBe("gpt-first%%7");
        expect(selection.effort).toBe("medium");
    });
});
