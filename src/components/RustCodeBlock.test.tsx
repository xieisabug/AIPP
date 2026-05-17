import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
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
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
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
});
