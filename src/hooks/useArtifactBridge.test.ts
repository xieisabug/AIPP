import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { iframeTargetOrigin, resolveBoundTarget, useArtifactBridge } from "./useArtifactBridge";

const ORIGINS = ["http://localhost", "http://127.0.0.1"];

function fakeIframe(src: string) {
    const contentWindow = { postMessage: vi.fn() };
    const iframe = { src, contentWindow, addEventListener: vi.fn(), removeEventListener: vi.fn() };
    return { iframe: iframe as unknown as HTMLIFrameElement, contentWindow };
}

function dispatchFromFrame(source: unknown, data: unknown, origin = "http://localhost:4100") {
    const event = new MessageEvent("message", { data, origin });
    Object.defineProperty(event, "source", { value: source });
    window.dispatchEvent(event);
}

describe("resolveBoundTarget", () => {
    it("should use the bound value when the page does not ask for one", () => {
        expect(resolveBoundTarget("notes", undefined, "db")).toBe("notes");
        expect(resolveBoundTarget(7, null, "assistant")).toBe(7);
    });

    it("should accept the page repeating the bound value", () => {
        expect(resolveBoundTarget("notes", "notes", "db")).toBe("notes");
        expect(resolveBoundTarget(7, 7, "assistant")).toBe(7);
    });

    it("should reject a different database or assistant", () => {
        expect(() => resolveBoundTarget("notes", "other", "db")).toThrow("只能访问绑定的数据库");
        expect(() => resolveBoundTarget(7, 8, "assistant")).toThrow("只能调用绑定的 AI 助手");
    });

    it("should reject page-supplied targets when nothing is bound", () => {
        expect(() => resolveBoundTarget(undefined, "other", "db")).toThrow("未绑定数据库");
        expect(() => resolveBoundTarget(undefined, 8, "assistant")).toThrow("未绑定 AI 助手");
        expect(() => resolveBoundTarget(undefined, undefined, "db")).toThrow("No database ID configured");
    });
});

describe("iframeTargetOrigin", () => {
    it("should return the preview origin for allowed hosts", () => {
        expect(iframeTargetOrigin(fakeIframe("http://localhost:4100/index.html").iframe, ORIGINS)).toBe("http://localhost:4100");
    });

    it("should return null for external or empty sources", () => {
        expect(iframeTargetOrigin(fakeIframe("https://example.com/").iframe, ORIGINS)).toBeNull();
        expect(iframeTargetOrigin(fakeIframe("").iframe, ORIGINS)).toBeNull();
        expect(iframeTargetOrigin(null, ORIGINS)).toBeNull();
    });
});

describe("useArtifactBridge", () => {
    afterEach(() => clearAllMockHandlers());

    it("should send config only to the iframe origin", () => {
        const { iframe, contentWindow } = fakeIframe("http://localhost:4100/");
        const { result } = renderHook(() =>
            useArtifactBridge({ iframeRef: { current: iframe }, config: { db_id: "notes" }, allowedOrigins: ORIGINS }),
        );
        result.current.sendConfig();
        expect(contentWindow.postMessage).toHaveBeenCalledTimes(1);
        expect(contentWindow.postMessage).toHaveBeenCalledWith(
            { type: "aipp_config", config: { db_id: "notes" } },
            "http://localhost:4100",
        );
    });

    it("should not broadcast config after the iframe points to an external site", () => {
        const { iframe, contentWindow } = fakeIframe("https://example.com/");
        const { result } = renderHook(() =>
            useArtifactBridge({ iframeRef: { current: iframe }, config: { db_id: "notes" }, allowedOrigins: ORIGINS }),
        );
        result.current.sendConfig();
        expect(contentWindow.postMessage).not.toHaveBeenCalled();
    });

    it("should query the bound database and refuse another one", async () => {
        const calls: unknown[] = [];
        mockInvokeHandler("artifact_db_query", (args) => {
            calls.push(args);
            return { columns: [], rows: [], row_count: 0 };
        });
        const { iframe, contentWindow } = fakeIframe("http://localhost:4100/");
        renderHook(() =>
            useArtifactBridge({ iframeRef: { current: iframe }, config: { db_id: "notes" }, allowedOrigins: ORIGINS }),
        );

        dispatchFromFrame(contentWindow, { id: "a", type: "db_query", payload: { sql: "select 1" } });
        dispatchFromFrame(contentWindow, { id: "b", type: "db_query", payload: { sql: "select 1", db_id: "other" } });

        await waitFor(() => expect(contentWindow.postMessage).toHaveBeenCalledTimes(2));
        expect(calls).toEqual([{ request: { db_id: "notes", sql: "select 1", params: [] } }]);
        const responses = contentWindow.postMessage.mock.calls.map(([response]) => response);
        expect(responses).toContainEqual(expect.objectContaining({ id: "a", success: true }));
        expect(responses).toContainEqual(
            expect.objectContaining({ id: "b", success: false, error: expect.stringContaining("只能访问绑定的数据库") }),
        );
    });

    it("should refuse to call an assistant other than the bound one", async () => {
        const ask = vi.fn(() => ({ content: "ok", model: "m" }));
        mockInvokeHandler("artifact_ai_ask", ask);
        const { iframe, contentWindow } = fakeIframe("http://localhost:4100/");
        renderHook(() =>
            useArtifactBridge({ iframeRef: { current: iframe }, config: { assistant_id: 3 }, allowedOrigins: ORIGINS }),
        );

        dispatchFromFrame(contentWindow, { id: "c", type: "ai_ask", payload: { prompt: "hi", assistant_id: 9 } });

        await waitFor(() => expect(contentWindow.postMessage).toHaveBeenCalled());
        expect(ask).not.toHaveBeenCalled();
        expect(contentWindow.postMessage.mock.calls[0][0]).toMatchObject({ id: "c", success: false });
    });

    it("should ignore messages from other frames", async () => {
        const query = vi.fn();
        mockInvokeHandler("artifact_db_query", query);
        const { iframe, contentWindow } = fakeIframe("http://localhost:4100/");
        renderHook(() =>
            useArtifactBridge({ iframeRef: { current: iframe }, config: { db_id: "notes" }, allowedOrigins: ORIGINS }),
        );

        dispatchFromFrame({ postMessage: vi.fn() }, { id: "d", type: "db_query", payload: { sql: "select 1" } });

        await new Promise((resolve) => setTimeout(resolve, 20));
        expect(query).not.toHaveBeenCalled();
        expect(contentWindow.postMessage).not.toHaveBeenCalled();
    });
});
