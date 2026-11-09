import { describe, expect, it } from "vitest";

import {
    computeChatViewportHeights,
    LAST_REPLY_TOP_OFFSET_PX,
} from "./layoutConstants";

function makeScrollContainer(padding: number, gap: number, clientHeight: number) {
    const container = document.createElement("div");
    container.style.paddingTop = `${padding}px`;
    container.style.paddingBottom = `${padding}px`;
    container.style.rowGap = `${gap}px`;
    Object.defineProperty(container, "clientHeight", {
        configurable: true,
        get: () => clientHeight,
    });
    document.body.appendChild(container);
    return container;
}

describe("computeChatViewportHeights", () => {
    // 贴底时 last-reply 顶部 = clientHeight − paddingBottom − rowGap − minHeight
    it.each([
        ["desktop p-6", 24],
        ["mobile p-3", 12],
    ])("keeps the last user message at the same top offset (%s)", (_label, padding) => {
        const clientHeight = 800;
        const gap = 16;
        const container = makeScrollContainer(padding, gap, clientHeight);

        const { liveOnlyHeight } = computeChatViewportHeights(container);
        const lastReplyTop = clientHeight - padding - gap - liveOnlyHeight;

        expect(lastReplyTop).toBe(LAST_REPLY_TOP_OFFSET_PX);
        container.remove();
    });
});
