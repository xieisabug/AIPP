import { describe, expect, it } from "vitest";
import { isSimpleVaultValue } from "./vaultHelpers";

describe("isSimpleVaultValue", () => {
    it.each(["", " ", "\t\n  "])("should not warn for an empty or whitespace-only value (%j)", (value) => {
        expect(isSimpleVaultValue(value)).toBe(false);
    });

    // Cover both sides of the backend's 4-byte literal-redaction minimum. The warning is a
    // conservative UI hint, not permission to skip redaction or validation at either boundary.
    it.each(["x", "abc", "text", "body", "true", "false", "1234", "password", "12345678", "密码", "🔑🔒", "       text       "])(
        "should warn for a short value that may occur in ordinary content (%j)", (value) => {
            expect(isSimpleVaultValue(value)).toBe(true);
        },
    );

    it.each(["a".repeat(24), "12".repeat(12), "abc".repeat(8), "abcd".repeat(6), "密钥".repeat(8), "ab\nc".repeat(5)])(
        "should warn for long repetitions of a small pattern (%j)", (value) => {
            expect(isSimpleVaultValue(value)).toBe(true);
        },
    );

    it.each(["abcDEF12!", "vault-token_d7Q9z!K2", "  vault-token_d7Q9z!K2  ", "不同文字组成的较长值", "abcdabcdX"])(
        "should not warn for longer nonrepeating values (%j)", (value) => {
            expect(isSimpleVaultValue(value)).toBe(false);
        },
    );
});
