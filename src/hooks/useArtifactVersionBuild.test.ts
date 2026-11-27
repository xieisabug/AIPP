import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useArtifactVersionBuild } from "./useArtifactVersionBuild";

vi.mock("@tauri-apps/api/core", async () => ({ ...(await import("@/__tests__/mocks/tauri")) }));
const confirmDialog = vi.hoisted(() => vi.fn(async () => true));
vi.mock("@/components/common/ConfirmHost", () => ({ confirmDialog }));

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

    it("should ask in-app before building and then open the new output", async () => {
        let built = false;
        const build = vi.fn((args?: Record<string, unknown>) => {
            if (args?.confirmedPrompt !== "bun install") return { status: "confirm", prompt: "bun install" };
            built = true;
            return { status: "done", value: null };
        });
        mockInvokeHandler("open_artifact_version_build", () => (built ? { url: "http://new/", lease: "L2" } : null));
        mockInvokeHandler("build_artifact_version", build);
        mockInvokeHandler("close_choice_build", () => undefined);
        const { result } = renderHook(() => useArtifactVersionBuild({ ...target, enabled: true }));
        await waitFor(() => expect(result.current?.url).toBeNull());
        await act(async () => {
            await result.current!.build("dist");
        });
        expect(confirmDialog).toHaveBeenCalledWith(expect.objectContaining({ description: "bun install" }));
        expect(build).toHaveBeenLastCalledWith({ conversationId: 7, artifactKey: "ui/app", version: 2, outputDir: "dist", confirmedPrompt: "bun install" });
        await waitFor(() => expect(result.current?.url).toBe("http://new/"));
    });

    it("should not build when the user cancels the confirmation", async () => {
        confirmDialog.mockResolvedValueOnce(false);
        const build = vi.fn(() => ({ status: "confirm", prompt: "bun install" }));
        mockInvokeHandler("open_artifact_version_build", () => null);
        mockInvokeHandler("build_artifact_version", build);
        const { result } = renderHook(() => useArtifactVersionBuild({ ...target, enabled: true }));
        await act(async () => {
            await result.current!.build("dist");
        });
        expect(build).toHaveBeenCalledTimes(1);
        expect(result.current?.error).toBe("");
        expect(result.current?.building).toBe(false);
    });

    it("should surface build errors", async () => {
        mockInvokeHandler("open_artifact_version_build", () => null);
        mockInvokeHandler("build_artifact_version", () => {
            throw new Error("bun install 退出状态 1");
        });
        const { result } = renderHook(() => useArtifactVersionBuild({ ...target, enabled: true }));
        await act(async () => {
            await result.current!.build("dist");
        });
        expect(result.current?.error).toContain("bun install 退出状态 1");
        expect(result.current?.building).toBe(false);
    });
});
