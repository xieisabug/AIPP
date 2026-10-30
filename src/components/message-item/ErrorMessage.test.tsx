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
});
