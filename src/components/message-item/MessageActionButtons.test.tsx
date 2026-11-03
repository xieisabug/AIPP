import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import MessageActionButtons from "./MessageActionButtons";

const platformState = vi.hoisted(() => ({ mobile: false }));
vi.mock("../../hooks/use-platform", () => ({
    useIsMobilePlatform: () => platformState.mobile,
}));
vi.mock("../token-statistics", () => ({
    MessageTokenTooltip: () => <button type="button" aria-label="token-info">i</button>,
    MessageTokenStatsDialog: ({ open }: { open: boolean }) => (open ? <div data-testid="token-dialog" /> : null),
}));
vi.mock("./MessageExportDialog", () => ({ default: () => null }));

function renderInBubble(messageType = "response", withExtras = false) {
    const onCopy = vi.fn();
    const onEdit = vi.fn();
    const onRegenerate = vi.fn();
    const onFork = vi.fn();
    render(
        <div className="group" data-testid="bubble">
            <p data-testid="text">hello</p>
            <MessageActionButtons
                messageId={1}
                messageType={messageType}
                isUserMessage={messageType === "user"}
                copyIconState="copy"
                onCopy={onCopy}
                onEdit={onEdit}
                onRegenerate={onRegenerate}
                onFork={onFork}
                messageContent={withExtras ? "hello" : undefined}
                pluginActions={withExtras ? <button>插件动作</button> : undefined}
            />
        </div>,
    );
    const bar = document.querySelector('[data-aipp-slot="message-action-bar"]') as HTMLElement;
    return { bar, onCopy, onEdit, onRegenerate, onFork };
}

describe("MessageActionButtons", () => {
    let resize: () => void;
    let width = 400;
    beforeEach(() => {
        width = 400;
        vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(() => ({ width }) as DOMRect);
        vi.stubGlobal("ResizeObserver", class {
            constructor(callback: () => void) { resize = callback; }
            observe() {}
            disconnect() {}
        });
        platformState.mobile = false;
    });

    it("should keep hover actions when on desktop", () => {
        const { bar } = renderInBubble();
        expect(bar.className).toContain("hidden group-hover:flex");
        expect(bar.className).toContain("absolute -bottom-9");
        expect(bar.className).not.toContain("aipp-mobile-action-bar");
    });

    it("should keep fork export and plugins in more even when the mobile row has space", () => {
        platformState.mobile = true;
        const { onCopy, onEdit, onRegenerate, onFork } = renderInBubble("response", true);
        expect(screen.getByRole("button", { name: "更多操作" })).toBeVisible();
        ["分叉会话", "导出消息", "插件动作"].forEach(name => expect(screen.queryByRole("button", { name })).not.toBeInTheDocument());
        ["复制消息", "编辑消息", "重新生成"].forEach(label =>
            fireEvent.click(screen.getByRole("button", { name: label })));
        [onCopy, onEdit, onRegenerate].forEach(callback => expect(callback).toHaveBeenCalledOnce());
        fireEvent.click(screen.getByRole("button", { name: "更多操作" }));
        expect(screen.getByRole("button", { name: "导出消息" })).toBeVisible();
        expect(screen.getByRole("button", { name: "插件动作" })).toBeVisible();
        fireEvent.click(screen.getByRole("button", { name: "分叉会话" }));
        expect(onFork).toHaveBeenCalledOnce();
        fireEvent.click(screen.getByRole("button", { name: "消息信息" }));
        expect(screen.getByTestId("token-dialog")).toBeInTheDocument();
    });

    it("should move only overflowing actions into more and restore them when resized", () => {
        platformState.mobile = true;
        width = 176; // copy + edit + regenerate + more
        const { onFork } = renderInBubble();
        expect(screen.getByRole("button", { name: "编辑消息" })).toBeVisible();
        expect(screen.getByRole("button", { name: "重新生成" })).toBeVisible();
        expect(screen.queryByRole("button", { name: "分叉会话" })).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: "更多操作" }));
        expect(document.querySelector('[data-aipp-slot="message-action-sheet-edit"]')).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "分叉会话" }));
        expect(onFork).toHaveBeenCalledOnce();
        act(() => { width = 220; resize(); });
        expect(screen.getByRole("button", { name: "更多操作" })).toBeVisible();
        expect(screen.getByRole("button", { name: "消息信息" })).toBeVisible();
        expect(screen.queryByRole("button", { name: "分叉会话" })).not.toBeInTheDocument();
    });

    it("should show user actions directly when they fit", () => {
        platformState.mobile = true;
        renderInBubble("user");
        expect(screen.getByRole("button", { name: "编辑消息" })).toBeVisible();
        expect(screen.getByRole("button", { name: "重新生成" })).toBeVisible();
        expect(screen.queryByRole("button", { name: "更多操作" })).not.toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "分叉会话" })).not.toBeInTheDocument();
    });
});
