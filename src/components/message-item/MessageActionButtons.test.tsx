import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import MessageActionButtons from "./MessageActionButtons";

const platformState = vi.hoisted(() => ({ mobile: false }));
vi.mock("../../hooks/use-platform", () => ({
    useIsMobilePlatform: () => platformState.mobile,
}));
vi.mock("../token-statistics", () => ({
    MessageTokenTooltip: () => <button type="button" aria-label="token-info">i</button>,
}));
vi.mock("./MessageExportDialog", () => ({ default: () => null }));

function renderInBubble(messageType = "response") {
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
            />
        </div>,
    );
    const bar = document.querySelector('[data-aipp-slot="message-action-bar"]') as HTMLElement;
    return { bar, onCopy, onEdit, onRegenerate, onFork };
}

describe("MessageActionButtons", () => {
    beforeEach(() => {
        platformState.mobile = false;
    });

    it("should keep hover actions when on desktop", () => {
        const { bar } = renderInBubble();
        expect(bar.className).toContain("hidden group-hover:flex");
        expect(bar.className).toContain("absolute -bottom-9");
        expect(bar.className).not.toContain("aipp-mobile-action-bar");
    });

    it("should show actions immediately and keep them visible when tapping a mobile bubble", () => {
        platformState.mobile = true;
        const { bar, onCopy, onEdit, onRegenerate, onFork } = renderInBubble();
        expect(bar).toBeVisible();
        expect(bar.className).toContain("aipp-mobile-action-bar");
        expect(bar.className).not.toContain("group-hover");

        expect(screen.queryByRole("button", { name: "消息操作" })).not.toBeInTheDocument();
        const actions = ["编辑消息", "重新生成", "分叉会话", "复制消息"];
        actions.forEach((title) => expect(screen.getByTitle(title)).toBeVisible());
        expect(screen.getByLabelText("token-info")).toBeVisible();

        fireEvent.click(screen.getByTestId("text"));
        actions.forEach((title) => expect(screen.getByTitle(title)).toBeVisible());
        actions.forEach((title) => fireEvent.click(screen.getByTitle(title)));
        expect(onCopy).toHaveBeenCalledTimes(1);
        expect(onEdit).toHaveBeenCalledTimes(1);
        expect(onRegenerate).toHaveBeenCalledTimes(1);
        expect(onFork).toHaveBeenCalledTimes(1);
        actions.forEach((title) => expect(screen.getByTitle(title)).toBeVisible());
    });

    it("should show user actions immediately when on mobile", () => {
        platformState.mobile = true;
        const { bar } = renderInBubble("user");
        expect(bar).not.toHaveClass("ml-auto");
        expect(screen.getByTitle("编辑消息")).toBeVisible();
        expect(screen.getByTitle("重新生成")).toBeVisible();
        expect(screen.getByTitle("复制消息")).toBeVisible();
        expect(screen.queryByTitle("分叉会话")).not.toBeInTheDocument();
    });
});
