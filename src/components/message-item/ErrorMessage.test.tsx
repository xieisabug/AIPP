import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ErrorMessage, { extractRootCause } from "./ErrorMessage";

const platformState = vi.hoisted(() => ({ mobile: false }));
vi.mock("@/hooks/use-platform", () => ({
    useIsMobilePlatform: () => platformState.mobile,
}));

const networkError = JSON.stringify({
    message: "AI请求失败: 网络连接异常，请检查网络设置",
    details: null,
    model: "mock-rich",
    phase: "stream",
    attempts: 3,
    original_error:
        "Stream processing failed: Web stream error for model 'mock-rich (adapter: OpenAI)'.\nCause: error sending request for url (http://10.0.2.2:18080/v1/chat/completions)",
});

describe("ErrorMessage", () => {
    it("extracts the Cause line from a genai error", () => {
        expect(extractRootCause("a\nCause: boom\nmore")).toBe("boom");
        expect(extractRootCause("only line")).toBe("only line");
    });

    it("shows the real cause when there is no response body", () => {
        render(<ErrorMessage content={networkError} messageId={1} />);
        expect(screen.getByText(/error sending request for url/)).toBeTruthy();
        expect(screen.getByText("查看详情（响应体/原始错误）")).toBeTruthy();
    });

    it("uses full width on mobile and the desktop width otherwise", () => {
        platformState.mobile = true;
        const { container, unmount } = render(<ErrorMessage content={networkError} messageId={1} />);
        expect((container.firstChild as HTMLElement).className).toContain("w-full max-w-full");
        unmount();
        platformState.mobile = false;
        const desktop = render(<ErrorMessage content={networkError} messageId={1} />);
        expect((desktop.container.firstChild as HTMLElement).className).toContain("w-fit max-w-[min(65%,56rem)]");
    });

    it.each([
        ["desktop", false],
        ["mobile", true],
    ])("keeps light colors and adds readable dark-theme colors on %s", (_label, mobile) => {
        platformState.mobile = mobile;
        document.documentElement.classList.toggle("aipp-mobile-platform", mobile);
        document.documentElement.classList.add("dark");
        try {
            const { container } = render(<ErrorMessage content={networkError} messageId={1} />);
            const card = container.firstChild as HTMLElement;
            // 浅色主题保持原样
            expect(card.className).toContain("bg-red-50");
            expect(card.className).toContain("border-red-200");
            // 深色主题：深红底 + 浅红字，不再是白字压粉底
            expect(card.className).toContain("dark:bg-red-950/40");
            expect(card.className).toContain("dark:text-red-200");
            const title = container.querySelector('[data-aipp-slot="error-title"]') as HTMLElement;
            expect(title.textContent).toBe("AI Request Failed");
            expect(title.className).toContain("text-red-800");
            expect(title.className).toContain("dark:text-red-200");
            const rootCause = container.querySelector('[data-aipp-slot="error-root-cause"]') as HTMLElement;
            expect(rootCause.className).toContain("dark:text-red-300");
            // 主文案走 .prose（text-foreground），配合深色卡片底可读
            const main = container.querySelector('[data-aipp-slot="error-main-message"]') as HTMLElement;
            expect(main.className).toContain("prose");
            // 操作条在深色下不再是粉色底
            const actionBarClass = mobile ? ".aipp-mobile-action-bar" : ".group-hover\\:flex";
            const actionBar = container.querySelector(actionBarClass) as HTMLElement;
            expect(actionBar.className).toContain("dark:bg-red-950");
        } finally {
            document.documentElement.classList.remove("dark", "aipp-mobile-platform");
            platformState.mobile = false;
        }
    });
});
