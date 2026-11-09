import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { runMobileBackHandlers } from "@/mobile/navigation";
import { useMobileConversationDetails } from "./useMobileConversationDetails";

function attachScroller(result: { current: ReturnType<typeof useMobileConversationDetails> }, top: number) {
    const scroller = document.createElement("div");
    scroller.scrollTop = top;
    act(() => result.current.detailsScrollRef(scroller));
    return scroller;
}

describe("useMobileConversationDetails", () => {
    it("should restore details and scroll position when artifact preview returns", () => {
        const { result } = renderHook(() => useMobileConversationDetails({ conversationId: "7", enabled: true }));
        act(() => result.current.openDetails());
        const scroller = attachScroller(result, 160);

        act(() => result.current.suspendDetails("artifact"));
        scroller.scrollTop = 0;
        expect(result.current.detailsOpen).toBe(false);
        act(() => {
            expect(runMobileBackHandlers("preview")).toBe(false);
        });

        expect(result.current.detailsOpen).toBe(true);
        expect(scroller.scrollTop).toBe(160);
    });

    it("should write the saved scroll position when the details scroller remounts", () => {
        const { result } = renderHook(() => useMobileConversationDetails({ conversationId: "7", enabled: true }));
        act(() => result.current.openDetails());
        attachScroller(result, 160);

        act(() => result.current.suspendDetails("artifact"));
        act(() => result.current.detailsScrollRef(null));
        act(() => {
            expect(runMobileBackHandlers("preview")).toBe(false);
        });

        const remounted = document.createElement("div");
        act(() => result.current.detailsScrollRef(remounted));
        expect(result.current.detailsOpen).toBe(true);
        expect(remounted.scrollTop).toBe(160);
    });

    it("should restore context preview only after it opened and closed", () => {
        const { result, rerender } = renderHook(
            ({ previewFileOpen }) => useMobileConversationDetails({ conversationId: "7", enabled: true, previewFileOpen }),
            { initialProps: { previewFileOpen: false } },
        );
        act(() => result.current.openDetails());
        const scroller = attachScroller(result, 90);
        act(() => result.current.suspendDetails("context"));

        rerender({ previewFileOpen: false });
        expect(result.current.detailsOpen).toBe(false);
        rerender({ previewFileOpen: true });
        expect(result.current.detailsOpen).toBe(false);
        scroller.scrollTop = 0;
        rerender({ previewFileOpen: false });

        expect(result.current.detailsOpen).toBe(true);
        expect(scroller.scrollTop).toBe(90);
    });

    it("should restore details when context preview was already open", () => {
        const { result, rerender } = renderHook(
            ({ previewFileOpen }) => useMobileConversationDetails({ conversationId: "7", enabled: true, previewFileOpen }),
            { initialProps: { previewFileOpen: true } },
        );
        act(() => result.current.openDetails());
        attachScroller(result, 40);
        act(() => result.current.suspendDetails("context"));

        expect(result.current.detailsOpen).toBe(false);
        rerender({ previewFileOpen: true });
        expect(result.current.detailsOpen).toBe(false);
        rerender({ previewFileOpen: false });

        expect(result.current.detailsOpen).toBe(true);
    });

    it("should not restore details into another conversation", () => {
        const { result, rerender } = renderHook(
            ({ conversationId }) => useMobileConversationDetails({ conversationId, enabled: true }),
            { initialProps: { conversationId: "7" } },
        );
        act(() => result.current.openDetails());
        act(() => result.current.suspendDetails("artifact"));
        rerender({ conversationId: "8" });

        act(() => {
            expect(runMobileBackHandlers("preview")).toBe(false);
        });
        expect(result.current.detailsOpen).toBe(false);
    });

    it("should ignore preview back when details did not open the preview", () => {
        const { result } = renderHook(() => useMobileConversationDetails({ conversationId: "7", enabled: true }));
        act(() => {
            expect(runMobileBackHandlers("preview")).toBe(false);
        });
        expect(result.current.detailsOpen).toBe(false);
    });
});
