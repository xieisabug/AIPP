import { afterEach, describe, expect, it, vi } from "vitest";

import { playSentBubbleAnimation } from "./sentBubbleAnimation";

function renderRow(messageId: number) {
    const container = document.createElement("div");
    container.innerHTML = `
        <div data-message-item data-message-id="${messageId}">
            <span data-queued-indicator></span>
            <div data-bubble></div>
        </div>`;
    document.body.appendChild(container);
    return container;
}

describe("playSentBubbleAnimation", () => {
    afterEach(() => {
        document.body.innerHTML = "";
        vi.unstubAllGlobals();
    });

    it("animates only transform/opacity on the bubble element", () => {
        const container = renderRow(42);
        const bubble = container.querySelector<HTMLElement>("[data-bubble]")!;
        const cancel = vi.fn();
        const animate = vi.fn(() => ({ cancel }) as unknown as Animation);
        bubble.animate = animate;

        const stop = playSentBubbleAnimation(container, 42);

        expect(animate).toHaveBeenCalledTimes(1);
        const [keyframes, options] = animate.mock.calls[0] as unknown as [
            Keyframe[],
            KeyframeAnimationOptions,
        ];
        keyframes.forEach((frame) => {
            Object.keys(frame).forEach((prop) => {
                expect(["opacity", "transform"]).toContain(prop);
            });
        });
        expect(keyframes[0].opacity).toBe(0);
        expect(keyframes.at(-1)?.opacity).toBe(1);
        expect(options.fill).toBe("backwards");

        stop();
        expect(cancel).toHaveBeenCalledTimes(1);
    });

    it("skips the animation when the user prefers reduced motion", () => {
        vi.stubGlobal("matchMedia", () => ({ matches: true }) as MediaQueryList);
        const container = renderRow(7);
        const bubble = container.querySelector<HTMLElement>("[data-bubble]")!;
        const animate = vi.fn();
        bubble.animate = animate;

        playSentBubbleAnimation(container, 7);

        expect(animate).not.toHaveBeenCalled();
    });
});
