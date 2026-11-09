/** 新发送的 user 气泡入场动画时长；略短于视口滑动，让气泡先站稳 */
export const SENT_BUBBLE_ANIMATION_MS = 380;
const MAX_LOOKUP_FRAMES = 10;

function prefersReducedMotion(): boolean {
    return typeof window !== "undefined"
        && typeof window.matchMedia === "function"
        && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * 给刚发送的 user 气泡播放「从下方浮起 + 淡入 + 轻微放大」的入场动画。
 *
 * 只动 transform / opacity，并用 Web Animations API 交给合成线程：
 * 发送瞬间 React 正在挂载 assistant 占位、启动流式，主线程忙也不会掉帧。
 * transform 不影响布局，所以不会干扰同时进行的滚动定位计算。
 */
export function playSentBubbleAnimation(
    container: HTMLElement,
    messageId: number,
): () => void {
    if (prefersReducedMotion()) {
        return () => {};
    }

    let cancelled = false;
    let frameId: number | null = null;
    let animation: Animation | null = null;
    let attempts = 0;

    const run = () => {
        frameId = null;
        if (cancelled) {
            return;
        }
        const row = container.querySelector<HTMLElement>(
            `[data-message-item][data-message-id='${messageId}']`,
        );
        // 行容器是 justify-end 的整行，真正的气泡是最后一个子元素（前面可能是排队指示器）
        const bubble = row?.lastElementChild as HTMLElement | null | undefined;
        if (!bubble) {
            attempts += 1;
            if (attempts < MAX_LOOKUP_FRAMES) {
                frameId = requestAnimationFrame(run);
            }
            return;
        }
        if (typeof bubble.animate !== "function") {
            return;
        }

        // transform-origin 不能放进关键帧：它不可合成，会让整段动画退回主线程
        const previousOrigin = bubble.style.transformOrigin;
        bubble.style.transformOrigin = "100% 100%";
        animation = bubble.animate(
            [
                { opacity: 0, transform: "translate3d(0, 16px, 0) scale(0.97)" },
                { opacity: 1, transform: "translate3d(0, 0, 0) scale(1)" },
            ],
            {
                duration: SENT_BUBBLE_ANIMATION_MS,
                // easeOutQuint 风格：起步干脆、收尾很软
                easing: "cubic-bezier(0.22, 1, 0.36, 1)",
                // 动画开始前就处于起始帧，避免首帧闪一下完整气泡
                fill: "backwards",
            },
        );
        const restoreOrigin = () => {
            bubble.style.transformOrigin = previousOrigin;
        };
        animation.onfinish = restoreOrigin;
        animation.oncancel = restoreOrigin;
    };

    run();

    return () => {
        cancelled = true;
        if (frameId !== null) {
            cancelAnimationFrame(frameId);
        }
        animation?.cancel();
    };
}
