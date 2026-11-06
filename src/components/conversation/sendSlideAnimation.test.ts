import { afterEach, describe, expect, it, vi } from "vitest";

import { SEND_SLIDE_DURATION_MS, startSendSlide } from "./sendSlideAnimation";

interface FakeAnimation {
    keyframes: Keyframe[];
    options: KeyframeAnimationOptions;
    cancel: ReturnType<typeof vi.fn>;
    onfinish: (() => void) | null;
}

function setup() {
    const clipHost = document.createElement("div");
    const target = document.createElement("div");
    clipHost.appendChild(target);
    document.body.appendChild(clipHost);
    const animations: FakeAnimation[] = [];
    target.animate = vi.fn((keyframes: Keyframe[], options: KeyframeAnimationOptions) => {
        const animation: FakeAnimation = {
            keyframes,
            options,
            cancel: vi.fn(),
            onfinish: null,
        };
        animations.push(animation);
        return animation as unknown as Animation;
    }) as unknown as HTMLElement["animate"];
    return { clipHost, target, animations };
}

describe("startSendSlide", () => {
    afterEach(() => {
        document.body.innerHTML = "";
        vi.restoreAllMocks();
        vi.unstubAllGlobals();
    });

    it("animates only transform from the compensating offset back to zero", () => {
        const { clipHost, target, animations } = setup();

        const slide = startSendSlide(clipHost, target, 320);

        expect(animations).toHaveLength(1);
        expect(animations[0].keyframes).toEqual([
            { transform: "translate3d(0, 320px, 0)" },
            { transform: "translate3d(0, 0, 0)" },
        ]);
        expect(animations[0].options.duration).toBe(SEND_SLIDE_DURATION_MS);
        // transform 会撑大滚动区域；动画期间必须裁掉，否则钉底会追着它跑
        expect(clipHost.style.overflowY).toBe("clip");
        expect(slide.isActive()).toBe(true);

        animations[0].onfinish?.();
        expect(clipHost.style.overflowY).toBe("");
        expect(slide.isActive()).toBe(false);
    });

    it("adds later jumps onto the current visual offset without a gap frame", () => {
        const { clipHost, target, animations } = setup();
        vi.spyOn(window, "getComputedStyle").mockReturnValue({
            transform: "matrix(1, 0, 0, 1, 0, 150)",
        } as CSSStyleDeclaration);

        const slide = startSendSlide(clipHost, target, 320);
        slide.addOffset(40);

        expect(animations).toHaveLength(2);
        expect(animations[1].keyframes[0]).toEqual({
            transform: "translate3d(0, 190px, 0)",
        });
        expect(animations[0].cancel).toHaveBeenCalledTimes(1);

        // 旧动画的 finish 不能提前收尾新动画
        animations[0].onfinish?.();
        expect(clipHost.style.overflowY).toBe("clip");
    });

    it("is a no-op for reduced motion or negligible offsets", () => {
        const { clipHost, target, animations } = setup();
        expect(startSendSlide(clipHost, target, 0.4).isActive()).toBe(false);

        vi.stubGlobal("matchMedia", () => ({ matches: true }) as MediaQueryList);
        expect(startSendSlide(clipHost, target, 300).isActive()).toBe(false);
        expect(animations).toHaveLength(0);
        expect(clipHost.style.overflowY).toBe("");
    });
});
