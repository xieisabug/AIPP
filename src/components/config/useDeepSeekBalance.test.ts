import { describe, expect, it } from "vitest";
import { describeBalance, formatBalance } from "./useDeepSeekBalance";

describe("DeepSeek balance formatting", () => {
    it("formats known currencies with a symbol and keeps the API's precision", () => {
        expect(formatBalance({ available: true, currency: "CNY", total: "19.00" })).toBe("¥19.00");
        expect(formatBalance({ available: true, currency: "USD", total: "1.5" })).toBe("$1.5");
        expect(formatBalance({ available: true, currency: "EUR", total: "2.00" })).toBe("2.00 EUR");
    });

    it("explains an unusable balance and query states", () => {
        expect(formatBalance({ available: false, currency: "CNY", total: "0.00" })).toBe("¥0.00（余额不足，暂不可调用）");
        expect(describeBalance({ status: "idle" })).toBe("");
        expect(describeBalance({ status: "loading" })).toBe("查询中…");
        expect(describeBalance({ status: "error", message: "API Key 无效或已吊销" })).toBe("查询失败：API Key 无效或已吊销");
    });
});
