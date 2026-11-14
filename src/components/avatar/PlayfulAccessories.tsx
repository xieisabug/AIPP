import { SvgPose } from "./SvgPose";

export function MustacheArtwork({ playing = false }) {
    return <g transform="translate(0 60) scale(1.45)" fill="#342C33" stroke="#E8DECF" strokeWidth="3" strokeLinejoin="round">
        {[-1, 1].map(side => <g key={side} transform={`scale(${side} 1)`}>
            <SvgPose pose={{ rotate: playing ? [0, -14, 5, -8, 0] : 0 }}
                transition={{ duration: 0.82, delay: playing && side > 0 ? 0.19 : 0 }}>
                <path d="M0 0C9-17 21-15 35-6C49 3 61-2 63-15C72-2 61 18 40 18C24 18 10 10 0 3Z" />
            </SvgPose>
        </g>)}
    </g>;
}

export function BandageArtwork() {
    return <g transform="translate(0 35) rotate(-16) scale(1.55)">
        <rect x="-30" y="-11" width="60" height="22" rx="8" fill="#E7B58F" stroke="#996E56" strokeWidth="3" />
        <rect x="-10" y="-9" width="20" height="18" rx="3" fill="#FFE5C7" />
        {[-20, 20].map(x => <g key={x} fill="#AC7C5D"><circle cx={x} cy="-4" r="2" /><circle cx={x} cy="4" r="2" /></g>)}
    </g>;
}

export function EyepatchStrap({ reach = 120 }: { reach?: number }) {
    // Both halves disappear under the patch. On the avatar the silhouette trims the ends.
    const strap = `M${-reach} ${-42 - reach * 0.18} Q-45 -61 43 0 Q75 26 ${reach} ${20 + reach * 0.11}`;
    return <g fill="none" strokeLinecap="round">
        <path d={strap} stroke="#89919C" strokeWidth="12" />
        <path d={strap} stroke="#292E39" strokeWidth="8" />
    </g>;
}

export function EyepatchArtwork() {
    return <g transform="translate(44 0) scale(-1.26 1.26) translate(44 0)" strokeLinejoin="round" strokeLinecap="round">
        <path d="M-82-28Q-48-42-9-22L-13 6Q-18 35-45 36Q-73 34-78 10Z" fill="#282C38" stroke="#98A0AA" strokeWidth="4" />
        <path d="M-70-18Q-49-26-31-21" fill="none" stroke="#566070" strokeWidth="4" />
    </g>;
}

export function RotorArtwork({ playing = false }) {
    return <g transform="scale(1.4 1.22)" strokeLinejoin="round" strokeLinecap="round">
        <path d="M0 8V-51" stroke="#88683B" strokeWidth="10" />
        <g transform="translate(0 -48)">
            <SvgPose pose={{ scaleX: playing ? [1, 0.1, -1, 0.1, 1, -1, 1, -1, 1, -1, 1, 1] : 1 }}
                transition={{ duration: 1.8, times: [0, .17, .31, .4, .48, .55, .61, .67, .73, .79, .85, 1] }}>
                <path d="M-3-2Q-38-28-90-21Q-104-9-89-4Q-43 6-3 4Z" fill="#82BC96" stroke="#416B56" strokeWidth="4" />
                <path d="M3 2Q38 28 90 21Q104 9 89 4Q43-6 3-4Z" fill="#E2AD65" stroke="#95663E" strokeWidth="4" />
            </SvgPose>
            <circle r="9" fill="#FADE91" stroke="#88683B" strokeWidth="4" />
        </g>
    </g>;
}

export function SproutArtwork({ playing = false }) {
    return <SvgPose pose={{ rotate: playing ? [0, -10, 18, -12, 7, -3, 0] : 0 }} transition={{ duration: 1.5 }}>
        <path d="M0 5Q6-31 0-56" fill="none" stroke="#48724B" strokeWidth="8" strokeLinecap="round" />
        <path d="M3-34Q-48-27-59-69Q-16-83 3-34Z" fill="#95C76F" stroke="#4D824E" strokeWidth="4" />
        <path d="M1-48Q8-93 65-91Q61-49 1-48Z" fill="#71B98C" stroke="#427958" strokeWidth="4" />
        <path d="M-40-60L-5-39M8-55L46-78" fill="none" stroke="#DAEABF" strokeWidth="3" strokeLinecap="round" />
    </SvgPose>;
}

export function NightcapArtwork({ playing = false }) {
    return <g strokeLinejoin="round">
        <path d="M-97 4Q-78-69-20-77Q47-95 83-43Q104-14 106 16L77 13Q55-28 32-28L61 3Z" fill="#7986BC" stroke="#454F7D" strokeWidth="5" />
        <path d="M-44-47l4-9 4 9 10 3-9 5-3 9-6-9-9-4Z" fill="#FFE3A6" />
        <path d="M-97-5Q-26-18 62-6L65 14Q-19 4-96 18Z" fill="#E3E4F1" stroke="#626C99" strokeWidth="5" />
        <g transform="translate(94 9)">
            <SvgPose pose={{ rotate: playing ? [0, -14, 8, 48, -18, 8, 0] : 0 }}
                transition={{ duration: 2, times: [0, .28, .47, .64, .77, .89, 1] }}>
                <path d="M0 0Q12 35 0 67" fill="none" stroke="#65719C" strokeWidth="8" />
                <circle cy="70" r="20" fill="#F5E5D4" stroke="#A19191" strokeWidth="4" />
            </SvgPose>
        </g>
    </g>;
}
