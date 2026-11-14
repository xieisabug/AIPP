import { useEffect, useId, useRef } from "react";
import { AnimatePresence, motion, useMotionValue, useSpring, useTransform } from "motion/react";
import type { ButlerAppearance } from "@/components/butler/butlerAppearance";
import { avatarDefinitions, type AvatarAction, type AvatarState } from "./avatarDefinition";
import { BlushArtwork, BowBellArtwork, GlassesArtwork, HatArtwork, FlowerArtwork, SunglassesArtwork } from "./AvatarAccessories";
import { useAvatarMotion, type AvatarActionRequest } from "./useAvatarMotion";
import { useAvatarPerformance } from "./useAvatarPerformance";
import AvatarEye from "./AvatarEye";
import { SvgPose } from "./SvgPose";
import { ConfettiOverlay, MorphParts } from "./AvatarEffects";

export interface AnimatedAvatarProps {
    appearance: ButlerAppearance;
    size?: number;
    animated?: boolean;
    state?: AvatarState;
    actionRequest?: AvatarActionRequest;
}

const smoothstep = (a: number, b: number, x: number) => { const t = Math.max(0, Math.min(1, (x - a) / (b - a))); return t * t * (3 - 2 * t); };
/** 圆核与装饰的颜色：黑白（品牌默认）形象跟随前景色，保证深浅主题下都看得见；彩色形象沿用外圈色。 */
export const coreColorOf = (appearance: ButlerAppearance) => /^#(000000|ffffff)$/i.test(appearance.colors.secondary) ? "currentColor" : appearance.colors.secondary;

export default function AnimatedAvatar({ appearance, size = 32, animated = false, state = "idle", actionRequest }: AnimatedAvatarProps) {
    const definition = avatarDefinitions[appearance.shape];
    const id = useId().replace(/:/g, "");
    const ref = useRef<HTMLSpanElement>(null);
    const { active, action } = useAvatarMotion(ref, animated && appearance.motion, state, actionRequest, `${appearance.accessories.eyes}-${appearance.accessories.head}`);
    const full = size >= 48;
    const performance = useAvatarPerformance(action ? "idle" : state, active, !full, id);
    const v = performance.values;
    const m = performance.morph;
    const faceItem = definition.capabilities.includes("eyes") ? appearance.accessories.eyes : null;
    const headItem = definition.capabilities.includes("headwear") ? appearance.accessories.head : null;
    const glasses = faceItem === "round-glasses";
    const hat = headItem === "beret";
    // 变形中心对齐可视区域中心：带头饰/大尺寸的 viewBox 整体上移了 45 单位。
    const morphCenterY = headItem || full ? 210 : 255;
    // 身体原地缩成圆核；绕竖轴的转身用 cos(spin) 压缩横向，半程正好侧对观众。
    const bodyScaleX = useTransform(() => v.sx.get()*v.zoom.get()*Math.cos(v.yaw.get()*0.45*Math.PI/180)*m.scale.get()*m.sx.get()*Math.cos(m.spin.get()));
    const bodyScaleY = useTransform(() => v.sy.get()*v.zoom.get()*m.scale.get()*m.sy.get());
    const bodyX = useTransform(() => v.x.get()+m.dx.get());
    const bodyY = useTransform(() => v.y.get()+m.dy.get()+(morphCenterY-255)*m.amount.get());
    // 形象 ⇄ 圆盘在转到侧面（amount≈0.5）时交接，交接瞬间几乎看不见，读起来就是「转身变形」。
    const logoOpacity = useTransform(() => 1-smoothstep(0.4, 0.6, m.amount.get()));
    const discOpacity = useTransform(() => smoothstep(0.4, 0.6, m.amount.get()));
    const eyeOpacity = useTransform(() => Math.max(0, 1-m.amount.get()*2.2));
    const currentWeight = useTransform(() => m.amount.get()*(performance.outgoing ? m.swap.get() : 1));
    const outgoingWeight = useTransform(() => m.amount.get()*(1-m.swap.get()));
    const confettiOpacity = useTransform((): number => active && full ? 1 : 0);
    const targetX = useMotionValue(0);
    const targetY = useMotionValue(0);
    const gazeX = useSpring(targetX, { stiffness: 150, damping: 22 });
    const gazeY = useSpring(targetY, { stiffness: 150, damping: 22 });
    useEffect(() => {
        if (!active) { targetX.set(0); targetY.set(0); gazeX.jump(0); gazeY.jump(0); }
    }, [active, targetX, targetY, gazeX, gazeY]);
    // 转身起步时眼睛先往转向一侧滑，给出方向感（随后眼睛淡出）。
    const faceX = useTransform(() => Math.sin(v.yaw.get()*Math.PI/180)*28 + Math.sin(m.spin.get())*40 + gazeX.get() + v.gazeX.get());
    const faceY = useTransform(() => v.pitch.get()*0.55 + gazeY.get() + v.gazeY.get());
    const performing = (name: AvatarAction, equipped: boolean) => active && full && equipped && action === name;
    const takingGlasses = performing("glasses", glasses);
    const tippingHat = performing("hat", hat);
    const peering = performing("peer", faceItem === "sunglasses");
    const blushing = performing("blush", faceItem === "blush");
    const jingling = performing("jingle", headItem === "bow-bell");
    const fluttering = performing("butterfly", headItem === "flower");
    const smiling = action === "smile" || action === "glasses" || blushing || fluttering;
    const blinking = action === "blink";
    // 装饰动作带动整个身体：摇头晃铃铛、害羞地扭一扭。
    const sway = jingling ? { rotate: [0, -11, 10, -9, 8, -5, 3, 0] } : blushing ? { rotate: [0, -5, 4, -4, 3, 0] } : { rotate: 0 };
    const swayTransition = jingling ? { duration: 1.7, ease: "easeInOut" as const } : blushing ? { duration: 2, ease: "easeInOut" as const } : { duration: 0.25 };
    const face = definition.face;
    const blossom = definition.id === "blossom";
    const codingFace = blossom && active && performance.playingState === "coding";
    const faceColor = blossom ? "#FFFFFF" : definition.colorLabels.primary ? appearance.colors.primary : appearance.colors.secondary;
    const coreColor = coreColorOf(appearance);
    const dock = definition.propDock;
    const glassesPose = takingGlasses ? {
        x: [face.x, face.x + 12, dock.x, dock.x, face.x],
        y: [face.y, face.y - 24, dock.y, dock.y, face.y],
        rotate: [0, -5, dock.rotation ?? -28, dock.rotation ?? -28, 0],
        scale: [face.scale, face.scale * 1.08, dock.scale, dock.scale, face.scale],
    } : { x: face.x, y: face.y, rotate: 0, scale: face.scale };

    const eyes = [-1, 1].map(side => <AvatarEye key={side} side={side} open={side < 0 ? v.left : v.right} happy={v.happy} sad={v.sad} yaw={v.yaw} tilt={v.tilt}
        blink={blinking} forceSmile={smiling} squint={peering} squeeze={jingling} color={faceColor} inkColor={definition.eyeColor} scale={definition.eyeScale} />);
    const eyePair = blossom ? <g opacity={codingFace ? 0 : 1} className="transition-opacity duration-150 motion-reduce:transition-none">{eyes}</g> : eyes;

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
        <svg viewBox={headItem || full ? "-55 -100 620 620" : "-12 -12 534 534"} className="h-full w-full overflow-visible" focusable="false">
            <defs>
                <linearGradient id={`${id}-primary`} x1="0" y1="0" x2="0.3" y2="1">
                    <stop stopColor="white" /><stop offset="0.4" stopColor="#F5F5F5" /><stop offset="1" stopColor="#1A2437" />
                </linearGradient>
                <linearGradient id={`${id}-secondary`} x1="0" y1="0" x2="0.8" y2="1">
                    <stop stopColor="white" /><stop offset="1" stopColor="black" />
                </linearGradient>
            </defs>
            <g transform="translate(255 255)">
                <motion.g style={{ x: bodyX, y: bodyY, rotate: v.roll, scaleX: bodyScaleX, scaleY: bodyScaleY, opacity: m.opacity, transformOrigin: "0px 0px" }}>
                    {performance.kind && <motion.g data-avatar-core="" style={{ opacity: discOpacity }}>
                        <circle r="250" fill={coreColor} />
                    </motion.g>}
                    <motion.g style={{ opacity: logoOpacity }}><g transform="translate(0 140)">
                    <SvgPose pose={sway} transition={swayTransition}><g transform={`translate(0 -140) scale(${definition.artworkScale ?? 1}) translate(-255 -255)`}>
                        {definition.parts.map(part => <g key={part.color}>
                            <path d={part.path} fill={part.color === "white" ? "#FFFFFF" : appearance.colors[part.color]}
                                stroke={part.outlineWidth ? appearance.colors.secondary : undefined} strokeWidth={part.outlineWidth} strokeLinejoin="round" />
                            {definition.colorLabels.primary && <path d={part.path} fill={`url(#${id}-${part.color})`} opacity="0.2" />}
                        </g>)}
                        <g transform={`translate(${face.x} ${face.y}) scale(${face.scale})`}>
                            <motion.g style={{ x: faceX, y: faceY, opacity: eyeOpacity }}>
                                {blossom && <path data-avatar-expression="coding" opacity={codingFace ? 1 : 0}
                                    className="transition-opacity duration-150 motion-reduce:transition-none"
                                    d="M-57-23L-38 0L-57 23M17 21H69" fill="none" stroke={appearance.colors.secondary}
                                    strokeWidth="13" strokeLinecap="round" strokeLinejoin="round" />}
                                {eyePair}
                            </motion.g>
                            {faceItem === "blush" && <g data-avatar-accessory="blush"><BlushArtwork /></g>}
                            {faceItem === "sunglasses" && <SvgPose data-avatar-accessory="sunglasses" pose={peering ? { y: [0, 0, -130, -130, 0], rotate: [0, 0, -5, -5, 0] } : { y: 0, rotate: 0 }}
                                transition={peering ? { duration: 3, times: [0, 0.1, 0.3, 0.8, 0.95], ease: "easeInOut" } : { duration: 0.2 }}>
                                <SunglassesArtwork />
                            </SvgPose>}
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
                        {headItem === "bow-bell" && <g data-avatar-accessory="bow-bell"
                            transform={`translate(${definition.clip.x} ${definition.clip.y}) rotate(${definition.clip.rotation ?? 0}) scale(${definition.clip.scale})`}>
                            <BowBellArtwork color={appearance.colors.primary} outline={coreColor} tilt={definition.clip.rotation} playing={jingling} />
                        </g>}
                        {headItem === "flower" && <g data-avatar-accessory="flower" transform={`translate(${definition.clip.x} ${definition.clip.y}) scale(${definition.clip.scale})`}>
                            <FlowerArtwork playing={fluttering} />
                        </g>}
                    </g></SvgPose>
                    </g></motion.g>
                </motion.g>
            </g>
            {active && <g transform={`translate(255 ${morphCenterY})`}>
                {performance.outgoing && <MorphParts key={`out-${performance.outgoing}`} kind={performance.outgoing} time={m.outgoingTime} weight={outgoingWeight} color={coreColor} />}
                {performance.kind && <MorphParts key={performance.kind} kind={performance.kind} time={m.kindTime} weight={currentWeight} color={coreColor} />}
                {performance.overlay && full && <g transform="translate(0 -30)"><ConfettiOverlay time={performance.time} opacity={confettiOpacity} /></g>}
            </g>}
        </svg>
    </span>;
}
