import { describe, expect, it } from "vitest";
import { butlerTaskStatusLabel } from "./Butler";

describe("butlerTaskStatusLabel", () => {
    it("maps completed/cancelled/interrupted distinctly", () => {
        expect(butlerTaskStatusLabel("succeeded")).toBe("已完成");
        expect(butlerTaskStatusLabel("completed")).toBe("已完成");
        expect(butlerTaskStatusLabel("cancelled")).toBe("已取消");
        expect(butlerTaskStatusLabel("interrupted")).toBe("已中断");
        expect(butlerTaskStatusLabel("failed")).toBe("失败");
    });
});
