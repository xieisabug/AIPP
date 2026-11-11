import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ErrorMessage, { extractRootCause, summarizeError } from "./ErrorMessage";

const platformState = vi.hoisted(() => ({ mobile: false }));
vi.mock("@/hooks/use-platform", () => ({
    useIsMobilePlatform: () => platformState.mobile,
}));

const toggle = () => fireEvent.click(screen.getByRole("button", { name: /查看详情$|查看错误$|收起$/ }));
const details = (container: HTMLElement) => container.querySelector('[data-aipp-slot="error-details"]') as HTMLElement | null;

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

    it("should name the server-side reason when the friendly message is generic", () => {
        const body = JSON.stringify({ error: { message: "upstream error: do request failed", code: "do_request_failed" } });
        expect(summarizeError("AI请求失败: 请求失败，请稍后重试", body, null))
            .toEqual({ summary: "请求失败，请稍后重试", reason: "upstream error: do request failed" });
        expect(summarizeError("助手尚未配置模型", null, null)).toEqual({ summary: "助手尚未配置模型", reason: null });
    });

    it("should fold into one line with the specific cause and expand details under the same line", () => {
        const { container } = render(<ErrorMessage content={networkError} messageId={1} />);
        expect(container.querySelector('[data-aipp-slot="error-summary"]')?.textContent).toBe("网络连接异常，请检查网络设置");
        expect(container.querySelector('[data-aipp-slot="error-reason"]')?.textContent).toContain("error sending request for url");
        expect(details(container)).toBeNull();

        toggle();
        const header = screen.getByRole("button", { name: /收起$/ });
        expect(header).toHaveAttribute("aria-expanded", "true");
        // 展开后标题行还是同一行文字，只是操作提示变成“收起”
        expect(header.querySelector('[data-aipp-slot="error-summary"]')?.textContent).toBe("网络连接异常，请检查网络设置");
        expect(details(container)?.textContent).toContain("mock-rich");
        expect(details(container)?.textContent).toContain("流式请求");
        fireEvent.click(screen.getByText("查看详情（响应体/原始错误）"));
        expect(screen.getByText(/Stream processing failed/)).toBeInTheDocument();

        toggle();
        expect(details(container)).toBeNull();
    });

    it("uses full width on mobile and the desktop width otherwise", () => {
        platformState.mobile = true;
        const { container, unmount } = render(<ErrorMessage content={networkError} messageId={1} />);
        toggle();
        expect(details(container)!.className).toContain("w-full max-w-full");
        unmount();
        platformState.mobile = false;
        const desktop = render(<ErrorMessage content={networkError} messageId={1} />);
        toggle();
        expect(details(desktop.container)!.className).toContain("max-w-[min(65%,56rem)]");
    });

    it.each([
        ["desktop", false],
        ["mobile", true],
    ])("should keep the expanded view in the folded line's neutral style on %s", (_label, mobile) => {
        platformState.mobile = mobile;
        try {
            const { container } = render(<ErrorMessage content={networkError} messageId={1} />);
            toggle();
            fireEvent.click(screen.getByText("查看详情（响应体/原始错误）"));
            // 不再切换成红底红框的大卡片：只有图标带红色，正文走主题色，深浅色自动适配
            const tinted = Array.from(container.querySelectorAll<HTMLElement>("[class]"))
                .filter((element) => /\b(bg|border)-red-/.test(element.getAttribute("class") ?? ""));
            expect(tinted).toEqual([]);
            expect(details(container)!.className).toContain("text-muted-foreground");
            expect(details(container)!.className).toContain("border-border");
            expect(screen.getByRole("button", { name: "复制错误" })).toBeInTheDocument();
        } finally {
            platformState.mobile = false;
        }
    });
});

it("should fold the previous error when a new reply supersedes it and allow inspection", () => {
    const { rerender } = render(<ErrorMessage content="助手尚未配置模型" messageId={9} />);
    expect(screen.getByRole("button", { name: "回复失败：助手尚未配置模型 · 查看详情" })).toBeVisible();
    rerender(<ErrorMessage content="助手尚未配置模型" messageId={9} superseded />);
    expect(screen.queryByText("助手尚未配置模型")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "此前请求失败 · 查看错误" }));
    expect(screen.getByText("助手尚未配置模型")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "此前请求失败 · 收起" }));
    expect(screen.queryByText("助手尚未配置模型")).not.toBeInTheDocument();
});
