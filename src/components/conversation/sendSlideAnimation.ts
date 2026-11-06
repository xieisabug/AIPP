/** 发送后「内容滑上去」的视觉时长 */
export const SEND_SLIDE_DURATION_MS = 460;
/** easeOutQuint 风格：起步干脆、收尾很软 */
export const SEND_SLIDE_EASING = "cubic-bezier(0.22, 1, 0.36, 1)";
/** 动画进行中被追加位移时，至少留这么久收尾，避免最后一下猛冲 */
const MIN_RETARGET_DURATION_MS = 160;

export interface SendSlide {
    /** 滚动位置又跳了 delta（新 − 旧）：叠加到当前视觉偏移上继续滑 */
    addOffset: (delta: number) => void;
    isActive: () => boolean;
    cancel: () => void;
}

const INACTIVE_SLIDE: SendSlide = {
    addOffset: () => {},
    isActive: () => false,
    cancel: () => {},
};

function prefersReducedMotion(): boolean {
    return typeof window !== "undefined"
        && typeof window.matchMedia === "function"
        && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** 读当前 transform 的 translateY（含正在播放的动画值） */
function readTranslateY(element: HTMLElement): number {
    const transform = getComputedStyle(element).transform;
    if (!transform || transform === "none") {
        return 0;
    }
    const values = transform
        .slice(transform.indexOf("(") + 1, -1)
        .split(",")
        .map((part) => Number.parseFloat(part));
    if (transform.startsWith("matrix3d")) {
        return values[13] || 0;
    }
    return values[5] || 0;
}

/**
 * 发送后的「滑动」视觉。
 *
 * 真正的 scrollTop 由钉底逻辑一次跳到最终位置（保证一定落在 user 气泡贴顶的位置），
 * 这里给内容加一个反向 translateY 再动画回 0，看起来就是平滑滚上去。
 * 只动 transform，由合成线程执行：发送瞬间主线程在挂载 assistant 占位、启动流式渲染，
 * 逐帧改 scrollTop 会跟着卡，transform 动画不会。
 *
 * clipHost 在动画期间设 overflow-y: clip：transform 会撑大滚动区域，
 * 不裁掉的话 scrollHeight 跟着变大，钉底又追着它跑。
 */
export function startSendSlide(
    clipHost: HTMLElement,
    target: HTMLElement,
    initialOffsetPx: number,
    durationMs = SEND_SLIDE_DURATION_MS,
): SendSlide {
    if (
        prefersReducedMotion()
        || typeof target.animate !== "function"
        || Math.abs(initialOffsetPx) < 1
    ) {
        return INACTIVE_SLIDE;
    }

    let done = false;
    let endsAt = performance.now() + durationMs;
    let animation: Animation | null = null;
    const previousOverflowY = clipHost.style.overflowY;
    clipHost.style.overflowY = "clip";

    const finish = () => {
        if (done) {
            return;
        }
        done = true;
        animation?.cancel();
        animation = null;
        clipHost.style.overflowY = previousOverflowY;
    };

    const play = (fromOffsetPx: number, duration: number) => {
        const previous = animation;
        const next = target.animate(
            [
                { transform: `translate3d(0, ${fromOffsetPx}px, 0)` },
                { transform: "translate3d(0, 0, 0)" },
            ],
            { duration, easing: SEND_SLIDE_EASING },
        );
        // 先挂新的再撤旧的，中间不会露出一帧无 transform 的状态
        previous?.cancel();
        animation = next;
        next.onfinish = () => {
            if (animation === next) {
                finish();
            }
        };
    };

    play(initialOffsetPx, durationMs);

    return {
        addOffset: (delta) => {
            if (done || Math.abs(delta) < 0.5) {
                return;
            }
            const now = performance.now();
            endsAt = Math.max(endsAt, now + MIN_RETARGET_DURATION_MS);
            play(readTranslateY(target) + delta, endsAt - now);
        },
        isActive: () => !done && performance.now() < endsAt,
        cancel: finish,
    };
}
