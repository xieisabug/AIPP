import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ArtifactCollectionsWindow from "./ArtifactCollectionsWindow";

vi.mock("@tauri-apps/api/core", async () => ({ ...(await import("@/__tests__/mocks/tauri")) }));
vi.mock("@/hooks/useTheme", () => ({ useTheme: () => {} }));
vi.mock("@/components/window/WindowTitleBar", () => ({
    WindowTopBar: () => null,
    dragRegionProps: () => ({}),
    useFramelessTitleBar: () => false,
    useWindowTitleBarLayout: () => true,
}));
vi.mock("@/components/EditArtifactDialog", () => ({ default: () => null }));
const chat = vi.hoisted(() => ({ startConversationInChat: vi.fn().mockResolvedValue(undefined), openConversationInChat: vi.fn() }));
vi.mock("@/utils/openConversationInChat", () => chat);

const source = (version: number) => JSON.stringify({ conversation_id: 7, artifact_key: "timer", version });

function collection() {
    return [
        { id: 1, name: "记账本", icon: "📒", description: "", artifact_type: "react", tags: "财务", created_time: "2026-11-01 00:00:00", last_used_time: "2026-11-20 00:00:00", use_count: 1, source: null },
        { id: 2, name: "番茄钟", icon: "⏱", description: "", artifact_type: "html", tags: "效率", created_time: "2026-11-02 00:00:00", last_used_time: null, use_count: 6, source: source(1) },
    ];
}

function cardNames() {
    return Array.from(document.querySelectorAll("[data-aipp-slot='artifact-collections-card']")).map(
        (card) => card.querySelector("p")?.textContent,
    );
}

describe("ArtifactCollectionsWindow", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_artifacts_collection", collection);
        mockInvokeHandler("get_collection_source_updates", () => [{ id: 2, current_version: 1, latest_version: 3 }]);
    });

    it("should list the most used tool first", async () => {
        render(<ArtifactCollectionsWindow />);
        await waitFor(() => expect(cardNames()).toEqual(["番茄钟", "记账本"]));
    });

    it("should filter by tag", async () => {
        render(<ArtifactCollectionsWindow />);
        const tags = await screen.findByRole("group", { name: "按标签筛选" });
        fireEvent.click(within(tags).getByRole("button", { name: "财务" }));
        expect(cardNames()).toEqual(["记账本"]);
        fireEvent.click(within(tags).getByRole("button", { name: "全部" }));
        expect(cardNames()).toEqual(["番茄钟", "记账本"]);
    });

    it("should update a tool from its source conversation without opening it", async () => {
        const update = vi.fn(() => 3);
        const open = vi.fn();
        mockInvokeHandler("update_collection_from_source", update);
        mockInvokeHandler("open_artifact_window", open);
        render(<ArtifactCollectionsWindow />);

        const badge = await screen.findByTitle(/原会话已有 v3/);
        mockInvokeHandler("get_artifacts_collection", () =>
            collection().map((item) => (item.id === 2 ? { ...item, source: source(3) } : item)),
        );
        mockInvokeHandler("get_collection_source_updates", () => []);
        fireEvent.click(badge);

        await waitFor(() => expect(update).toHaveBeenCalledWith({ id: 2 }));
        await waitFor(() => expect(screen.queryByTitle(/原会话已有 v3/)).toBeNull());
        expect(open).not.toHaveBeenCalled();
    });

    it("should expose edit and delete on the card", async () => {
        render(<ArtifactCollectionsWindow />);
        await waitFor(() => expect(cardNames()).toHaveLength(2));
        expect(screen.getAllByRole("button", { name: "编辑" })).toHaveLength(2);
        fireEvent.click(screen.getAllByRole("button", { name: "删除" })[0]);
        expect(await screen.findByText(/确定要删除 "番茄钟"/)).toBeTruthy();
    });

    it("should start a new conversation referencing a tool that has no source", async () => {
        render(<ArtifactCollectionsWindow />);
        await waitFor(() => expect(cardNames()).toHaveLength(2));
        // 有来源的「番茄钟」显示回原会话，没有来源的「记账本」显示在新会话里修改
        const buttons = screen.getAllByRole("button", { name: "在新会话里修改" });
        expect(buttons).toHaveLength(1);
        expect(screen.getAllByRole("button", { name: "回到原会话继续修改" })).toHaveLength(1);
        fireEvent.click(buttons[0]);
        expect(chat.startConversationInChat).toHaveBeenCalledWith("#记账本 ");
    });
});
