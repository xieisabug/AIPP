import { describe, expect, it } from "vitest";
import { isAssistantFormDirty } from "./assistantFormDirty";

describe("isAssistantFormDirty", () => {
    it("should stay clean when a field registered later only adds an undefined key", () => {
        // RHF Controller registration for a name missing from reset() leaves `mcp_config: undefined`.
        expect(isAssistantFormDirty({ prompt: "hi", mcp_config: undefined }, { prompt: "hi" })).toBe(false);
    });

    it("should treat null, undefined and empty string as equal", () => {
        expect(isAssistantFormDirty({ a: null, b: "" }, { a: "", b: undefined })).toBe(false);
    });

    it("should compare scalars by their stored string form", () => {
        expect(isAssistantFormDirty({ temperature: 0.7, stream: true }, { temperature: "0.7", stream: "true" })).toBe(false);
    });

    it("should report dirty when a value really changed", () => {
        expect(isAssistantFormDirty({ prompt: "new" }, { prompt: "old" })).toBe(true);
        expect(isAssistantFormDirty({ stream: false }, { stream: true })).toBe(true);
    });

    it("should report dirty when a field gains a value the baseline lacks", () => {
        expect(isAssistantFormDirty({ prompt: "hi", reasoning_effort: "high" }, { prompt: "hi" })).toBe(true);
    });
});
