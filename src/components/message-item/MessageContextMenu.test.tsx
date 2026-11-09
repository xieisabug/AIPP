import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import MessageContextMenu from "./MessageContextMenu";
import type { MessageActionButtonsProps } from "./MessageActionButtons";

const platformState = vi.hoisted(() => ({ mobile: false }));
vi.mock("@/hooks/use-platform", () => ({
    useIsMobilePlatform: () => platformState.mobile,
}));
vi.mock("../token-statistics", () => ({
    MessageTokenStatsDialog: () => <div data-testid="token-dialog" />,
}));
vi.mock("./MessageExportDialog", () => ({ default: () => <div data-testid="export-dialog" /> }));

function setup(overrides: Partial<MessageActionButtonsProps> = {}) {
    const actions: MessageActionButtonsProps = {
        messageId: 1,
        messageType: "response",
        isUserMessage: false,
        copyIconState: "copy",
        onCopy: vi.fn(),
        onEdit: vi.fn(),
        onRegenerate: vi.fn(),
        onFork: vi.fn(),
        messageContent: "hello",
        ...overrides,
    };
    render(
        <MessageContextMenu actions={actions}>
            <div data-testid="bubble">
                <p>hello</p>
                <span data-testid="own-menu" onContextMenu={(event) => event.preventDefault()}>图片</span>
            </div>
        </MessageContextMenu>,
    );
    return actions;
}

const menu = () => document.querySelector('[data-aipp-slot="message-context-menu"]');

describe("MessageContextMenu", () => {
    beforeEach(() => {
        platformState.mobile = false;
    });

    it("should offer the toolbar actions when right-clicking a message on desktop", () => {
        const actions = setup();
        fireEvent.contextMenu(screen.getByTestId("bubble"), { clientX: 20, clientY: 30 });
        expect(menu()).not.toBeNull();
        for (const label of ["复制消息", "编辑消息", "重新生成", "分叉会话", "消息信息", "导出消息"]) {
            expect(screen.getByRole("menuitem", { name: label })).toBeInTheDocument();
        }
        fireEvent.click(screen.getByRole("menuitem", { name: "重新生成" }));
        expect(actions.onRegenerate).toHaveBeenCalledTimes(1);
        expect(menu()).toBeNull();
    });

    it("should close the menu after clicking a plugin action such as quote reply", () => {
        const quote = vi.fn();
        setup({ pluginActions: <button type="button" onClick={quote}>引用回复</button> });
        fireEvent.contextMenu(screen.getByTestId("bubble"));
        fireEvent.click(screen.getByRole("button", { name: "引用回复" }));
        expect(quote).toHaveBeenCalledTimes(1);
        expect(menu()).toBeNull();
    });

    it("should hide actions the toolbar would hide when the message type does not support them", () => {
        setup({ messageType: "user", isUserMessage: true });
        fireEvent.contextMenu(screen.getByTestId("bubble"));
        expect(screen.getByRole("menuitem", { name: "编辑消息" })).toBeInTheDocument();
        expect(screen.queryByRole("menuitem", { name: "分叉会话" })).toBeNull();
        expect(screen.queryByRole("menuitem", { name: "消息信息" })).toBeNull();
    });

    it("should leave right-clicks to an inner element that has its own menu", () => {
        setup();
        fireEvent.contextMenu(screen.getByTestId("own-menu"));
        expect(menu()).toBeNull();
    });

    it("should not take over right-click or long-press when on mobile", () => {
        platformState.mobile = true;
        setup();
        const event = fireEvent.contextMenu(screen.getByTestId("bubble"));
        expect(event).toBe(true);
        expect(menu()).toBeNull();
    });
});
