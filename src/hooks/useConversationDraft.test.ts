import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useConversationDraft } from "./useConversationDraft";

describe("useConversationDraft", () => {
    it("restores each conversation draft when switching", () => {
        const { result, rerender } = renderHook(({ id }) => useConversationDraft(id), { initialProps: { id: "a" } });
        act(() => result.current[1]("draft a"));
        rerender({ id: "b" });
        expect(result.current[0]).toBe("");
        act(() => result.current[1]("draft b"));
        rerender({ id: "a" });
        expect(result.current[0]).toBe("draft a");
        rerender({ id: "b" });
        expect(result.current[0]).toBe("draft b");
    });

    it("keeps a delayed setter bound to the original conversation", () => {
        const { result, rerender } = renderHook(({ id }) => useConversationDraft(id), { initialProps: { id: "a" } });
        act(() => result.current[1]("old"));
        const delayedSetter = result.current[1];
        rerender({ id: "b" });
        act(() => result.current[1]("current"));
        act(() => delayedSetter(previous => `${previous} updated`));
        expect(result.current[0]).toBe("current");
        rerender({ id: "a" });
        expect(result.current[0]).toBe("old updated");
    });

    it("clears only the selected conversation draft", () => {
        const { result, rerender } = renderHook(({ id }) => useConversationDraft(id), { initialProps: { id: "a" } });
        act(() => result.current[1]("draft a"));
        rerender({ id: "b" });
        act(() => result.current[1]("draft b"));
        act(() => result.current[1](""));
        expect(result.current[0]).toBe("");
        rerender({ id: "a" });
        expect(result.current[0]).toBe("draft a");
        rerender({ id: "b" });
        expect(result.current[0]).toBe("");
    });
});
