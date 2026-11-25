import { act, renderHook } from "@testing-library/react";
import { createElement, type PropsWithChildren } from "react";
import { describe, expect, it } from "vitest";
import { ConversationDraftContext, createConversationDraftStore, useConversationDraft } from "./useConversationDraft";

describe("useConversationDraft", () => {
    it("should restore each private chat's text when its composer remounts in the same window", () => {
        const store = createConversationDraftStore();
        const wrapper = ({ children }: PropsWithChildren) =>
            createElement(ConversationDraftContext.Provider, { value: store }, children);
        const openChat = (id: string) => renderHook(() => useConversationDraft(id), { wrapper });

        const first = openChat("101");
        const setFirstDraft = first.result.current[1];
        act(() => setFirstDraft("第一行\n还没写完"));
        first.unmount();

        const second = openChat("102");
        expect(second.result.current[0]).toBe("");
        act(() => second.result.current[1]("第二个私聊"));
        second.unmount();

        const reopened = openChat("101");
        expect(reopened.result.current[0]).toBe("第一行\n还没写完");
        // A send callback from the previous mount still updates the visible composer.
        act(() => setFirstDraft(""));
        expect(reopened.result.current[0]).toBe("");
        reopened.unmount();
        expect(openChat("102").result.current[0]).toBe("第二个私聊");
    });

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
