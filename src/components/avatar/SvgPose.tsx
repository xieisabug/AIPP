import { useEffect, useRef, useState, type ReactNode, type SVGProps } from "react";
import { animate, motionValue, type MotionValue, type ValueAnimationTransition } from "motion/react";

const channels = ["x", "y", "rotate", "scale", "scaleX", "scaleY", "opacity"] as const;
type Channel = (typeof channels)[number];
export type SvgPoseTarget = Partial<Record<Channel, number | number[]>>;
const rest: Record<Channel, number> = { x: 0, y: 0, rotate: 0, scale: 1, scaleX: 1, scaleY: 1, opacity: 1 };
const first = (value: number | number[] | undefined) => Array.isArray(value) ? value[0] : value;

const transformOf = (v: Record<Channel, MotionValue<number>>) => {
    const s = v.scale.get();
    return `translate(${v.x.get()} ${v.y.get()}) rotate(${v.rotate.get()}) scale(${s * v.scaleX.get()} ${s * v.scaleY.get()})`;
};

/**
 * 用 SVG `transform` 属性播放关键帧：旋转/缩放永远以元素本地原点为轴。
 * motion 的 CSS transform 在 SVG 上依赖 transform-box，形象的 viewBox 不从 0 开始时轴心会偏，
 * 导致铃铛、翅膀绕着远处的点乱飞、眯眼时眼睛下沉。
 */
export function SvgPose({ pose, transition, children, ...props }: {
    pose: SvgPoseTarget; transition?: ValueAnimationTransition<number>; children?: ReactNode;
} & Omit<SVGProps<SVGGElement>, "transform" | "opacity">) {
    const ref = useRef<SVGGElement>(null);
    const [values] = useState(() => Object.fromEntries(channels.map(channel =>
        [channel, motionValue(first(pose[channel]) ?? rest[channel])])) as Record<Channel, MotionValue<number>>);
    // 只在首帧写入；之后由动画直接改 DOM，React 不会再覆盖（属性值不变）。
    const [initial] = useState(() => ({ transform: transformOf(values), opacity: values.opacity.get() }));
    const signature = JSON.stringify([pose, transition]);

    useEffect(() => {
        const element = ref.current;
        if (!element) return;
        const render = () => {
            element.setAttribute("transform", transformOf(values));
            element.setAttribute("opacity", String(values.opacity.get()));
        };
        const unsubscribe = channels.map(channel => values[channel].on("change", render));
        const options = transition ?? { duration: 0.2 };
        const controls = channels.filter(channel => pose[channel] !== undefined)
            .map(channel => animate(values[channel], pose[channel]!, options));
        return () => { controls.forEach(control => control.stop()); unsubscribe.forEach(off => off()); };
        // pose/transition 每次渲染都是新对象，用序列化签名判断是否真的变化。
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [signature, values]);

    return <g ref={ref} transform={initial.transform} opacity={initial.opacity} {...props}>{children}</g>;
}
