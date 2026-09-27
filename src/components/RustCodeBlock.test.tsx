import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import RustCodeBlock from "./RustCodeBlock";

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
    beforeEach(() => {
        vi.stubGlobal("ResizeObserver", class {
            constructor(callback: ResizeObserverCallback) {
                notifyResize = (height) => callback([
                    { borderBoxSize: [{ blockSize: height }] } as unknown as ResizeObserverEntry,
                ], this as unknown as ResizeObserver);
            }
            observe() {}
            disconnect() {}
        });
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
});
