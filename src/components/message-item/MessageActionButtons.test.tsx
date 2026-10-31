import { act, fireEvent, render, screen } from "@testing-library/react";
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

function renderInBubble() {
    const onCopy = vi.fn();
    render(
        <div className="group" data-testid="bubble">
            <p data-testid="text">hello</p>
            <MessageActionButtons
                messageId={1}
                messageType="response"
                isUserMessage={false}
                copyIconState="copy"
                onCopy={onCopy}
            />
        </div>,
    );
    const bar = document.querySelector('[data-aipp-slot="message-action-bar"]') as HTMLElement;
    return { bar, onCopy };
}

describe("MessageActionButtons", () => {
    beforeEach(() => {
        platformState.mobile = false;
    });

    it("keeps the desktop hover classes unchanged", () => {
        const { bar } = renderInBubble();
        expect(bar.className).toContain("hidden group-hover:flex");
        expect(bar.className).toContain("absolute -bottom-9");
        expect(bar.className).not.toContain("aipp-mobile-action-bar");
    });

    it("on mobile keeps the more button visible and expands actions", () => {
        platformState.mobile = true;
        const { bar, onCopy } = renderInBubble();
        expect(bar).toBeVisible();
        expect(bar.className).toContain("aipp-mobile-action-bar");
        expect(bar.className).not.toContain("group-hover");

        const more = screen.getByRole("button", { name: "消息操作" });
        expect(more).toHaveAttribute("aria-expanded", "false");
        expect(screen.getByLabelText("token-info").parentElement).toHaveClass("hidden");
        fireEvent.click(more);
        expect(more).toHaveAttribute("aria-expanded", "true");
        expect(screen.getByLabelText("token-info").parentElement).toHaveClass("contents");

        // 点击操作栏里的按钮不会把栏收起
        act(() => {
            fireEvent.click(screen.getByTitle("复制消息"));
        });
        expect(onCopy).toHaveBeenCalledTimes(1);
        expect(more).toHaveAttribute("aria-expanded", "true");

        act(() => {
            fireEvent.click(screen.getByTestId("text"));
        });
        expect(more).toHaveAttribute("aria-expanded", "false");
        expect(more).toBeVisible();
    });
});
