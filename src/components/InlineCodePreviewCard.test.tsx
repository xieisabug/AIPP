import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import InlineCodePreviewCard from "@/components/InlineCodePreviewCard";
import {
    PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX,
    PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS,
} from "@/utils/previewCode";

vi.mock("@/hooks/useDisplayConfig", () => ({
    useDisplayConfig: () => ({
        config: null,
        isLoading: false,
        error: null,
        isUserMessageMarkdownEnabled: true,
        isMergeAssistantMessages: true,
        isShowThinking: true,
        isPreviewCodeShowToolbar: true,
        refreshConfig: vi.fn(),
    }),
}));

describe("InlineCodePreviewCard", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
        vi.useRealTimers();
    });

    it("renders final HTML content inside the host container", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "html_preview",
                    renderer: "html",
                    code: "<div>Rendered HTML</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={1}
                messageId={10}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        expect(await screen.findByText("html_preview")).toBeInTheDocument();
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() =>
            expect(host.shadowRoot?.textContent).toContain("Rendered HTML")
        );
    });

    it("does not render raw external image URLs before authorization", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "external_image",
                    renderer: "html",
                    code: '<img alt="remote" src=https://example.com/raw.png><style>.hero{background:url(https://example.com/bg.png)}</style>',
                    interaction_mode: "submit_once",
                })}
                conversationId={1}
                messageId={101}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        expect(await screen.findByRole("button", { name: "需要加载外部资源" })).toBeInTheDocument();
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() => {
            const img = host.shadowRoot?.querySelector<HTMLImageElement>('img[alt="remote"]');
            expect(img).not.toBeNull();
            expect(img?.getAttribute("src")).not.toBe("https://example.com/raw.png");
            expect(host.shadowRoot?.innerHTML).not.toContain("https://example.com/bg.png");
        });
    });

    it("uses proxy when authorizing client-detected preview_code resources with proxy button", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        mockInvokeHandler("authorize_preview_code_external_resource_urls", () => ({
            previewCode: {
                request_id: "authorized-request",
                conversation_id: 1,
                title: "external_image",
                renderer: "html",
                code: '<img alt="remote" src="aipp-preview://localhost/image-ok">',
                loadingMessages: [],
                interactionMode: "submit_once",
                externalResources: {
                    requestId: "authorized-request",
                    resources: [],
                },
            },
        }));

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "external_image",
                    renderer: "html",
                    code: '<img alt="remote" src=https://example.com/raw.png>',
                    interaction_mode: "submit_once",
                })}
                conversationId={1}
                messageId={102}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const user = userEvent.setup();
        await user.click(await screen.findByRole("button", { name: "需要加载外部资源" }));
        await user.click(await screen.findByRole("button", { name: "使用代理加载所选资源" }));

        await waitFor(() => {
            expect(invoke).toHaveBeenCalledWith(
                "authorize_preview_code_external_resource_urls",
                expect.objectContaining({
                    conversationId: 1,
                    conversation_id: 1,
                    useProxy: true,
                    use_proxy: true,
                    resources: [
                        expect.objectContaining({
                            originalUrl: "https://example.com/raw.png",
                            normalizedUrl: "https://example.com/raw.png",
                            type: "image",
                        }),
                    ],
                })
            );
        });
    });

    it("keeps generated styles inside a shadow root", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "scoped_styles",
                    renderer: "html",
                    code: "<style>body { color: rgb(255, 0, 0); }</style><div>Scoped UI</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={3}
                messageId={12}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() =>
            expect(host.shadowRoot?.textContent).toContain("Scoped UI")
        );
        const shadowStyles = Array.from(host.shadowRoot?.querySelectorAll("style") ?? []).map(
            (style) => style.textContent ?? ""
        );
        expect(shadowStyles.some((text) => text.includes(":host"))).toBe(true);
        expect(host.querySelector("style")).toBeNull();
    });

    it("defaults historical previews to a collapsed masked viewport and expands on overlay click", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "collapse_me",
                    renderer: "html",
                    code: "<div>Collapsible</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={2}
                messageId={11}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const user = userEvent.setup();
        const expandOverlay = await screen.findByRole("button", { name: "展开预览" });
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() => expect(host.shadowRoot?.textContent).toContain("Collapsible"));
        expect(host).not.toHaveClass("hidden");
        expect(host).toHaveStyle({
            height: `${PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX}px`,
            minHeight: `${PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX}px`,
            maxHeight: `${PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX}px`,
        });
        expect(host.className).toContain("overflow-hidden");
        expect(host.className).not.toContain("overflow-auto");
        expect(screen.getByText("点击展开预览")).toBeInTheDocument();

        await user.click(expandOverlay);
        await waitFor(() => {
            expect(screen.queryByRole("button", { name: "展开预览" })).not.toBeInTheDocument();
        });
        expect(await screen.findByRole("button", { name: "收起" })).toBeInTheDocument();
        expect(host.style.height).toBe("");
        expect(host.style.minHeight).toBe("");
        expect(host.style.maxHeight).toBe("");
        expect(invoke).not.toHaveBeenCalledWith(
            "submit_preview_code_response",
            expect.anything()
        );
    });

    it("keeps the last message preview expanded by default and supports local hide and show", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "latest_preview",
                    renderer: "html",
                    code: "<div>Latest Preview</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={6}
                messageId={14}
                isLastMessage
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const user = userEvent.setup();
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() =>
            expect(host.shadowRoot?.textContent).toContain("Latest Preview")
        );
        expect(host.style.height).toBe("");
        expect(await screen.findByRole("button", { name: "隐藏" })).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "展开预览" })).not.toBeInTheDocument();
        expect(screen.getByRole("button", { name: "收起" })).toBeInTheDocument();

        await user.click(screen.getByRole("button", { name: "隐藏" }));
        expect(await screen.findByText("预览已隐藏。")).toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "显示" })).toBeInTheDocument();

        await user.click(screen.getByRole("button", { name: "显示" }));
        expect(screen.queryByText("预览已隐藏。")).not.toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "隐藏" })).toBeInTheDocument();
        const restoredHost = await screen.findByTestId("preview-code-host");
        await waitFor(() =>
            expect(restoredHost.shadowRoot?.textContent).toContain("Latest Preview")
        );
    });

    it("keeps hide available while collapsed and can show the preview again", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "collapsed_hide",
                    renderer: "html",
                    code: "<div>Collapsed Hide</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={8}
                messageId={16}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const user = userEvent.setup();
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() => expect(host.shadowRoot?.textContent).toContain("Collapsed Hide"));
        expect(await screen.findByRole("button", { name: "隐藏" })).toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "展开预览" })).toBeInTheDocument();

        await user.click(screen.getByRole("button", { name: "隐藏" }));
        expect(await screen.findByText("预览已隐藏。")).toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "显示" })).toBeInTheDocument();

        await user.click(screen.getByRole("button", { name: "显示" }));
        expect(await screen.findByRole("button", { name: "隐藏" })).toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "展开预览" })).toBeInTheDocument();
    });

    it("does not render a manual dismiss action for display-only previews", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "display_only",
                    renderer: "html",
                    code: "<div>Display Only</div>",
                    interaction_mode: "none",
                })}
                conversationId={5}
                messageId={13}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        expect(await screen.findByText("display_only")).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "关闭并继续" })).not.toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith(
            "submit_preview_code_response",
            expect.anything()
        );
    });

    it("treats omitted interaction_mode as display-only", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "default_display_only",
                    renderer: "html",
                    code: "<div>Display Only By Default</div>",
                })}
                conversationId={15}
                messageId={115}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        expect(await screen.findByText("default_display_only")).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "关闭并继续" })).not.toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith(
            "submit_preview_code_response",
            expect.anything()
        );
    });

    it("keeps scripted historical previews static while collapsed and enables interaction after expand", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "script_preview",
                    renderer: "html",
                    code: "<div>Static First</div><script>const marker = document.createElement('div'); marker.textContent = 'Script Active'; marker.setAttribute('data-script-active', 'true'); document.body.appendChild(marker);</script>",
                    interaction_mode: "submit_once",
                })}
                conversationId={7}
                messageId={15}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const user = userEvent.setup();
        const host = await screen.findByTestId("preview-code-host");
        await screen.findByText("点击展开预览");
        expect(host.shadowRoot?.textContent).toContain("Static First");
        expect(host.shadowRoot?.querySelector('[data-script-active="true"]')).toBeNull();

        await user.click(screen.getByRole("button", { name: "展开预览" }));

        await waitFor(() => expect(host.shadowRoot?.textContent).toContain("Script Active"));
        expect(screen.getByRole("button", { name: "收起" })).toBeInTheDocument();
    });

    it("throttles visible streaming preview updates at the card level instead of repainting every chunk", () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        vi.useFakeTimers();
        vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
            return window.setTimeout(() => callback(performance.now()), 16);
        });
        vi.spyOn(window, "cancelAnimationFrame").mockImplementation((handle) => {
            window.clearTimeout(handle);
        });

        const { rerender } = render(
            <InlineCodePreviewCard
                parameters="{}"
                llmCallId="preview_stream_test"
                conversationId={30}
                messageId={21}
                mcpToolCallStates={new Map()}
                isStreaming
                isLastMessage
                streamingPreviewState={{
                    title: "stream_preview",
                    renderer: "html",
                    code: "<div>chunk-1</div>",
                    loadingMessages: ["正在生成交互面板"],
                    interactionMode: "submit_once",
                    hasRenderableDom: true,
                    containsScript: false,
                    renderableHtml: "<div>chunk-1</div>",
                    sourceExcerpt: "<div>chunk-1</div>",
                }}
            />
        );

        const host = screen.getByTestId("preview-code-host");
        act(() => {
            vi.advanceTimersByTime(16);
        });
        expect(host.shadowRoot?.textContent).toContain("chunk-1");

        rerender(
            <InlineCodePreviewCard
                parameters="{}"
                llmCallId="preview_stream_test"
                conversationId={30}
                messageId={21}
                mcpToolCallStates={new Map()}
                isStreaming
                isLastMessage
                streamingPreviewState={{
                    title: "stream_preview",
                    renderer: "html",
                    code: "<div>chunk-2</div>",
                    loadingMessages: ["正在生成交互面板"],
                    interactionMode: "submit_once",
                    hasRenderableDom: true,
                    containsScript: false,
                    renderableHtml: "<div>chunk-2</div>",
                    sourceExcerpt: "<div>chunk-2</div>",
                }}
            />
        );

        act(() => {
            vi.advanceTimersByTime(PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS - 1);
        });
        expect(host.shadowRoot?.textContent).toContain("chunk-1");

        act(() => {
            vi.advanceTimersByTime(1);
        });
        expect(host.shadowRoot?.textContent).toContain("chunk-2");
    });

    it("throttles correctly across many rapid streaming chunks", () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        vi.useFakeTimers();
        vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
            return window.setTimeout(() => callback(performance.now()), 16);
        });
        vi.spyOn(window, "cancelAnimationFrame").mockImplementation((handle) => {
            window.clearTimeout(handle);
        });

        const makeState = (n: number): Parameters<typeof render>[0] => (
            <InlineCodePreviewCard
                parameters="{}"
                llmCallId="rapid_stream_test"
                conversationId={40}
                messageId={31}
                mcpToolCallStates={new Map()}
                isStreaming
                isLastMessage
                streamingPreviewState={{
                    title: "stream_preview",
                    renderer: "html",
                    code: `<p>chunk-${n}</p>`,
                    loadingMessages: ["加载中"],
                    interactionMode: "none",
                    hasRenderableDom: true,
                    containsScript: false,
                    renderableHtml: `<p>chunk-${n}</p>`,
                    sourceExcerpt: `<p>chunk-${n}</p>`,
                }}
            />
        );

        const { rerender } = render(makeState(1));
        const host = screen.getByTestId("preview-code-host");
        act(() => { vi.advanceTimersByTime(16); });
        expect(host.shadowRoot?.textContent).toContain("chunk-1");

        // Send 20 rapid chunks at 50ms intervals (over 1 second total)
        for (let i = 2; i <= 20; i++) {
            rerender(makeState(i));
            act(() => { vi.advanceTimersByTime(50); });
        }
        // After 950ms of chunk arrivals since first apply:
        // chunk-1 should still be visible (throttle hasn't fired yet)
        expect(host.shadowRoot?.textContent).toContain("chunk-1");

        // Advance past the 1-second component throttle boundary + runtime rAF
        act(() => { vi.advanceTimersByTime(50); });
        // Flush the runtime's scheduled apply (rAF-like timer)
        act(() => { vi.advanceTimersByTime(16); });
        // Now the throttled update should have applied the latest pending chunk
        expect(host.shadowRoot?.textContent).not.toContain("chunk-1");

        // Record the currently displayed chunk
        const textAfterFirstThrottle = host.shadowRoot?.textContent ?? "";

        // Send more chunks for the next throttle cycle
        for (let i = 21; i <= 30; i++) {
            rerender(makeState(i));
            act(() => { vi.advanceTimersByTime(50); });
        }
        // During this 500ms window, the display should still show the previous throttled value
        expect(host.shadowRoot?.textContent).toBe(textAfterFirstThrottle);

        // Cross the next throttle boundary + runtime flush
        act(() => { vi.advanceTimersByTime(PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS); });
        act(() => { vi.advanceTimersByTime(16); });
        // Now should show a newer chunk
        expect(host.shadowRoot?.textContent).not.toBe(textAfterFirstThrottle);
    });

    it("simulates full model streaming pipeline: 3s of incremental HTML generation with patch timeline", () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        vi.useFakeTimers();
        vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
            return window.setTimeout(() => callback(performance.now()), 16);
        });
        vi.spyOn(window, "cancelAnimationFrame").mockImplementation((handle) => {
            window.clearTimeout(handle);
        });

        // Simulate a model generating a compound-interest calculator page token by token.
        // Each chunk represents one streaming SSE event from the LLM.
        const htmlTokens = [
            '<div class="card">',
            "\n  <h2>",
            "复利",
            "计算器",
            "</h2>",
            "\n  <form>",
            '\n    <label>本金</label><input type="number" id="p" value="10000">',
            '\n    <label>利率</label><input type="number" id="r" value="5">',
            '\n    <label>年数</label><input type="number" id="y" value="10">',
            '\n    <button type="button" id="calc">计算</button>',
            "\n  </form>",
            '\n  <div id="result"></div>',
            "\n</div>",
            '\n<style>.card{padding:16px;font-family:sans-serif}form{display:flex;flex-direction:column;gap:8px}</style>',
        ];

        const buildState = (tokenCount: number) => {
            const code = htmlTokens.slice(0, tokenCount).join("");
            const hasRenderableDom = code.includes("<");
            return {
                title: "compound_interest",
                renderer: "html" as const,
                code,
                loadingMessages: ["正在生成交互面板"],
                interactionMode: "submit_once" as const,
                hasRenderableDom,
                containsScript: false,
                renderableHtml: code,
                sourceExcerpt: code.slice(0, 200),
            };
        };

        // --- Render first chunk ---
        const { rerender } = render(
            <InlineCodePreviewCard
                parameters="{}"
                llmCallId="sim_stream_pipeline"
                conversationId={50}
                messageId={40}
                mcpToolCallStates={new Map()}
                isStreaming
                isLastMessage
                streamingPreviewState={buildState(1)}
            />,
        );
        const host = screen.getByTestId("preview-code-host");

        // Flush initial rAF so first patch lands
        act(() => { vi.advanceTimersByTime(16); });

        // --- Record DOM changes across simulated streaming ---
        const CHUNK_INTERVAL_MS = 50;
        const patchTimeline: Array<{ elapsedMs: number; tokenCount: number; text: string }> = [];
        let elapsedMs = 16;
        let lastText = host.shadowRoot?.querySelector(".aipp-preview-code-root")?.textContent ?? "";
        patchTimeline.push({ elapsedMs, tokenCount: 1, text: lastText });

        // Stream remaining tokens, one every 50ms (simulates ~20 tokens/sec LLM speed)
        for (let t = 2; t <= htmlTokens.length; t++) {
            rerender(
                <InlineCodePreviewCard
                    parameters="{}"
                    llmCallId="sim_stream_pipeline"
                    conversationId={50}
                    messageId={40}
                    mcpToolCallStates={new Map()}
                    isStreaming
                    isLastMessage
                    streamingPreviewState={buildState(t)}
                />,
            );
            act(() => { vi.advanceTimersByTime(CHUNK_INTERVAL_MS); });
            elapsedMs += CHUNK_INTERVAL_MS;

            const text = host.shadowRoot?.querySelector(".aipp-preview-code-root")?.textContent ?? "";
            if (text !== lastText) {
                patchTimeline.push({ elapsedMs, tokenCount: t, text });
                lastText = text;
            }
        }

        // Continue advancing time to let final throttle window flush
        for (let tick = 0; tick < 30; tick++) {
            act(() => { vi.advanceTimersByTime(100); });
            elapsedMs += 100;
            const text = host.shadowRoot?.querySelector(".aipp-preview-code-root")?.textContent ?? "";
            if (text !== lastText) {
                patchTimeline.push({ elapsedMs, tokenCount: htmlTokens.length, text });
                lastText = text;
                break;
            }
        }

        // --- Print patch timeline for debugging ---
        // eslint-disable-next-line no-console
        console.log(
            "\n📊 Component pipeline patch timeline:\n" +
            patchTimeline
                .map(
                    (p, idx) =>
                        `  [${idx}] t=${String(p.elapsedMs).padStart(5)}ms  tokens=${String(p.tokenCount).padStart(3)}  text="${p.text.slice(0, 80)}${p.text.length > 80 ? "…" : ""}"`,
                )
                .join("\n"),
        );

        // --- Assertions ---
        // First patch should be immediate (within first 100ms)
        expect(patchTimeline[0].elapsedMs).toBeLessThanOrEqual(100);

        // Subsequent patches should be ≥1000ms apart (within margin)
        for (let i = 2; i < patchTimeline.length; i++) {
            const gap = patchTimeline[i].elapsedMs - patchTimeline[i - 1].elapsedMs;
            expect(
                gap,
                `Patch gap between [${i - 1}] (t=${patchTimeline[i - 1].elapsedMs}) and [${i}] (t=${patchTimeline[i].elapsedMs}) was only ${gap}ms, expected ≥${PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS}ms`,
            ).toBeGreaterThanOrEqual(PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS - 50);
        }

        // Total patches should be bounded: for N seconds of streaming, at most N+1 patches
        const totalStreamingMs = htmlTokens.length * CHUNK_INTERVAL_MS;
        const maxExpectedPatches = Math.ceil(totalStreamingMs / PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS) + 2;
        expect(
            patchTimeline.length,
            `Expected at most ${maxExpectedPatches} patches for ${totalStreamingMs}ms of streaming, got ${patchTimeline.length}`,
        ).toBeLessThanOrEqual(maxExpectedPatches);

        // Content should be progressively more complete
        if (patchTimeline.length >= 2) {
            expect(patchTimeline[patchTimeline.length - 1].text.length)
                .toBeGreaterThan(patchTimeline[0].text.length);
        }
    });
});
