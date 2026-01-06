/**
 * 工具函数测试
 *
 * 验证 src/lib/utils.ts 中的工具函数
 */
import { describe, it, expect } from "vitest";
import { cn } from "./utils";

describe("cn (className 合并工具)", () => {
    it("应该合并多个 class 名", () => {
        const result = cn("class1", "class2");
        expect(result).toBe("class1 class2");
    });

    it("应该处理条件 class", () => {
        const isActive = true;
        const result = cn("base", isActive && "active");
        expect(result).toContain("base");
        expect(result).toContain("active");
    });

    it("应该过滤 falsy 值", () => {
        const result = cn("base", false, null, undefined, "valid");
        expect(result).toBe("base valid");
    });

    it("应该正确合并 Tailwind 冲突的 class", () => {
        // tailwind-merge 应该保留后面的 class
        const result = cn("px-2", "px-4");
        expect(result).toBe("px-4");
    });

    it("应该处理空输入", () => {
        const result = cn();
        expect(result).toBe("");
    });

    it("应该处理对象形式的条件 class", () => {
        const result = cn({
            base: true,
            active: true,
            disabled: false,
        });
        expect(result).toContain("base");
        expect(result).toContain("active");
        expect(result).not.toContain("disabled");
    });
});
