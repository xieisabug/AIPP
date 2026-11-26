import { describe, it, expect } from "vitest";
import { parseProviderShareMessage, providerShareMessage } from "./providerShareMessage";
const code = btoa(JSON.stringify({ version: "1.0", type: "provider", encrypted_data: "abc", nonce: "abc" }));
describe("provider share messages", () => {
    it("should read a forwarded message and its original password", () => {
        expect(parseProviderShareMessage(providerShareMessage("示例", code, "p: X"))).toEqual({
            code,
            password: "p: X",
        });
        expect(parseProviderShareMessage(code + "\nPassword:  a  ")).toEqual({ code, password: "a" });
        expect(parseProviderShareMessage(`分享码：${code} 密码：inline-pass`)).toEqual({
            code,
            password: "inline-pass",
        });
    });
    it("should accept a bare code and omit a separately shared password", () => {
        expect(parseProviderShareMessage(code)?.code).toBe(code);
        expect(parseProviderShareMessage(providerShareMessage("示例", code))).toEqual({
            code,
            password: undefined,
        });
    });
    it("should reject unrelated envelopes and truncated data", () => {
        expect(
            parseProviderShareMessage(btoa(JSON.stringify({ type: "assistant", payload: "x".repeat(60) })))
        ).toBeNull();
        expect(parseProviderShareMessage(code.slice(0, -8))).toBeNull();
    });
});
