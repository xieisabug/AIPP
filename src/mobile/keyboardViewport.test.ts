import { describe, expect, it } from "vitest";
import { isKeyboardCompact, mobileTextareaMaxHeight } from "./keyboardViewport";

describe("keyboard viewport", () => {
    it("keeps a tall portrait keyboard in the normal composer", () => {
        expect(isKeyboardCompact(810, 443)).toBe(false);
        expect(mobileTextareaMaxHeight(810, 443)).toBeGreaterThan(44);
    });

    it("locks the composer to one line when the landscape keyboard leaves little height", () => {
        expect(isKeyboardCompact(384, 148)).toBe(true);
        expect(mobileTextareaMaxHeight(384, 148)).toBe(44);
    });

    it("does not compact when the keyboard is closed", () => {
        expect(isKeyboardCompact(384, 384)).toBe(false);
    });
});
