import { motion, useTransform, type MotionValue } from "motion/react";
import { SvgPose } from "./SvgPose";

export function eyeContour(open: number, happy: number, sad: number) {
    const w = 26.5, h = Math.max(1.5, 26.5 * open);
    const top = -h * (1 - sad * 1.35);
    const bottom = h * (1 - happy * 1.65);
    return `M${-w} 0C${-w} ${top} -12 ${top} 0 ${top}C12 ${top} ${w} ${top} ${w} 0C${w} ${bottom} 12 ${bottom} 0 ${bottom}C-12 ${bottom} ${-w} ${bottom} ${-w} 0Z`;
}
/** `squint`：抬墨镜时眯眼细看；`squeeze`：摇头时眼睛挤成 > <。 */
export default function AvatarEye({ side, open, happy, sad, yaw, tilt, blink, forceSmile, color, inkColor, scale = 1, squint = false, squeeze = false }: {
    side: number; open: MotionValue<number>; happy: MotionValue<number>; sad: MotionValue<number>; yaw: MotionValue<number>; tilt: MotionValue<number>;
    blink: boolean; forceSmile: boolean; color: string; inkColor?: string; scale?: number; squint?: boolean; squeeze?: boolean;
}) {
    const rgb = [1, 3, 5].map(offset => parseInt(color.slice(offset, offset + 2), 16) / 255);
    const brightness = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
    const ink = inkColor ?? (color === "#000000" ? "#FFFFFF" : brightness < 0.48 ? "#F5F7FF" : "#111A2D");
    const path = useTransform(() => eyeContour(open.get(), forceSmile ? 1 : happy.get(), sad.get()));
    const width = useTransform(() => Math.max(0.35, Math.cos((yaw.get()+side*20)*Math.PI/180)/Math.cos(20*Math.PI/180)));
    const rotation = useTransform(() => side * tilt.get() * -18);
    return <g transform={`translate(${side*44} 0) scale(${scale})`}>
        <motion.g style={{ scaleX: width, rotate: rotation, transformOrigin: "0px 0px" }}>
            {squeeze
                ? <path d={`M${side*18}-17L${-side*15} 0L${side*18} 17`} fill="none" stroke={ink} strokeWidth="10" strokeLinecap="round" strokeLinejoin="round" />
                : <SvgPose pose={{ scaleY: blink ? [1, 0.04, 1] : squint ? [1, 1, 0.3, 0.4, 0.28, 0.32, 1] : 1 }}
                    transition={blink ? { duration: 0.28 } : squint ? { duration: 3, times: [0, 0.22, 0.34, 0.48, 0.62, 0.8, 0.94] } : { duration: 0.1 }}>
                    <motion.path d={path} fill={ink} stroke={color} />
                </SvgPose>}
        </motion.g>
    </g>;
}
