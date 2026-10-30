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

    it("on mobile toggles the bar by tapping the bubble and keeps copy / token reachable", () => {
        platformState.mobile = true;
        const { bar, onCopy } = renderInBubble();
        expect(bar.className).toContain("hidden");
        expect(bar.className).toContain("aipp-mobile-action-bar");
        expect(bar.className).not.toContain("group-hover");

        act(() => {
            fireEvent.click(screen.getByTestId("text"));
        });
        expect(bar.className.split(" ")).toContain("flex");
        expect(screen.getByLabelText("token-info")).toBeTruthy();

        // 点击操作栏里的按钮不会把栏收起
        const buttons = bar.querySelectorAll("button");
        act(() => {
            fireEvent.click(buttons[buttons.length - 1]);
        });
        expect(onCopy).toHaveBeenCalledTimes(1);
        expect(bar.className.split(" ")).toContain("flex");

        act(() => {
            fireEvent.click(screen.getByTestId("text"));
        });
        expect(bar.className.split(" ")).toContain("hidden");
    });
});
