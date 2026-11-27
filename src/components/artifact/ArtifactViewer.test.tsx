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

    it("should fill an element edit request into the conversation after picking an element", async () => {
        const onFixWithAI = vi.fn().mockResolvedValue(true);
        const { container } = render(
            <ArtifactViewer
                type="html"
                code="<button>开始</button>"
                contentKey={1}
                title="番茄钟"
                buildLogs={[]}
                versions={[{ version: 2, created_at: "2026-11-27T10:00:00Z" }]}
                currentVersion={2}
                onFixWithAI={onFixWithAI}
            />,
        );
        const frame = container.querySelector("iframe")!;
        const postToFrame = vi.spyOn(frame.contentWindow!, "postMessage");

        fireEvent.click(screen.getByRole("button", { name: "选择元素修改" }));
        expect(postToFrame).toHaveBeenCalledWith({ __aippArtifactPick: "start" }, "*");
        expect(screen.getByText(/点击页面中想修改的元素/)).toBeTruthy();

        // 其他来源的点选结果不处理
        const picked = { __aippArtifactPick: 1, state: "picked", element: { tag: "button", selector: "body > button", text: "开始", html: "<button>开始</button>" } };
        act(() => {
            window.dispatchEvent(new MessageEvent("message", { data: picked, source: null }));
        });
        expect(screen.queryByLabelText("修改要求")).toBeNull();

        act(() => {
            window.dispatchEvent(new MessageEvent("message", { data: picked, source: frame.contentWindow }));
        });
        expect(screen.queryByText(/点击页面中想修改的元素/)).toBeNull();
        expect(screen.getByText("body > button")).toBeTruthy();

        const submit = screen.getByRole("button", { name: "填入会话" }) as HTMLButtonElement;
        expect(submit.disabled).toBe(true);
        fireEvent.change(screen.getByLabelText("修改要求"), { target: { value: "改成圆角" } });
        await act(async () => {
            fireEvent.click(submit);
        });

        expect(onFixWithAI).toHaveBeenCalledTimes(1);
        const request = onFixWithAI.mock.calls[0][0] as string;
        expect(request).toContain("「番茄钟」（HTML） v2里的这个元素：改成圆角");
        expect(request).toContain("`body > button`");
        expect(screen.queryByLabelText("修改要求")).toBeNull();
    });

    it("should attach the element screenshot captured in the background", async () => {
        const onFixWithAI = vi.fn().mockResolvedValue(true);
        const onCaptureElement = vi.fn().mockResolvedValue("C:/tmp/element.png");
        const { container } = render(
            <ArtifactViewer
                type="html"
                code="<button>开始</button>"
                contentKey={1}
                buildLogs={[]}
                onFixWithAI={onFixWithAI}
                onCaptureElement={onCaptureElement}
            />,
        );
        const frame = container.querySelector("iframe")!;
        fireEvent.click(screen.getByRole("button", { name: "选择元素修改" }));
        await act(async () => {
            window.dispatchEvent(
                new MessageEvent("message", {
                    data: { __aippArtifactPick: 1, state: "picked", element: { tag: "button", selector: "body > button", text: "开始", html: "" } },
                    source: frame.contentWindow,
                }),
            );
        });
        expect(onCaptureElement).toHaveBeenCalledWith("body > button", expect.objectContaining({ width: expect.any(Number) }));
        expect(await screen.findByText(/已截取元素截图/)).toBeTruthy();

        fireEvent.change(screen.getByLabelText("修改要求"), { target: { value: "改成圆角" } });
        await act(async () => {
            fireEvent.click(screen.getByRole("button", { name: "填入会话" }));
        });
        expect(onFixWithAI).toHaveBeenCalledWith(expect.stringContaining("截图：见附件"), ["C:/tmp/element.png"]);
    });

    it("should leave pick mode on Escape while focus is still in the host page", () => {
        const { container } = render(
            <ArtifactViewer type="html" code="<p>a</p>" contentKey={1} buildLogs={[]} onFixWithAI={vi.fn()} />,
        );
        const postToFrame = vi.spyOn(container.querySelector("iframe")!.contentWindow!, "postMessage");
        fireEvent.click(screen.getByRole("button", { name: "选择元素修改" }));
        expect(screen.getByText(/点击页面中想修改的元素/)).toBeTruthy();

        fireEvent.keyDown(window, { key: "Escape" });

        expect(screen.queryByText(/点击页面中想修改的元素/)).toBeNull();
        expect(postToFrame).toHaveBeenLastCalledWith({ __aippArtifactPick: "stop" }, "*");
    });

    it("should not offer element picking without a receiving conversation or for static types", () => {
        const { rerender } = render(<ArtifactViewer type="html" code="<p>a</p>" contentKey={1} buildLogs={[]} />);
        expect(screen.queryByRole("button", { name: "选择元素修改" })).toBeNull();
        rerender(<ArtifactViewer type="markdown" code="# a" contentKey={1} buildLogs={[]} onFixWithAI={vi.fn()} />);
        expect(screen.queryByRole("button", { name: "选择元素修改" })).toBeNull();
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

    it("should offer a Bun build for multi-file versions with package.json and preview the build output", () => {
        const files = { "index.html": btoa('<script type="module" src="./main.js"></script>'), "main.js": btoa("1"), "package.json": btoa("{}") };
        const build = vi.fn().mockResolvedValue(undefined);
        const props = { type: "project" as const, code: "", contentKey: 1, buildLogs: [], entryFile: "index.html", projectFiles: files };
        const { container, rerender } = render(
            <ArtifactViewer {...props} projectBuild={{ url: null, building: false, error: "", build }} />,
        );
        fireEvent.change(screen.getByLabelText("构建输出目录"), { target: { value: "build" } });
        fireEvent.click(screen.getByRole("button", { name: "用 Bun 安装依赖并构建" }));
        expect(build).toHaveBeenCalledWith("build");

        rerender(<ArtifactViewer {...props} projectBuild={{ url: "http://t.aipp-choice.localhost:4000/", building: false, error: "", build }} />);
        expect(screen.queryByRole("button", { name: "用 Bun 安装依赖并构建" })).toBeNull();
        const frame = container.querySelector('[data-aipp-slot="artifact-viewer-build-frame"]');
        expect(frame?.getAttribute("src")).toBe("http://t.aipp-choice.localhost:4000/");
    });

    it("should not offer a build without package.json", () => {
        render(
            <ArtifactViewer
                type="project"
                code=""
                contentKey={1}
                buildLogs={[]}
                entryFile="index.html"
                projectFiles={{ "index.html": btoa("<p>a</p>") }}
                projectBuild={{ url: null, building: false, error: "", build: vi.fn() }}
            />,
        );
        expect(screen.queryByRole("button", { name: "用 Bun 安装依赖并构建" })).toBeNull();
    });

    it("should compare the code with the previous version", async () => {
        const loadVersionFiles = vi.fn().mockResolvedValue({ entryFile: "index.html", files: { "index.html": btoa("<p>old</p>\n<p>same</p>") } });
        render(
            <ArtifactViewer
                type="html"
                code={"<p>new</p>\n<p>same</p>"}
                contentKey={1}
                buildLogs={[]}
                versions={[
                    { version: 3, created_at: "2026-11-27T10:00:00Z" },
                    { version: 2, created_at: "2026-11-27T09:00:00Z" },
                ]}
                currentVersion={3}
                onSelectVersion={vi.fn()}
                loadVersionFiles={loadVersionFiles}
            />,
        );
        fireEvent.mouseDown(screen.getByRole("tab", { name: "代码" }));
        fireEvent.click(screen.getByRole("tab", { name: "代码" }));
        await act(async () => {
            fireEvent.click(screen.getByRole("button", { name: "对比 v2" }));
        });
        expect(loadVersionFiles).toHaveBeenCalledWith(2);
        expect(screen.getByText("相比 v2：+1 行，-1 行")).toBeTruthy();
        const diff = document.querySelector('[data-aipp-slot="artifact-viewer-diff"]')!;
        expect(diff.textContent).toContain("<p>old</p>");
        expect(diff.textContent).toContain("<p>new</p>");
    });

    it("should not offer comparison on the oldest version", () => {
        render(
            <ArtifactViewer
                type="html"
                code="<p>a</p>"
                contentKey={1}
                buildLogs={[]}
                versions={[
                    { version: 2, created_at: "2026-11-27T10:00:00Z" },
                    { version: 1, created_at: "2026-11-27T09:00:00Z" },
                ]}
                currentVersion={1}
                onSelectVersion={vi.fn()}
                loadVersionFiles={vi.fn()}
            />,
        );
        fireEvent.mouseDown(screen.getByRole("tab", { name: "代码" }));
        fireEvent.click(screen.getByRole("tab", { name: "代码" }));
        expect(screen.queryByRole("button", { name: /对比 v/ })).toBeNull();
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
