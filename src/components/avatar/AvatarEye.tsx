import { motion, useTransform, type MotionValue } from "motion/react";

export function eyeContour(open: number, happy: number, sad: number) {
    const w = 26.5, h = Math.max(1.5, 26.5 * open);
    const top = -h * (1 - sad * 1.35);
    const bottom = h * (1 - happy * 1.65);
    return `M${-w} 0C${-w} ${top} -12 ${top} 0 ${top}C12 ${top} ${w} ${top} ${w} 0C${w} ${bottom} 12 ${bottom} 0 ${bottom}C-12 ${bottom} ${-w} ${bottom} ${-w} 0Z`;
}
export default function AvatarEye({ side, open, happy, sad, yaw, tilt, blink, forceSmile, color }: {
    side: number; open: MotionValue<number>; happy: MotionValue<number>; sad: MotionValue<number>; yaw: MotionValue<number>; tilt: MotionValue<number>;
    blink: boolean; forceSmile: boolean; color: string;
}) {
    const rgb = [1, 3, 5].map(offset => parseInt(color.slice(offset, offset + 2), 16) / 255);
    const brightness = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
    const ink = brightness < 0.48 ? "#F5F7FF" : "#111A2D";
    const path = useTransform(() => eyeContour(open.get(), forceSmile ? 1 : happy.get(), sad.get()));
    const width = useTransform(() => Math.max(0.35, Math.cos((yaw.get()+side*20)*Math.PI/180)/Math.cos(20*Math.PI/180)));
    const rotation = useTransform(() => side * tilt.get() * -18);
    return <g transform={`translate(${side*44} 0)`}>
        <motion.g style={{ scaleX: width, rotate: rotation, transformOrigin: "0px 0px" }}>
            <motion.path d={path} fill={ink} stroke={color} initial={false}
                animate={{ scaleY: blink ? [1,0.04,1] : 1 }} style={{ transformOrigin: "0px 0px" }} transition={{ duration: blink ? 0.28 : 0.1 }} />
        </motion.g>
    </g>;
}
