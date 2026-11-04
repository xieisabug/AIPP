import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import {
    __resetPlatformCacheForTests,
    MOBILE_PLATFORM_HTML_CLASS,
    toPlatformKind,
    useIsMobilePlatform,
    usePlatformKind,
} from "./use-platform";

describe("use-platform tri-state", () => {
    afterEach(() => {
        clearAllMockHandlers();
        __resetPlatformCacheForTests();
        document.documentElement.classList.remove(MOBILE_PLATFORM_HTML_CLASS);
    });

    it("maps platforms to mobile / desktop / unknown", () => {
        expect(toPlatformKind("android")).toBe("mobile");
        expect(toPlatformKind("ios")).toBe("mobile");
        expect(toPlatformKind("windows")).toBe("desktop");
        expect(toPlatformKind("linux")).toBe("desktop");
        expect(toPlatformKind("unknown")).toBe("unknown");
        expect(toPlatformKind(null)).toBe("unknown");
    });

    it("is unknown (rendered as desktop) before get_platform resolves on a desktop UA, then desktop", async () => {
        let resolve!: (value: string) => void;
        mockInvokeHandler("get_platform", () => new Promise<string>((r) => { resolve = r; }));
        const { result } = renderHook(() => ({ kind: usePlatformKind(), mobile: useIsMobilePlatform() }));
        expect(result.current).toEqual({ kind: "unknown", mobile: false });
        resolve("linux");
        await waitFor(() => expect(result.current.kind).toBe("desktop"));
        expect(result.current.mobile).toBe(false);
        expect(document.documentElement.classList.contains(MOBILE_PLATFORM_HTML_CLASS)).toBe(false);
    });

    it("resolves android to mobile, tags <html>, and later mounts read the cached value synchronously", async () => {
        mockInvokeHandler("get_platform", () => "android");
        const first = renderHook(() => usePlatformKind());
        await waitFor(() => expect(first.result.current).toBe("mobile"));
        expect(document.documentElement.classList.contains(MOBILE_PLATFORM_HTML_CLASS)).toBe(true);
        const second = renderHook(() => usePlatformKind());
        expect(second.result.current).toBe("mobile");
    });
});
