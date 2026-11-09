import type { AvatarEffect } from "./avatarRecipes";

/**
 * 功能状态的「变形」：身体原地缩成一个圆核（core），圆核本身就是装饰的主角——
 * 思考时它是三个点里中间那颗，提醒时它是感叹号下面的点，弹球时它就是球。
 * 这里只放纯数学（圆核的位移/形变），渲染与装饰图元见 AvatarEffects.tsx。
 * 坐标与 510 单位的形象骨架一致，原点是变形中心。
 */
export type MorphKind = Exclude<AvatarEffect, "none" | "confetti">;
export const morphKinds: readonly MorphKind[] = ["dots", "orbit", "radar", "progress", "spark", "bars", "send", "receive", "upload", "pencil", "alert", "ball", "power"];
export const isMorphKind = (effect: AvatarEffect): effect is MorphKind => effect !== "none" && effect !== "confetti";

/** 未变形时身体的近似半径，用于把整个身体缩放到圆核半径。 */
export const BODY_RADIUS = 250;
/** 各装饰里圆核的半径。 */
export const morphRadius: Record<MorphKind, number> = {
    dots: 46, orbit: 64, radar: 58, progress: 52, spark: 58, bars: 27, send: 58,
    receive: 58, upload: 52, pencil: 38, alert: 42, ball: 56, power: 48,
};

export interface CoreMotion { dx: number; dy: number; sx: number; sy: number; opacity: number }
export const restCore: CoreMotion = { dx: 0, dy: 0, sx: 1, sy: 1, opacity: 1 };

export const clamp01 = (x: number) => Math.max(0, Math.min(1, x));
export const frac = (x: number) => x - Math.floor(x);
export const smooth = (x: number) => { const t = clamp01(x); return t * t * (3 - 2 * t); };
export const easeOut = (x: number) => 1 - (1 - clamp01(x)) ** 3;

export const DOT_GAP = 150;
export const DOT_LIFT = 52;
/** 三点波浪：每颗点错开 0.14 个周期依次抬起，半个周期抬起、半个周期静止。 */
export function dotLift(t: number, index: number) {
    const p = frac(t / 1.4 - index * 0.14);
    return p < 0.5 ? Math.sin(p / 0.5 * Math.PI) ** 2 : 0;
}

/** 声波条的电平：两个不同频率的正弦相乘，避免看起来像机械循环。 */
export function barLevel(t: number, index: number) {
    return (0.5 + 0.5 * Math.sin(t * (6.3 + index * 1.1) + index * 1.9)) * (0.55 + 0.45 * Math.sin(t * 2.3 + index * 0.8));
}

export const PENCIL_PERIOD = 2.5;
export const PENCIL_DRAW = 0.68;
/** 连笔书写路径：前进中叠加回环，形成手写的「ellll」笔迹。 */
export function pencilPoint(f: number) {
    const a = f * Math.PI * 5;
    return { x: -165 + 330 * f + 40 * Math.sin(a), y: 30 - 75 * (0.5 - 0.5 * Math.cos(a)) };
}
/** 返回笔尖位置与笔迹进度：写完后笔迹淡出，笔尖抬起回到起点。 */
export function pencilStroke(t: number) {
    const p = frac(t / PENCIL_PERIOD);
    if (p < PENCIL_DRAW) {
        const f = p / PENCIL_DRAW;
        return { ...pencilPoint(f), progress: f, trail: 1 };
    }
    const q = smooth((p - PENCIL_DRAW) / (1 - PENCIL_DRAW));
    const end = pencilPoint(1), start = pencilPoint(0);
    return {
        x: end.x + (start.x - end.x) * q, y: end.y + (start.y - end.y) * q - 45 * Math.sin(q * Math.PI),
        progress: 1, trail: 1 - smooth((p - PENCIL_DRAW) / 0.2),
    };
}

export const BALL_PERIOD = 0.95;
export const BALL_FLOOR = 70;
export function ballBounce(t: number) {
    const p = frac(t / BALL_PERIOD);
    const height = 4 * p * (1 - p);
    const contact = Math.max(0, 1 - Math.min(p, 1 - p) / 0.07);
    return { height, contact };
}

export const ALERT_DOT_Y = 118;
/** 每 2.2 秒一次的提醒冲击：衰减的摆动角与弹出量。 */
export function alertKick(t: number) {
    const k = t % 2.2;
    return { angle: Math.sin(k * 28) * 10 * Math.exp(-k * 4), pop: Math.exp(-k * 5.5) };
}

export const UPLOAD_PERIOD = 1.8;
export const UPLOAD_REST = 80;

/** 圆核在给定装饰里的运动；t 为进入该装饰后经过的秒数。 */
export function coreMotion(kind: MorphKind, t: number): CoreMotion {
    switch (kind) {
        case "dots": return { ...restCore, dy: -DOT_LIFT * dotLift(t, 1) };
        case "orbit": return { ...restCore, dy: 6 * Math.sin(t * 1.7) };
        case "radar": { const pulse = 0.07 * Math.exp(-frac(t / 0.9) * 7); return { ...restCore, sx: 1 + pulse, sy: 1 + pulse }; }
        case "progress": { const breath = 0.04 * Math.sin(t * 3.2); return { ...restCore, sx: 1 + breath, sy: 1 + breath }; }
        case "spark": {
            // 每颗粒子汇入时圆核鼓一下
            let swell = 0;
            for (let i = 0; i < 6; i++) { const f = frac(t / 1.6 + i / 6); swell += f > 0.85 ? Math.sin((f - 0.85) / 0.15 * Math.PI) : 0; }
            return { ...restCore, sx: 1 + 0.06 * swell, sy: 1 + 0.06 * swell };
        }
        // 圆核就是中间那根声波条：纵向拉伸到与两侧条同量级
        case "bars": { const level = barLevel(t, 4); return { ...restCore, sy: 1 + 2.8 * level }; }
        case "send": {
            const p = frac(t / 1.5);
            if (p < 0.18) { const s = Math.sin(p / 0.18 * Math.PI); return { ...restCore, dy: 10 * s, sx: 1 + 0.1 * s, sy: 1 - 0.12 * s }; }
            if (p < 0.42) { const s = Math.sin((p - 0.18) / 0.24 * Math.PI); return { ...restCore, dy: -55 * s, sx: 1 - 0.06 * s, sy: 1 + 0.1 * s }; }
            return restCore;
        }
        case "receive": {
            let swell = 0;
            for (let i = 0; i < 3; i++) { const f = frac(t / 1.7 + i / 3); swell += f > 0.82 ? Math.sin((f - 0.82) / 0.18 * Math.PI) : 0; }
            return { ...restCore, sx: 1 + 0.1 * swell, sy: 1 + 0.1 * swell };
        }
        case "upload": {
            const p = frac(t / UPLOAD_PERIOD);
            const rise = smooth((p - 0.15) / 0.55);
            const opacity = p < 0.55 ? 1 : p < 0.7 ? 1 - (p - 0.55) / 0.15 : p < 0.75 ? 0 : clamp01((p - 0.75) / 0.2);
            return { ...restCore, dy: p < 0.7 ? UPLOAD_REST - 200 * rise : UPLOAD_REST, opacity };
        }
        case "pencil": { const s = pencilStroke(t); return { ...restCore, dx: s.x, dy: s.y }; }
        case "alert": {
            const { angle, pop } = alertKick(t);
            const rad = angle * Math.PI / 180, arm = 160 - ALERT_DOT_Y;
            return { ...restCore, dx: arm * Math.sin(rad), dy: ALERT_DOT_Y + arm * (1 - Math.cos(rad)), sx: 1 + 0.12 * pop, sy: 1 + 0.12 * pop };
        }
        case "ball": {
            const { height, contact } = ballBounce(t);
            const sx = 1 + 0.2 * contact, sy = 1 - 0.2 * contact;
            // 压扁时保持底部贴地
            return { ...restCore, dy: BALL_FLOOR - 190 * height + morphRadius.ball * (1 - sy), sx, sy };
        }
        case "power": {
            const breath = 0.5 + 0.5 * Math.sin(t * 1.6);
            return { ...restCore, sx: 0.94 + 0.06 * breath, sy: 0.94 + 0.06 * breath, opacity: 0.52 + 0.4 * breath };
        }
    }
}
