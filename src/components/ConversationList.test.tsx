import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ConversationList from "./ConversationList";
import { AntiLeakageProvider } from "@/contexts/AntiLeakageContext";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";

vi.mock("@/hooks/useConversationListStatus", () => ({
    useConversationListStatus: () => ({ getItemStatus: () => "idle" }),
}));
vi.mock("./ConversationTitleEditDialog", () => ({ default: () => null }));
vi.mock("./ConfirmDialog", () => ({ default: () => null }));

const row = (id: number, name = `Chat ${id}`) => ({ id, name, assistant_name: "Assistant" });
const show = (conversationId = "1") => {
    const onSelectConversation = vi.fn();
    const rendered = render(<AntiLeakageProvider enabled={false}>
        <ConversationList conversationId={conversationId} onSelectConversation={onSelectConversation} />
    </AntiLeakageProvider>);
    return { ...rendered, onSelectConversation };
};
const sync = () => act(async () => { await emit("conversation_data_changed"); });

beforeEach(() => { clearAllMockHandlers(); vi.clearAllMocks(); });

describe("ConversationList remote invalidation", () => {
    it("should refresh additions and titles without changing the selection when synced", async () => {
        let rows = [row(1, "Before")];
        mockInvokeHandler("list_conversations", () => rows);
        const { onSelectConversation } = show();
        await screen.findByText("Before");
        rows = [row(2, "New chat"), row(1, "Renamed")];
        await sync();
        await screen.findByText("Renamed");
        expect(screen.getByText("New chat")).toBeInTheDocument();
        expect(screen.queryByText("Before")).not.toBeInTheDocument();
        expect(onSelectConversation).not.toHaveBeenCalled();
    });

    it("should select the first remaining chat when the selected chat was deleted remotely", async () => {
        let rows = [row(1), row(2)];
        mockInvokeHandler("list_conversations", () => rows);
        mockInvokeHandler("conversation_exists", () => false);
        const { onSelectConversation } = show();
        await screen.findByText("Chat 1");
        rows = [row(2)];
        await sync();
        await waitFor(() => expect(onSelectConversation).toHaveBeenCalledWith("2"));
        expect(screen.queryByText("Chat 1")).not.toBeInTheDocument();
    });

    it("should open an empty chat when all conversations were removed remotely", async () => {
        let rows = [row(1)];
        mockInvokeHandler("list_conversations", () => rows);
        mockInvokeHandler("conversation_exists", () => false);
        const { onSelectConversation } = show();
        await screen.findByText("Chat 1");
        rows = [];
        await sync();
        await waitFor(() => expect(onSelectConversation).toHaveBeenCalledWith(""));
    });

    it("should keep a selected search result when it exists outside the loaded range", async () => {
        mockInvokeHandler("list_conversations", () => [row(1)]);
        mockInvokeHandler("conversation_exists", () => true);
        const { onSelectConversation } = show("999");
        await screen.findByText("Chat 1");
        await sync();
        await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === "conversation_exists")).toHaveLength(2));
        expect(onSelectConversation).not.toHaveBeenCalled();
    });

    it("should refresh every loaded page without duplicate rows when sync changes the ordering", async () => {
        let rows = Array.from({ length: 250 }, (_, index) => row(index + 1));
        mockInvokeHandler("list_conversations", (args) => rows.slice(0, Number(args?.pageSize)));
        const { container } = show();
        await screen.findByText("Chat 100");
        const list = container.querySelector('[data-aipp-slot="chat-conversation-list"]') as HTMLElement;
        Object.defineProperties(list, {
            scrollTop: { value: 900, writable: true }, scrollHeight: { value: 1000 }, clientHeight: { value: 100 },
        });
        fireEvent.scroll(list);
        await screen.findByText("Chat 200");
        rows = [row(999, "New first"), ...rows.filter((item) => item.id !== 2)];
        await sync();
        await screen.findByText("New first");
        expect(screen.queryByText("Chat 2")).not.toBeInTheDocument();
        expect(screen.getAllByText("Chat 100")).toHaveLength(1);
        expect(container.querySelectorAll('[data-aipp-slot="chat-conversation-list-item"]')).toHaveLength(200);
        expect(invoke).toHaveBeenLastCalledWith("list_conversations", { page: 1, pageSize: 200 });
    });
});
