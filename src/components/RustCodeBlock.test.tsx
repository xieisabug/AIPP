import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import RustCodeBlock from "./RustCodeBlock";

const platform = vi.hoisted(() => ({ mobile: false }));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => platform.mobile }));
vi.mock("@/lib/mobileUnsupported", () => ({ useFeatureAvailableOnPlatform: () => !platform.mobile }));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
    writeText: vi.fn(),
}));

vi.mock("@/hooks/useTheme", () => ({
    useTheme: () => ({
        resolvedTheme: "light",
    }),
}));

vi.mock("@/hooks/useCodeTheme", () => ({
    useCodeTheme: () => ({
        currentTheme: "github",
    }),
}));

describe("RustCodeBlock", () => {
    let notifyResize: (height: number) => void;
    let notifyIntersection: (entries: Array<[Element, boolean]>) => void;
    beforeEach(() => {
        platform.mobile = false;
        vi.stubGlobal("ResizeObserver", class {
            constructor(callback: ResizeObserverCallback) {
                notifyResize = (height) => callback([
                    { borderBoxSize: [{ blockSize: height }] } as unknown as ResizeObserverEntry,
                ], this as unknown as ResizeObserver);
            }
            observe() {}
            disconnect() {}
        });
        vi.stubGlobal("IntersectionObserver", class {
            constructor(callback: IntersectionObserverCallback) {
                notifyIntersection = entries => callback(
                    entries.map(([target, isIntersecting]) => ({ target, isIntersecting }) as IntersectionObserverEntry),
                    this as unknown as IntersectionObserver,
                );
            }
            observe() {}
            disconnect() {}
            unobserve() {}
        });
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => { cb(0); return 0; });
    });

    it("should float controls only when the code block has scrolled past its toolbar", () => {
        const { container } = render(<RustCodeBlock language="text">toolbar position</RustCodeBlock>);
        const root = container.firstElementChild as HTMLElement;
        const sentinel = root.firstElementChild as HTMLElement;
        let bounds = { top: 900, bottom: 1100 };
        vi.spyOn(root, "getBoundingClientRect").mockImplementation(() => bounds as DOMRect);
        const toolbar = screen.getByRole("button", { name: "复制代码" }).parentElement!;
        act(() => notifyIntersection([[sentinel, false], [root, false]]));
        expect(toolbar).not.toHaveClass("fixed");
        bounds = { top: -100, bottom: 400 };
        act(() => notifyIntersection([[sentinel, false], [root, true]]));
        expect(toolbar).toHaveClass("fixed");
        // 真实浏览器中 sentinel 已不在视口，不会再回调；只能靠滚动重新判断。
        bounds = { top: -400, bottom: 100 };
        act(() => { window.dispatchEvent(new Event("scroll")); });
        expect(toolbar).not.toHaveClass("fixed");
        bounds = { top: -100, bottom: 400 };
        act(() => { window.dispatchEvent(new Event("scroll")); });
        expect(toolbar).toHaveClass("fixed");
        // 整个代码块离开视口时取消固定。
        bounds = { top: -800, bottom: -300 };
        act(() => notifyIntersection([[root, false]]));
        expect(toolbar).not.toHaveClass("fixed");
    });

    it("should open the in-app preview with full source when the platform is mobile", async () => {
        platform.mobile = true;
        const preview = vi.fn();
        window.addEventListener("aipp:mobile-preview", preview);
        try {
            render(<RustCodeBlock language="text">mobile source</RustCodeBlock>);
            const button = screen.getByRole("button", { name: "预览制品" });
            expect(button).toHaveClass("min-h-11", "min-w-11");
            expect(screen.getByRole("button", { name: "复制代码" })).toHaveClass("min-h-11");
            await userEvent.click(button);
            expect(preview).toHaveBeenCalledOnce();
            expect((preview.mock.calls[0][0] as CustomEvent).detail).toEqual({ language: "text", code: "mobile source" });
        } finally {
            window.removeEventListener("aipp:mobile-preview", preview);
        }
    });
    afterEach(() => {
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("should measure asynchronously and preserve manual expansion when code resizes", async () => {
        const readHeight = vi.spyOn(HTMLElement.prototype, "scrollHeight", "get");
        render(<RustCodeBlock language="text">short code</RustCodeBlock>);
        expect(readHeight).not.toHaveBeenCalled();
        act(() => notifyResize(400));
        expect(screen.getByRole("button", { name: "展开代码" })).toBeInTheDocument();
        await userEvent.click(screen.getByRole("button", { name: "展开代码" }));
        act(() => notifyResize(500));
        expect(screen.getByRole("button", { name: "收起代码" })).toBeInTheDocument();
        expect(readHeight).not.toHaveBeenCalled();
    });

    it("highlights only a preview while collapsed and highlights the full code after expand", async () => {
        const longCode = Array.from({ length: 180 }, (_, index) => {
            return index === 170 ? "const FULL_MARKER = true;" : `const line${index} = ${index};`;
        }).join("\n");
        const highlightedInputs: string[] = [];

        mockInvokeHandler("highlight_code", (args) => {
            const code = String(args?.code ?? "");
            highlightedInputs.push(code);
            return `<pre><code>${code}</code></pre>`;
        });

        render(
            <RustCodeBlock language="ts">
                {longCode}
            </RustCodeBlock>
        );

        await waitFor(() => {
            expect(highlightedInputs.length).toBeGreaterThan(0);
        });
        expect(highlightedInputs[0]).not.toContain("FULL_MARKER");
        expect(highlightedInputs[0].split("\n")).toHaveLength(120);

        await userEvent.click(screen.getByRole("button", { name: "展开代码" }));

        await waitFor(() => {
            expect(highlightedInputs.some((input) => input.includes("FULL_MARKER"))).toBe(true);
        });
        expect(highlightedInputs[highlightedInputs.length - 1]).toBe(longCode);
    });

    it("renders plain text code blocks without invoking syntax highlighting", async () => {
        mockInvokeHandler("highlight_code", () => "<pre><code>highlighted</code></pre>");

        render(
            <RustCodeBlock language="text">
                {"ascii box\n+-- demo --+"}
            </RustCodeBlock>
        );

        expect(screen.getByText(/ascii box/)).toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith("highlight_code", expect.anything());
    });

    it("should reuse highlighted markup on the first render when a code block remounts", async () => {
        mockInvokeHandler("highlight_code", () => '<pre><code><span data-testid="cached-highlight">cached remount</span></code></pre>');
        const first = render(<RustCodeBlock language="ts">cached remount</RustCodeBlock>);
        await screen.findByTestId("cached-highlight");
        first.unmount();
        render(<RustCodeBlock language="ts">cached remount</RustCodeBlock>);
        expect(screen.getByTestId("cached-highlight")).toBeInTheDocument();
    });

    it("keeps text selection after pointer leaves the code block", () => {
        render(
            <RustCodeBlock language="text">
                {"alpha beta gamma"}
            </RustCodeBlock>
        );

        const codeNode = screen.getByText(/alpha beta gamma/);
        const range = document.createRange();
        range.selectNodeContents(codeNode);
        const selection = window.getSelection();
        selection?.removeAllRanges();
        selection?.addRange(range);
        expect(selection?.toString()).toContain("beta");

        const root = codeNode.closest(".group\\/codeblock");
        expect(root).not.toBeNull();
        root!.dispatchEvent(new MouseEvent("mouseleave", { bubbles: true }));
        expect(window.getSelection()?.toString()).toContain("beta");
    });

});
