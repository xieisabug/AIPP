import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useArtifactVersionBuild } from "./useArtifactVersionBuild";

vi.mock("@tauri-apps/api/core", async () => ({ ...(await import("@/__tests__/mocks/tauri")) }));

const target = { conversationId: 7, artifactKey: "ui/app", version: 2 };

describe("useArtifactVersionBuild", () => {
    afterEach(() => clearAllMockHandlers());

    it("should stay off for single-file content", () => {
        const { result } = renderHook(() => useArtifactVersionBuild({ ...target, enabled: false }));
        expect(result.current).toBeUndefined();
    });

    it("should open an existing build and release it when leaving", async () => {
        const close = vi.fn();
        mockInvokeHandler("open_artifact_version_build", () => ({ url: "http://built/", lease: "L1" }));
        mockInvokeHandler("close_choice_build", close);
        const { result, unmount } = renderHook(() => useArtifactVersionBuild({ ...target, enabled: true }));
        await waitFor(() => expect(result.current?.url).toBe("http://built/"));
        unmount();
        await waitFor(() => expect(close).toHaveBeenCalledWith({ lease: "L1" }));
    });

    it("should build and then open the new output", async () => {
        let built = false;
        const build = vi.fn(() => {
            built = true;
        });
        mockInvokeHandler("open_artifact_version_build", () => (built ? { url: "http://new/", lease: "L2" } : null));
        mockInvokeHandler("build_artifact_version", build);
        mockInvokeHandler("close_choice_build", () => undefined);
        const { result } = renderHook(() => useArtifactVersionBuild({ ...target, enabled: true }));
        await waitFor(() => expect(result.current?.url).toBeNull());
        await act(async () => {
            await result.current!.build("dist");
        });
        expect(build).toHaveBeenCalledWith({ conversationId: 7, artifactKey: "ui/app", version: 2, outputDir: "dist" });
        await waitFor(() => expect(result.current?.url).toBe("http://new/"));
    });

    it("should surface build errors", async () => {
        mockInvokeHandler("open_artifact_version_build", () => null);
        mockInvokeHandler("build_artifact_version", () => {
            throw new Error("用户取消了本次构建");
        });
        const { result } = renderHook(() => useArtifactVersionBuild({ ...target, enabled: true }));
        await act(async () => {
            await result.current!.build("dist");
        });
        expect(result.current?.error).toContain("用户取消了本次构建");
        expect(result.current?.building).toBe(false);
    });
});
