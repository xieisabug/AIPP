import type { AccessoryId } from "./avatarDefinition";
import { SvgPose } from "./SvgPose";

/** 编辑器里装饰缩略图的取景框。 */
export const accessoryPreviewViewBox: Record<AccessoryId, string> = {
    "round-glasses": "-146 -65 292 130",
    sunglasses: "-132 -62 264 124",
    "thick-brows": "-94 -91 188 102",
    blush: "-95 -20 190 116",
    "baseball-cap": "-125 -108 312 150",
    "baseball-cap-left": "-187 -108 312 150",
    crown: "-116 -103 232 138",
    headphones: "-244 -44 488 266",
    mustache: "-104 25 208 75",
    bandage: "-57 0 114 71",
    eyepatch: "-129 -75 258 137",
    rotor: "-151 -102 302 125",
    sprout: "-72 -103 150 120",
    nightcap: "-111 -102 250 211",
    beret: "-140 -100 280 140",
    "bow-bell": "-76 -52 152 136",
    flower: "-38 -38 80 76",
};

export function GlassesArtwork() {
    return <g transform="scale(1.28)" fill="none" stroke="#253449" strokeWidth="9" strokeLinecap="round" strokeLinejoin="round">
        <path d="M-101-10L-83-7M83-7L101-10M-12-5Q0-14 12-5" />
        <rect x="-82" y="-34" width="70" height="62" rx="25" fill="#B5DBF5" fillOpacity="0.2" />
        <rect x="12" y="-34" width="70" height="62" rx="25" fill="#B5DBF5" fillOpacity="0.2" />
        <path d="M-67-20L-54-25M27-20L40-25" stroke="white" strokeWidth="5" opacity="0.75" />
    </g>;
}

const sunLens = "M-100-34H-9Q0-34-2-24L-8 14Q-14 42-52 42Q-90 42-98 16L-106-22Q-108-34-100-34Z";
/** 墨镜：大号不透明深色镜片，戴着时完全遮住眼睛。 */
export function SunglassesArtwork() {
    return <g strokeLinecap="round" strokeLinejoin="round">
        <path d="M-106-26L-123-30M106-26L123-30M-4-25Q0-31 4-25" fill="none" stroke="#151B26" strokeWidth="8" />
        {[1, -1].map(side => <g key={side} transform={`scale(${side} 1)`}>
            <path d={sunLens} fill="#1E2633" stroke="#151B26" strokeWidth="7" />
            <path d="M-88-20L-70-20M-92-7L-83-7" stroke="white" strokeWidth="6" opacity="0.45" />
        </g>)}
    </g>;
}

/** 两条眉毛按双眼中心定位，保持水平等宽；浅色细边让深色脸上也能看清。 */
export function ThickBrowsArtwork({ raised = false }: { raised?: boolean }) {
    return <g fill="#302D35" stroke="#E6DFDC" strokeWidth="3" strokeLinejoin="round">
        {[-1, 1].map(side => <g key={side} transform={`translate(${side * 44} -54)`}>
            <SvgPose pose={{ y: raised && side === 1 ? [0, -12, -12, 0] : 0 }}
                transition={{ duration: raised ? 1 : 0.15, ease: "easeInOut" }}>
                <rect x="-31" y="-14" width="62" height="28" rx="7" />
            </SvgPose>
        </g>)}
    </g>;
}

/** 腮红：静态的两团红晕。 */
export function BlushArtwork() {
    return <g>
        {[-1, 1].map(side => <g key={side} transform={`translate(${side * 60} 40)`} opacity="0.6">
            <ellipse rx="23" ry="12" fill="#FF8FA3" />
            <path d="M-11-5L-16 5M-1-5L-6 5M9-5L4 5" stroke="#E25E78" strokeWidth="3.5" strokeLinecap="round" opacity="0.7" />
        </g>)}
    </g>;
}

export function BaseballCapArtwork({ reversed = false }: { reversed?: boolean }) {
    return <g transform={reversed ? "scale(-1 1) rotate(-6)" : "rotate(-6)"} strokeLinecap="round" strokeLinejoin="round">
        <path d="M-110 8C-111-46-81-88-28-91C30-96 69-64 82-12L77 7Q-12 27-110 8Z"
            fill="#28384A" stroke="#8191A2" strokeWidth="4" />
        <path d="M-28-88Q4-68 9-9M-32-88Q-70-62-72 2" fill="none" stroke="#536779" strokeWidth="4" />
        <ellipse cx="-29" cy="-91" rx="10" ry="5" fill="#8191A2" />
        <path d="M23-37L38-54L51-47L30-28Z" fill="#E1E9ED" />
        <path d="M-101 6Q-11 14 59-10Q116-24 167-5Q178 1 165 7Q139 14 116 11Q83 7 48 17Q-25 36-101 16Z"
            fill="#172536" stroke="#8191A2" strokeWidth="4" />
        <path d="M62-4Q111-16 149-7" fill="none" stroke="#ACBDC8" strokeWidth="3" />
    </g>;
}

export function CrownArtwork() {
    return <g transform="scale(0.86 1)" stroke="#915F23" strokeWidth="5" strokeLinejoin="round">
        <path d="M-99-57L-52-30L-4-83Q0-88 4-83L52-30L99-57L82 11Q0 28-82 11Z" fill="#F6C34E" />
        <path d="M-83-2Q0 12 83-2L80 17Q0 31-80 17Z" fill="#E8A637" />
        <path d="M0-47L14-28L0-10L-14-28Z" fill="#AE4362" stroke="#FFE39A" />
        {[-99, 0, 99].map(x => <circle key={x} cx={x} cy={x ? -59 : -85} r="7" fill="#FFE39A" />)}
    </g>;
}

export function HeadphonesArtwork() {
    const band = "M-201 141C-201 34-149-14-56-18Q0-21 56-18C149-14 201 34 201 141";
    return <g strokeLinecap="round" strokeLinejoin="round">
        <path d={band} fill="none" stroke="#8494A7" strokeWidth="26" />
        <path d={band} fill="none" stroke="#273346" strokeWidth="17" />
        {[-1, 1].map(side => <g key={side} transform={`scale(${side} 1) rotate(7 201 155)`}>
            <rect x="174" y="100" width="35" height="103" rx="16" fill="#182436" stroke="#8291A0" strokeWidth="5" />
            <rect x="192" y="107" width="36" height="88" rx="17" fill="#465C74" stroke="#A4B4C4" strokeWidth="5" />
            <path d="M214 127V161" fill="none" stroke="#B5DAE8" strokeWidth="6" />
        </g>)}
    </g>;
}

export function HatArtwork() {
    return <g>
        <path d="M-112 5C-133-43-62-75 21-70C88-66 135-36 110 0L-112 5Z" fill="#364D6C" />
        <path d="M-106-5Q0 16 108-10L101 16Q0 35-101 20Z" fill="#24354D" />
        <path d="M13-69L20-86" stroke="#24354D" strokeWidth="13" strokeLinecap="round" />
        <path d="M-82-32Q-50-53-8-51" fill="none" stroke="#6F86A3" strokeWidth="7" strokeLinecap="round" />
    </g>;
}

/**
 * 铃铛蝴蝶结发卡：蝴蝶结与形象亮色同色，结下垂一根丝带，铃铛系在丝带末端。
 * `tilt` 是发卡本身的倾斜角，丝带反向转回去，保证静止时竖直下垂。
 * 摇头时丝带绕结心摆动，铃铛跟在后面晚一拍再晃。
 */
export function BowBellArtwork({ color, outline, tilt = 0, playing = false }: { color: string; outline: string; tilt?: number; playing?: boolean }) {
    return <g strokeLinecap="round" strokeLinejoin="round">
        <g fill={color} stroke={outline} strokeWidth="4.5">
            <path d="M-5 8L-27 40L-14 37L-9 48L2 12Z" />
            <path d="M5 8L27 40L14 37L9 48L-2 12Z" />
        </g>
        <g transform={`rotate(${-tilt})`}>
            <SvgPose pose={{ rotate: playing ? [0, 30, -26, 22, -16, 9, -4, 0] : 0 }}
                transition={{ duration: playing ? 1.7 : 0.2, delay: playing ? 0.06 : 0, ease: "easeInOut" }}>
                <path d="M0 4V40" stroke={outline} strokeWidth="11" />
                <path d="M0 4V40" stroke={color} strokeWidth="5" />
                <g transform="translate(0 40)">
                    <SvgPose pose={{ rotate: playing ? [0, -16, 20, -16, 12, -8, 4, 0] : 0 }}
                        transition={{ duration: playing ? 1.7 : 0.2, delay: playing ? 0.16 : 0, ease: "easeInOut" }}>
                        <circle cy="4" r="4.5" fill="none" stroke="#B7862A" strokeWidth="3.5" />
                        <circle cy="20" r="13" fill="#F6C744" stroke="#B7862A" strokeWidth="4" />
                        <path d="M-8 23H8" stroke="#8A6418" strokeWidth="3.5" />
                        <circle cy="28" r="3" fill="#8A6418" />
                        <path d="M-7 13Q-3 9 1 10" fill="none" stroke="white" strokeWidth="3" opacity="0.8" />
                        {playing && [1, -1].map(side => <g key={side} transform={`scale(${side} 1)`}>
                            <SvgPose pose={{ opacity: [0, 1, 0, 1, 0] }} transition={{ duration: 1.6, delay: side > 0 ? 0.1 : 0.3 }}>
                                <path d="M19 12Q26 20 19 28M27 7Q37 20 27 33" fill="none" stroke="#E0A92E" strokeWidth="3.5" />
                            </SvgPose>
                        </g>)}
                    </SvgPose>
                </g>
            </SvgPose>
        </g>
        <path d="M-8-6C-26-38-66-36-64-4C-62 26-28 26-8 6Z" fill={color} stroke={outline} strokeWidth="4.5" />
        <path d="M8-6C26-38 66-36 64-4C62 26 28 26 8 6Z" fill={color} stroke={outline} strokeWidth="4.5" />
        <path d="M-15-2Q-36-14-52-6M15-2Q36-14 52-6" fill="none" stroke={outline} strokeWidth="3.5" opacity="0.45" />
        <rect x="-12" y="-13" width="24" height="26" rx="9" fill={color} stroke={outline} strokeWidth="4.5" />
    </g>;
}

const petals = [0, 72, 144, 216, 288].map(angle => ({ angle, x: Math.sin(angle * Math.PI / 180) * 17, y: -Math.cos(angle * Math.PI / 180) * 17 }));

/** 蝴蝶：原点在身体中心，翅膀以身体为轴扇动。 */
function Butterfly() {
    return <g strokeLinecap="round" strokeLinejoin="round">
        <SvgPose pose={{ scaleX: [1, 0.3, 1] }} transition={{ duration: 0.24, repeat: Infinity, ease: "easeInOut" }}>
            {[-1, 1].map(side => <g key={side} transform={`scale(${side} 1)`} fill="#B9D3FF" stroke="#5B7FD0" strokeWidth="3">
                <path d="M2-3C10-26 34-28 34-12C34 0 18 4 2 1Z" />
                <path d="M2 2C14 4 26 10 22 22C18 30 6 22 2 8Z" />
                <circle cx="20" cy="-12" r="4" fill="#FFFFFF" stroke="none" opacity="0.8" />
            </g>)}
        </SvgPose>
        <path d="M0-10V16" stroke="#2B3445" strokeWidth="5" />
        <path d="M-1-10Q-6-22-12-24M1-10Q6-22 12-24" fill="none" stroke="#2B3445" strokeWidth="2.5" />
    </g>;
}

/** 小花发卡：原点在花心。播放时一只蝴蝶飞来停在花上，扇几下翅膀再飞走。 */
export function FlowerArtwork({ playing = false }: { playing?: boolean }) {
    return <g>
        <SvgPose pose={playing ? { scale: [1, 1, 0.9, 1.06, 1, 1], rotate: [0, 0, -6, 4, 0, 0] } : { scale: 1, rotate: 0 }}
            transition={{ duration: playing ? 3.4 : 0.2, times: playing ? [0, 0.42, 0.47, 0.54, 0.6, 1] : undefined, ease: "easeInOut" }}>
            <path d="M6 14C20 18 34 14 40 4C28 0 14 4 6 14Z" fill="#7CCB6F" stroke="#4E9E55" strokeWidth="3" strokeLinejoin="round" />
            {petals.map(petal => <circle key={petal.angle} cx={petal.x} cy={petal.y} r="13" fill="#FFC2D3" stroke="#E88AA5" strokeWidth="3" />)}
            <circle r="10" fill="#FFC93C" stroke="#E0A92E" strokeWidth="3" />
            <circle cx="-3" cy="-3" r="3" fill="white" opacity="0.7" />
        </SvgPose>
        {playing && <SvgPose
            pose={{
                x: [220, 150, 80, 40, 6, 6, 6, -60, -200],
                y: [-120, -170, -90, -70, -32, -32, -32, -130, -180],
                rotate: [-20, -10, -25, -15, 0, -6, 0, 15, 25],
                opacity: [0, 1, 1, 1, 1, 1, 1, 1, 0],
            }}
            transition={{ duration: 3.4, times: [0, 0.12, 0.26, 0.36, 0.45, 0.6, 0.72, 0.86, 1], ease: "easeInOut" }}>
            <Butterfly />
        </SvgPose>}
    </g>;
}
