import { useLayoutEffect, useRef, type ReactNode } from "react";
import { motion, useMotionValueEvent, type MotionValue } from "motion/react";
import {
    BALL_FLOOR, DOT_GAP, DOT_LIFT, alertKick, ballBounce, barLevel, clamp01, dotLift, easeOut, frac,
    morphRadius, pencilPoint, pencilStroke, smooth, type MorphKind,
} from "./avatarMorphs";

type Pieces = SVGElement[];
const set = (node: Element | undefined, attrs: Record<string, string | number>) => {
    if (!node) return;
    for (const key in attrs) node.setAttribute(key, typeof attrs[key] === "number" ? (attrs[key] as number).toFixed(2) : attrs[key] as string);
};
const PROGRESS_R = 150, PROGRESS_C = 2 * Math.PI * PROGRESS_R;
const powerArc = (() => {
    const a = 40 * Math.PI / 180, x = PROGRESS_R * Math.sin(a), y = -PROGRESS_R * Math.cos(a);
    return `M${x.toFixed(1)} ${y.toFixed(1)}A${PROGRESS_R} ${PROGRESS_R} 0 1 1 ${(-x).toFixed(1)} ${y.toFixed(1)}`;
})();

/**
 * 装饰图元：只画圆核「之外」的部分（圆核就是缩小后的身体，由 AnimatedAvatar 渲染）。
 * 结构静态、逐帧只写 attribute；w 为该装饰当前权重（amount × swap），e 为入场展开度。
 */
const morphParts: Record<MorphKind, { nodes: ReactNode; update: (n: Pieces, t: number, w: number) => void }> = {
    dots: {
        nodes: [0, 1].map(i => <circle key={i} data-piece="" stroke="none" />),
        update: (n, t, w) => {
            const e = easeOut(w);
            [-1, 1].forEach((side, k) => set(n[k], { cx: side * DOT_GAP * e, cy: -DOT_LIFT * dotLift(t, side < 0 ? 0 : 2), r: morphRadius.dots * (0.45 + 0.55 * e), opacity: w }));
        },
    },
    orbit: {
        nodes: <g transform="rotate(-14)">
            <ellipse data-piece="" rx="175" ry="58" fill="none" strokeWidth="8" />
            {[0, 1].map(i => <circle key={i} data-piece="" stroke="none" />)}
        </g>,
        update: (n, t, w) => {
            const e = easeOut(w);
            set(n[0], { opacity: 0.16 * w, transform: `scale(${e.toFixed(3)})` });
            for (let k = 0; k < 2; k++) {
                const a = t * 1.7 + k * Math.PI, depth = Math.sin(a);
                set(n[k + 1], { cx: 175 * Math.cos(a) * e, cy: 58 * depth * e, r: 26 * (0.82 + 0.26 * depth), opacity: w * (0.62 + 0.38 * depth) });
            }
        },
    },
    radar: {
        nodes: [0, 1].map(i => <circle key={i} data-piece="" fill="none" />),
        update: (n, t, w) => n.forEach((node, k) => {
            const f = frac(t / 1.8 + k * 0.5), r = morphRadius.radar + (232 - morphRadius.radar) * easeOut(f);
            set(node, { r, "stroke-width": 4 + 16 * (1 - f), opacity: w * (1 - f) ** 1.4 * 0.9 });
        }),
    },
    progress: {
        nodes: <>
            <circle data-piece="" r={PROGRESS_R} fill="none" strokeWidth="24" />
            <circle data-piece="" r={PROGRESS_R} fill="none" strokeWidth="26" />
        </>,
        update: (n, t, w) => {
            const e = easeOut(w), scale = `scale(${(0.6 + 0.4 * e).toFixed(3)})`;
            // Material 式转圈：头部伸长→尾部追上，整体同时自转。
            const cycle = t / 1.6, g = frac(cycle), min = 0.08 * PROGRESS_C, span = 0.6 * PROGRESS_C;
            const grow = g < 0.5 ? smooth(g / 0.5) : 1, shrink = g < 0.5 ? 0 : smooth((g - 0.5) / 0.5);
            const length = min + span * (grow - shrink), offset = -(Math.floor(cycle) * span + span * shrink);
            set(n[0], { opacity: 0.14 * w, transform: scale });
            set(n[1], { opacity: w, transform: `${scale} rotate(${(t * 200 - 90).toFixed(1)})`, "stroke-dasharray": `${length.toFixed(1)} ${PROGRESS_C.toFixed(1)}`, "stroke-dashoffset": offset });
        },
    },
    spark: {
        nodes: Array.from({ length: 6 }, (_, i) => <circle key={i} data-piece="" stroke="none" />),
        update: (n, t, w) => n.forEach((node, i) => {
            const f = frac(t / 1.6 + i / 6), a = (i * 60 + t * 30) * Math.PI / 180;
            const d = morphRadius.spark + (235 - morphRadius.spark) * (1 - f * f);
            set(node, { cx: Math.cos(a) * d, cy: Math.sin(a) * d, r: 10 + 10 * (1 - f), opacity: w * Math.sin(Math.PI * f) ** 0.7 });
        }),
    },
    bars: {
        nodes: [-2, -1, 1, 2].map(i => <rect key={i} data-piece="" x="-26" width="52" rx="26" stroke="none" />),
        update: (n, t, w) => {
            const e = easeOut(w);
            [-2, -1, 1, 2].forEach((slot, k) => {
                const h = 52 + 150 * barLevel(t, k) * e;
                set(n[k], { y: -h / 2, height: h, transform: `translate(${(slot * 92 * e).toFixed(1)} 0)`, opacity: w });
            });
        },
    },
    send: {
        nodes: [0, 1].map(i => <path key={i} data-piece="" d="M-52 24L0-24L52 24" fill="none" strokeWidth="26" />),
        update: (n, t, w) => n.forEach((node, k) => {
            const f = frac(t / 1.5 - 0.3 - k * 0.16) / 0.6;
            const y = -morphRadius.send - 50 - 160 * easeOut(f);
            set(node, { transform: `translate(0 ${y.toFixed(1)})`, opacity: f < 1 ? w * (1 - f) * clamp01(f / 0.1) : 0 });
        }),
    },
    receive: {
        nodes: [0, 1, 2].map(i => <path key={i} data-piece="" d="M-52-24L0 24L52-24" fill="none" strokeWidth="26" />),
        update: (n, t, w) => n.forEach((node, k) => {
            const f = frac(t / 1.7 + k / 3), y = -238 + (238 - morphRadius.receive - 46) * f;
            set(node, { transform: `translate(0 ${y.toFixed(1)}) scale(${(0.7 + 0.3 * f).toFixed(3)})`, opacity: w * Math.sin(Math.PI * f) ** 0.7 });
        }),
    },
    upload: {
        nodes: <rect data-piece="" x="-150" y="135" width="300" height="30" rx="15" stroke="none" />,
        update: (n, _t, w) => set(n[0], { transform: `scale(${easeOut(w).toFixed(3)} 1)`, opacity: w }),
    },
    pencil: {
        nodes: <polyline data-piece="" fill="none" strokeWidth="18" />,
        update: (n, t, w) => {
            const stroke = pencilStroke(t), points: string[] = [];
            for (let i = 0; i <= 60; i++) {
                const f = Math.min(i / 60, stroke.progress), p = pencilPoint(f);
                points.push(`${p.x.toFixed(1)},${p.y.toFixed(1)}`);
                if (f >= stroke.progress) break;
            }
            set(n[0], { points: points.join(" "), opacity: 0.55 * w * stroke.trail });
        },
    },
    alert: {
        nodes: <rect data-piece="" x="-38" y="-175" width="76" height="215" rx="38" stroke="none" />,
        update: (n, t, w) => {
            const { angle } = alertKick(t), e = easeOut(w);
            set(n[0], { transform: `rotate(${angle.toFixed(2)} 0 160) translate(0 40) scale(1 ${e.toFixed(3)}) translate(0 -40)`, opacity: w });
        },
    },
    ball: {
        nodes: <ellipse data-piece="" cy={BALL_FLOOR + morphRadius.ball + 8} ry="12" stroke="none" />,
        update: (n, t, w) => { const { height } = ballBounce(t); set(n[0], { rx: 72 * (1 - 0.5 * height), opacity: w * 0.2 * (1 - 0.6 * height) }); },
    },
    power: {
        nodes: <g fill="none" strokeWidth="26"><path data-piece="" d={powerArc} /><path data-piece="" d="M0-200V-100" /></g>,
        update: (n, t, w) => {
            const alpha = w * (0.5 + 0.35 * (0.5 + 0.5 * Math.sin(t * 1.6))), scale = `scale(${(0.7 + 0.3 * easeOut(w)).toFixed(3)})`;
            n.forEach(node => set(node, { opacity: alpha, transform: scale }));
        },
    },
};

export function MorphParts({ kind, time, weight, color }: { kind: MorphKind; time: MotionValue<number>; weight: MotionValue<number>; color: string }) {
    const root = useRef<SVGGElement>(null);
    const update = (t: number) => {
        const nodes = root.current ? [...root.current.querySelectorAll<SVGElement>("[data-piece]")] : [];
        morphParts[kind].update(nodes, t, clamp01(weight.get()));
    };
    useMotionValueEvent(time, "change", update);
    // 挂载/换种类时先按当前时刻摆好，避免第一帧图元堆在原点闪一下。
    // eslint-disable-next-line react-hooks/exhaustive-deps
    useLayoutEffect(() => update(time.get()), [kind]);
    return <g ref={root} data-avatar-effect={kind} fill={color} stroke={color} strokeWidth="9" strokeLinecap="round" strokeLinejoin="round">
        {morphParts[kind].nodes}
    </g>;
}

/** 庆祝彩带：唯一不让身体变形的装饰，叠加在形象上方。 */
export function ConfettiOverlay({ time, opacity }: { time: MotionValue<number>; opacity: MotionValue<number> }) {
    const root = useRef<SVGGElement>(null);
    useMotionValueEvent(time, "change", t => {
        root.current?.querySelectorAll<SVGElement>("[data-piece]").forEach((node, i) => {
            const f = (t * 0.45 + i / 10) % 1, angle = i * 2.399;
            node.setAttribute("transform", `translate(${Math.cos(angle) * (90 + f * 160)} ${Math.sin(angle) * (90 + f * 140) + f * f * 100}) rotate(${f * 300})`);
            node.setAttribute("opacity", String(Math.sin(f * Math.PI)));
        });
    });
    return <motion.g ref={root} data-avatar-effect="confetti" style={{ opacity }}>
        {Array.from({ length: 10 }, (_, i) => <rect key={i} data-piece="" width="9" height="23" rx="3" fill={["#D8A242", "#698CC4", "#B77BA3"][i % 3]} />)}
    </motion.g>;
}
