import { useEffect, useId, useRef } from "react";
import { AnimatePresence, motion, useMotionValue, useSpring, useTransform } from "motion/react";
import type { ButlerAppearance } from "@/components/butler/butlerAppearance";
import { avatarDefinitions, type AvatarState } from "./avatarDefinition";
import { GlassesArtwork, HatArtwork } from "./AvatarAccessories";
import { useAvatarMotion, type AvatarActionRequest } from "./useAvatarMotion";
import { useAvatarPerformance } from "./useAvatarPerformance";
import AvatarEye from "./AvatarEye";
import AvatarEffects from "./AvatarEffects";

export interface AnimatedAvatarProps {
    appearance: ButlerAppearance;
    size?: number;
    animated?: boolean;
    state?: AvatarState;
    actionRequest?: AvatarActionRequest;
}

export default function AnimatedAvatar({ appearance, size = 32, animated = false, state = "idle", actionRequest }: AnimatedAvatarProps) {
    const definition = avatarDefinitions[appearance.shape];
    const id = useId().replace(/:/g, "");
    const ref = useRef<HTMLSpanElement>(null);
    const { active, action } = useAvatarMotion(ref, animated && appearance.motion, state, actionRequest, `${appearance.accessories.eyes}-${appearance.accessories.head}`);
    const full = size >= 48;
    const performance = useAvatarPerformance(action ? "idle" : state, active, !full, id);
    const v = performance.values;
    const bodyScaleX = useTransform(() => v.sx.get()*v.zoom.get()*Math.cos(v.yaw.get()*0.45*Math.PI/180)*(1-performance.amount.get()*0.72));
    const bodyScaleY = useTransform(() => v.sy.get()*v.zoom.get()*(1-performance.amount.get()*0.72));
    const bodyY = useTransform(() => v.y.get()+performance.amount.get()*130);
    const bodyRoll = useTransform(() => v.roll.get()+performance.amount.get()*180);
    const eyeOpacity = useTransform(() => 1-performance.amount.get());
    const effectOpacity = useTransform(() => !active || !full ? 0 : performance.effect === "confetti" ? 1 : performance.amount.get());
    const targetX = useMotionValue(0);
    const targetY = useMotionValue(0);
    const gazeX = useSpring(targetX, { stiffness: 150, damping: 22 });
    const gazeY = useSpring(targetY, { stiffness: 150, damping: 22 });
    useEffect(() => {
        if (!active) { targetX.set(0); targetY.set(0); gazeX.jump(0); gazeY.jump(0); }
    }, [active, targetX, targetY, gazeX, gazeY]);
    const faceX = useTransform(() => Math.sin(v.yaw.get()*Math.PI/180)*28 + gazeX.get() + v.gazeX.get());
    const faceY = useTransform(() => v.pitch.get()*0.55 + gazeY.get() + v.gazeY.get());
    const smiling = action === "smile" || action === "glasses";
    const blinking = action === "blink";
    const glasses = appearance.accessories.eyes === "round-glasses" && definition.capabilities.includes("eyes");
    const hat = appearance.accessories.head === "beret" && definition.capabilities.includes("headwear");
    const takingGlasses = active && full && glasses && action === "glasses";
    const tippingHat = active && full && hat && action === "hat";
    const face = definition.face;
    const dock = definition.propDock;
    const glassesPose = takingGlasses ? {
        x: [face.x, face.x + 12, dock.x, dock.x, face.x],
        y: [face.y, face.y - 24, dock.y, dock.y, face.y],
        rotate: [0, -5, dock.rotation ?? -28, dock.rotation ?? -28, 0],
        scale: [face.scale, face.scale * 1.08, dock.scale, dock.scale, face.scale],
    } : { x: face.x, y: face.y, rotate: 0, scale: face.scale };

    return <span ref={ref} aria-hidden="true" data-avatar-shape={definition.id} data-avatar-state={performance.playingState}
        data-avatar-action={action ?? "none"} data-avatar-active={active}
        className="relative inline-flex shrink-0 items-center justify-center" style={{ width: size, height: size }}
        onPointerLeave={() => { targetX.set(0); targetY.set(0); }}
        onPointerMove={event => {
            if (!active || !full) return;
            const rect = event.currentTarget.getBoundingClientRect();
            targetX.set(Math.max(-9, Math.min(9, (event.clientX - rect.left - rect.width / 2) / rect.width * 18)));
            targetY.set(Math.max(-6, Math.min(6, (event.clientY - rect.top - rect.height / 2) / rect.height * 12)));
        }}>
        <svg viewBox={hat || full ? "-55 -100 620 620" : "-12 -12 534 534"} className="h-full w-full overflow-visible" focusable="false">
            <defs>
                <linearGradient id={`${id}-primary`} x1="0" y1="0" x2="0.3" y2="1">
                    <stop stopColor="white" /><stop offset="0.4" stopColor="#F5F5F5" /><stop offset="1" stopColor="#1A2437" />
                </linearGradient>
                <linearGradient id={`${id}-secondary`} x1="0" y1="0" x2="0.8" y2="1">
                    <stop stopColor="white" /><stop offset="1" stopColor="black" />
                </linearGradient>
            </defs>
            <g transform="translate(255 255)">
                <motion.g style={{ x: v.x, y: bodyY, rotate: bodyRoll, scaleX: bodyScaleX, scaleY: bodyScaleY, transformOrigin: "0px 0px" }}>
                    <g transform="translate(-255 -255)">
                        {definition.parts.map(part => <g key={part.color}>
                            <path d={part.path} fill={appearance.colors[part.color]} />
                            <path d={part.path} fill={`url(#${id}-${part.color})`} opacity="0.2" />
                        </g>)}
                        <g transform={`translate(${face.x} ${face.y}) scale(${face.scale})`}>
                            <motion.g style={{ x: faceX, y: faceY, opacity: eyeOpacity }}>
                                {[-1, 1].map(side => <AvatarEye key={side} side={side} open={side < 0 ? v.left : v.right} happy={v.happy} sad={v.sad} yaw={v.yaw} tilt={v.tilt} blink={blinking} forceSmile={smiling} color={appearance.colors.primary} />)}
                            </motion.g>
                        </g>
                        <AnimatePresence initial={false}>
                        {glasses && <motion.g key="glasses" data-avatar-accessory="round-glasses"
                            initial={active && full ? { x: dock.x, y: dock.y, scale: dock.scale, opacity: 0 } : false}
                            animate={{ ...glassesPose, opacity: 1 }} style={{ transformOrigin: "0px 0px" }}
                            exit={{ x: dock.x, y: dock.y, rotate: dock.rotation ?? -28, opacity: 0, transition: { duration: active && full ? 0.4 : 0 } }}
                            transition={{ duration: takingGlasses ? 2.4 : active && full ? 0.35 : 0, ...(takingGlasses ? { times: [0, 0.15, 0.42, 0.68, 1] } : {}), ease: "easeInOut" }}>
                            <GlassesArtwork />
                        </motion.g>}
                        </AnimatePresence>
                        {hat && <g transform={`translate(${definition.crown.x} ${definition.crown.y}) scale(${definition.crown.scale})`}>
                            <motion.g data-avatar-accessory="beret" initial={false}
                                animate={{ y: tippingHat ? [0, -32, -32, 0] : 0, rotate: tippingHat ? [0, -15, -15, 0] : 0 }}
                                style={{ transformOrigin: "0px 0px" }} transition={{ duration: tippingHat ? 1.5 : 0, ease: "easeInOut" }}>
                                <HatArtwork />
                            </motion.g>
                        </g>}
                    </g>
                </motion.g>
            </g>
            <g transform="translate(255 180)">
                <AnimatePresence mode="sync">
                    {performance.effect !== "none" && <motion.g key={performance.effect} initial={{ opacity: 0, scale: 0.6 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.6 }} transition={{ duration: active ? 0.28 : 0 }} style={{ transformOrigin: "0px 0px" }}>
                        <AvatarEffects kind={performance.effect} time={performance.time} opacity={effectOpacity} color="currentColor" />
                    </motion.g>}
                </AnimatePresence>
            </g>
        </svg>
    </span>;
}
