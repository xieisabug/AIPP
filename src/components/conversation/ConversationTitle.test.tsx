import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ConversationTitle from "./ConversationTitle";
import { AntiLeakageProvider } from "@/contexts/AntiLeakageContext";
import type { Conversation } from "@/data/Conversation";

const platformState = vi.hoisted(() => ({ platform: "windows" as string }));
vi.mock("@/hooks/use-platform", async (importOriginal) => ({
    ...(await importOriginal<typeof import("@/hooks/use-platform")>()),
    usePlatform: () => platformState.platform,
}));
vi.mock("@/hooks/useConversationManager", () => ({
    default: () => ({ deleteConversation: vi.fn() }),
}));
vi.mock("./DeepSeekBalanceBadge", () => ({ default: () => null }));
vi.mock("../token-statistics", () => ({
    ConversationStatsDialog: () => <button type="button">统计</button>,
}));
vi.mock("./ConversationExportDialog", () => ({
    default: () => <button type="button">导出</button>,
}));

const sampleConversation: Conversation = {
    id: 42,
    name: "短标题",
    assistant_id: 1,
    assistant_name: "通用助手",
    created_time: new Date("2026-11-24T00:00:00Z"),
};

const slot = (name: string) => document.querySelector(`[data-aipp-slot="${name}"]`) as HTMLElement | null;

describe("ConversationTitle drag regions", () => {
    beforeEach(() => {
        platformState.platform = "windows";
    });

    it("should keep non-interactive areas and subtext draggable while limiting rename click target to title text", () => {
        const onEdit = vi.fn();
        render(
            <AntiLeakageProvider enabled={false}>
                <ConversationTitle conversation={sampleConversation} onEdit={onEdit} onDelete={vi.fn()} />
            </AntiLeakageProvider>,
        );

        expect(slot("chat-conversation-title-bar")).toHaveAttribute("data-tauri-drag-region");
        const titleContainer = slot("chat-conversation-title")!;
        expect(titleContainer).toHaveAttribute("data-tauri-drag-region");
        expect(titleContainer.className).toContain("items-start");

        const titleText = slot("chat-conversation-title-text")!;
        expect(titleText).not.toHaveAttribute("data-tauri-drag-region");
        fireEvent.click(titleText);
        expect(onEdit).toHaveBeenCalledOnce();

        expect(slot("chat-conversation-title-subtext")).toHaveAttribute("data-tauri-drag-region");
        expect(slot("chat-conversation-title-actions")).toHaveAttribute("data-tauri-drag-region");
        expect(screen.getByTitle("重命名")).not.toHaveAttribute("data-tauri-drag-region");
        expect(screen.getByTitle("删除对话")).not.toHaveAttribute("data-tauri-drag-region");
    });

    it("should make title text itself draggable when renaming is disabled", () => {
        render(
            <AntiLeakageProvider enabled={false}>
                <ConversationTitle conversation={sampleConversation} onEdit={vi.fn()} onDelete={vi.fn()} allowRename={false} />
            </AntiLeakageProvider>,
        );

        expect(slot("chat-conversation-title-text")).toHaveAttribute("data-tauri-drag-region");
    });

    it("should omit drag attributes on Linux where system title bar is used", () => {
        platformState.platform = "linux";
        render(
            <AntiLeakageProvider enabled={false}>
                <ConversationTitle conversation={sampleConversation} onEdit={vi.fn()} onDelete={vi.fn()} allowRename={false} />
            </AntiLeakageProvider>,
        );

        expect(slot("chat-conversation-title-bar")).not.toHaveAttribute("data-tauri-drag-region");
        expect(slot("chat-conversation-title")).not.toHaveAttribute("data-tauri-drag-region");
        expect(slot("chat-conversation-title-text")).not.toHaveAttribute("data-tauri-drag-region");
        expect(slot("chat-conversation-title-subtext")).not.toHaveAttribute("data-tauri-drag-region");
        expect(slot("chat-conversation-title-actions")).not.toHaveAttribute("data-tauri-drag-region");
    });
});
