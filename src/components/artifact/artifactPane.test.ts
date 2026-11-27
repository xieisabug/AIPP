import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import {
    ARTIFACT_PANE_WINDOW,
    MIN_WINDOW_WIDTH_FOR_PANE,
    artifactPreviewTarget,
    closeArtifactPane,
    enableArtifactPane,
    useArtifactPaneOpen,
} from "./artifactPane";

describe("artifactPane", () => {
    afterEach(() => enableArtifactPane(false));

    it("should leave previews to the standalone window when the pane is not enabled", () => {
        const { result } = renderHook(() => useArtifactPaneOpen());
        expect(artifactPreviewTarget(1400)).toEqual({});
        expect(result.current).toBe(false);
    });

    it("should open the pane and target the chat window when enabled", () => {
        enableArtifactPane(true);
        const { result } = renderHook(() => useArtifactPaneOpen());
        let target: { sourceWindow?: string } = {};
        act(() => {
            target = artifactPreviewTarget(1400);
        });
        expect(target).toEqual({ sourceWindow: ARTIFACT_PANE_WINDOW });
        expect(result.current).toBe(true);

        act(() => closeArtifactPane());
        expect(result.current).toBe(false);
    });

    it("should fall back to the standalone window when the chat window is too narrow", () => {
        enableArtifactPane(true);
        expect(artifactPreviewTarget(MIN_WINDOW_WIDTH_FOR_PANE - 1)).toEqual({});
    });

    it("should close the pane when disabled", () => {
        enableArtifactPane(true);
        const { result } = renderHook(() => useArtifactPaneOpen());
        act(() => {
            artifactPreviewTarget(1400);
        });
        act(() => enableArtifactPane(false));
        expect(result.current).toBe(false);
    });
});
