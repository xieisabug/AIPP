import { SvgPose } from "./SvgPose";
import { whaleMealDuration } from "./avatarDefinition";

// The rim approaches the mouth on each bite as the mound of rice gets shorter.
const beats = [0, 0.19, 0.30, 0.40, 0.50, 0.60, 0.70, 0.82, 1];
const timing = { duration: whaleMealDuration / 1000, times: beats, ease: "easeInOut" as const };

export function WhaleRiceMeal({ bodyColor }: { bodyColor: string }) {
    return <g data-avatar-prop="rice-meal" strokeLinecap="round" strokeLinejoin="round">
        <g transform="translate(122 223)">
            <SvgPose pose={{ scaleY: [0, 0.35, 1, 0.25, 1, 0.25, 1, 0.45, 0] }} transition={timing}>
                <path d="M-15-4Q0 3 15-4Q12 15 1 15Q-11 15-15-4Z" fill={bodyColor} stroke="white" strokeWidth="2" />
                <path d="M-7 9Q0 5 8 9Q1 17-7 9Z" fill="white" />
            </SvgPose>
        </g>
        <SvgPose pose={{
            x: [236, 148, 128, 142, 128, 139, 124, 143, 236],
            y: [369, 357, 337, 325, 310, 298, 282, 272, 355],
            rotate: [18, -5, -8, -3, -8, -3, -8, 0, 18],
            scale: [0.4, 1, 1, 1, 1, 1, 1, 1, 0.4],
            opacity: [0, 1, 1, 1, 1, 1, 1, 1, 0],
        }} transition={timing}>
            <ellipse rx="70" ry="14" fill={bodyColor} stroke="white" strokeWidth="5" />
            <SvgPose pose={{ scaleY: [1, 1, 1, 0.76, 0.76, 0.52, 0.52, 0.22, 0.22] }} transition={timing}>
                <path d="M-69 0Q-75-14-63-25Q-66-39-52-46Q-55-59-40-66Q-42-80-28-86Q-28-99-15-103Q-12-117 0-115Q12-117 17-104Q30-102 30-89Q44-84 42-71Q57-65 54-52Q68-45 64-32Q77-21 69-10L67 0Q0 18-69 0Z"
                    fill="white" stroke={bodyColor} strokeWidth="3" />
                <path d="M-7-99q3-4 7-4M-22-78q2-4 6-5M10-83q4 0 7 4M-40-56q2-5 7-6M-7-60q4-4 8-3M25-55q4 0 7 4M-51-33q3-4 8-5M-19-37q3-3 7-3M7-34q4 0 7 3M41-30q4 0 7 4M-33-16q4-2 8-2M17-15q3-2 7-1"
                    fill="none" stroke={bodyColor} strokeOpacity="0.35" strokeWidth="3" />
            </SvgPose>
            <path d="M-70 0Q0 22 70 0Q62 41 26 51L22 58H-22L-26 51Q-62 41-70 0Z"
                fill={bodyColor} stroke="white" strokeWidth="5" />
            <path d="M-58 13Q0 32 58 13" fill="none" stroke="white" strokeOpacity="0.7" strokeWidth="3" />
            <g fill={bodyColor} stroke="#FFFFFF" strokeWidth="3">
                <path d="M-78 12Q-66 9-44 28Q-39 38-53 37Q-72 34-78 12Z" />
                <path d="M77 10Q65 9 45 27Q39 37 54 35Q73 30 77 10Z" />
            </g>
        </SvgPose>
    </g>;
}
