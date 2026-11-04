import { describe, expect, it } from "vitest";
import aiApiRustSource from "../../src-tauri/src/api/ai_api.rs?raw";
import { describeAskAiError, NO_MODEL_CONFIGURED_MESSAGE } from "./askAiError";

describe("describeAskAiError", () => {
    it("maps the NoModelFound unit variant to a readable message", () => {
        expect(describeAskAiError("NoModelFound")).toBe(NO_MODEL_CONFIGURED_MESSAGE);
    });

    it("unwraps serialized AppError variants instead of printing [object Object]", () => {
        expect(describeAskAiError({ UnknownError: "boom" })).toBe("boom");
        expect(describeAskAiError(new Error("x"))).toBe("x");
    });
});

describe("NO_MODEL_CONFIGURED_MESSAGE", () => {
    it("matches the backend constant so both sides show the same text", () => {
        expect(aiApiRustSource).toContain(`"${NO_MODEL_CONFIGURED_MESSAGE}"`);
    });
});
