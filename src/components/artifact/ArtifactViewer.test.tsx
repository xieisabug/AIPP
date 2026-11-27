import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import ArtifactViewer from "./ArtifactViewer";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("@tauri-apps/plugin-fs", () => ({ writeFile: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn() }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("@/hooks/useArtifactBridge", () => ({ useArtifactBridge: () => ({ sendConfig: vi.fn(), isConnected: false }) }));
vi.mock("@/components/ArtifactPreviewCodeBlock", () => ({
    default: ({ children }: { children: ReactNode }) => <pre data-testid="code">{children}</pre>,
}));
vi.mock("./MermaidPreview", () => ({ default: () => <div data-testid="mermaid" /> }));
vi.mock("./MarkdownPreview", () => ({ default: ({ code }: { code: string }) => <div data-testid="markdown">{code}</div> }));
vi.mock("./DrawioPreview", () => ({ default: () => <div data-testid="drawio" /> }));
vi.mock("@/components/ui/tooltip", () => ({
    Tooltip: ({ children }: { children: ReactNode }) => <>{children}</>,
    TooltipTrigger: ({ children }: { children: ReactNode }) => <>{children}</>,
    TooltipContent: () => null,
    TooltipProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

function postFromFrame(frame: HTMLIFrameElement | null, level: string, message: string) {
    act(() => {
        window.dispatchEvent(
            new MessageEvent("message", {
                data: { __aippArtifactConsole: 1, entry: { level, message, time: 1 } },
                source: frame?.contentWindow ?? null,
            }),
        );
    });
}

describe("ArtifactViewer", () => {
    beforeEach(() => {
        vi.clearAllMocks();
    });
    afterEach(() => {
        vi.restoreAllMocks();
    });

    it("shows a loading state while the React dev server is starting", () => {
        render(
            <ArtifactViewer
                type="react"
                code="export default function A(){return null}"
                previewUrl={null}
                contentKey={1}
                buildLogs={[{ type: "log", message: "安装/更新依赖" }]}
            />,
        );
        expect(screen.getByText("安装/更新依赖")).toBeTruthy();
        expect(document.querySelector("iframe")).toBeNull();
    });

    it("collects runtime errors only from its own iframe and hands them to the AI", async () => {
        const onFixWithAI = vi.fn().mockResolvedValue(true);
        const { container } = render(
            <ArtifactViewer
                type="html"
                code="<p>hi</p>"
                contentKey={1}
                title="卡片"
                buildLogs={[]}
                onFixWithAI={onFixWithAI}
            />,
        );
        const frame = container.querySelector("iframe");
        expect(frame?.getAttribute("srcdoc")).toContain("__aippConsoleInstalled");

        // 其他来源的消息不计入
        postFromFrame(null, "error", "foreign error");
        expect(screen.queryByText(/页面运行出现/)).toBeNull();

        postFromFrame(frame, "error", "ReferenceError: foo is not defined");
        expect(screen.getByText("页面运行出现 1 个错误")).toBeTruthy();

        await act(async () => {
            fireEvent.click(screen.getByRole("button", { name: /让 AI 修复/ }));
        });
        expect(onFixWithAI).toHaveBeenCalledTimes(1);
        const report = onFixWithAI.mock.calls[0][0] as string;
        expect(report).toContain("「卡片」");
        expect(report).toContain("ReferenceError: foo is not defined");
        expect(report).not.toContain("foreign error");
    });

    it("clears runtime output when new content arrives", () => {
        const { container, rerender } = render(
            <ArtifactViewer type="html" code="<p>a</p>" contentKey={1} buildLogs={[]} />,
        );
        postFromFrame(container.querySelector("iframe"), "error", "old error");
        expect(screen.getByText("页面运行出现 1 个错误")).toBeTruthy();

        rerender(<ArtifactViewer type="html" code="<p>b</p>" contentKey={2} buildLogs={[]} />);
        expect(screen.queryByText(/页面运行出现/)).toBeNull();
    });

    it("hides the fix action when no conversation can receive it", () => {
        const { container } = render(<ArtifactViewer type="html" code="<p>a</p>" contentKey={1} buildLogs={[]} />);
        postFromFrame(container.querySelector("iframe"), "error", "boom");
        expect(screen.queryByRole("button", { name: /让 AI 修复/ })).toBeNull();
    });

    it("shows the version switcher only when there is history and marks old versions", () => {
        const versions = [
            { version: 3, created_at: "2026-11-27T10:00:00Z" },
            { version: 2, created_at: "2026-11-27T09:00:00Z" },
        ];
        const { rerender } = render(
            <ArtifactViewer type="html" code="<p>a</p>" contentKey={1} buildLogs={[]} versions={versions.slice(0, 1)} currentVersion={3} onSelectVersion={vi.fn()} />,
        );
        expect(screen.queryByRole("button", { name: "切换版本" })).toBeNull();

        rerender(
            <ArtifactViewer type="html" code="<p>a</p>" contentKey={1} buildLogs={[]} versions={versions} currentVersion={2} onSelectVersion={vi.fn()} />,
        );
        const trigger = screen.getByRole("button", { name: "切换版本" });
        expect(trigger.textContent).toContain("v2");
        expect(trigger.textContent).toContain("旧版");
    });

    it("waits for project files before rendering a multi-file version", () => {
        const { container, rerender } = render(
            <ArtifactViewer type="project" code="import B from './B'" contentKey={1} buildLogs={[]} entryFile="App.tsx" projectFiles={null} />,
        );
        expect(container.querySelector("iframe")).toBeNull();
        expect(screen.getByText("正在启动预览…")).toBeTruthy();

        const encode = (text: string) => btoa(text);
        rerender(
            <ArtifactViewer
                type="project"
                code="import B from './B'"
                contentKey={1}
                buildLogs={[]}
                entryFile="App.tsx"
                projectFiles={{ "App.tsx": encode("import B from './B'"), "B.tsx": encode("export default 1") }}
            />,
        );
        const frame = container.querySelector("iframe");
        expect(frame?.getAttribute("sandbox")).toBe("allow-scripts");
        expect(frame?.getAttribute("srcdoc")).toContain("__aippConsoleInstalled");
    });

    it("uses the override instead of the default preview", () => {
        render(
            <ArtifactViewer
                type="html"
                code="AIPP_CHOICE_PREVIEW_V1\n{}"
                contentKey={1}
                buildLogs={[]}
                previewOverride={<div data-testid="override" />}
            />,
        );
        expect(screen.getByTestId("override")).toBeTruthy();
        expect(document.querySelector("iframe")).toBeNull();
    });
});
