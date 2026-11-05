import { useEffect, useMemo, useRef, useState } from "react";
import { motionValue } from "motion/react";
import { AvatarPlayer } from "./avatarPlayer";
import type { MorphKind } from "./avatarMorphs";
import { avatarRecipes, restPose, resolveRecipeState, type AvatarPose, type AvatarState, type StateRecipe } from "./avatarRecipes";

const morphKeys = ["amount", "swap", "spin", "kindTime", "outgoingTime", "scale", "dx", "dy", "sx", "sy", "opacity"] as const;
const morphRest: Record<(typeof morphKeys)[number], number> = { amount: 0, swap: 1, spin: 0, kindTime: 0, outgoingTime: 0, scale: 1, dx: 0, dy: 0, sx: 1, sy: 1, opacity: 1 };
type Values<K extends string> = { [P in K]: ReturnType<typeof motionValue<number>> };

export function useAvatarPerformance(state: AvatarState, active: boolean, compact: boolean, seed: string) {
    const values = useMemo(() => Object.fromEntries(Object.entries(restPose).map(([key, value]) => [key, motionValue(value)])) as Values<keyof AvatarPose>, []);
    const morph = useMemo(() => Object.fromEntries(morphKeys.map(key => [key, motionValue(morphRest[key])])) as Values<(typeof morphKeys)[number]>, []);
    const time = useMemo(() => motionValue(0), []);
    const player = useRef<AvatarPlayer | null>(null);
    if (!player.current) player.current = new AvatarPlayer(state, [...seed].reduce((sum, char) => sum * 31 + char.charCodeAt(0) | 0, 7));
    const [playingState, setPlayingState] = useState(resolveRecipeState(state));
    // 装饰种类只在切换时变化，用 React state 驱动图元结构；逐帧量走 motion value。
    const [kinds, setKinds] = useState<{ kind: MorphKind | null; outgoing: MorphKind | null }>({ kind: null, outgoing: null });
    useEffect(() => { player.current!.setState(state); setPlayingState(resolveRecipeState(state)); }, [state]);
    useEffect(() => {
        if (!active) {
            for (const key of Object.keys(restPose) as (keyof AvatarPose)[]) values[key].set(restPose[key]);
            for (const key of morphKeys) morph[key].set(morphRest[key]);
            setKinds({ kind: null, outgoing: null });
            return;
        }
        let frame = 0, last = 0, lastState = player.current!.state;
        let lastKind: MorphKind | null = null, lastOutgoing: MorphKind | null = null;
        const tick = (now: number) => {
            const sampled = player.current!.advance(last ? (now - last) / 1000 : 0, compact);
            last = now;
            for (const key of Object.keys(restPose) as (keyof AvatarPose)[]) values[key].set(sampled.pose[key]);
            const m = sampled.morph;
            morph.amount.set(m.amount); morph.swap.set(m.swap); morph.spin.set(m.spin);
            morph.kindTime.set(m.kindTime); morph.outgoingTime.set(m.outgoingTime);
            for (const key of ["scale", "dx", "dy", "sx", "sy", "opacity"] as const) morph[key].set(m.core[key]);
            time.set(sampled.time);
            if (m.kind !== lastKind || m.outgoing !== lastOutgoing) {
                lastKind = m.kind; lastOutgoing = m.outgoing;
                setKinds({ kind: m.kind, outgoing: m.outgoing });
            }
            if (sampled.state !== lastState) { lastState = sampled.state; setPlayingState(sampled.state); }
            frame = requestAnimationFrame(tick);
        };
        frame = requestAnimationFrame(tick);
        return () => cancelAnimationFrame(frame);
    }, [active, compact, values, morph, time]);
    const recipe: StateRecipe = avatarRecipes[playingState];
    return { values, morph, time, ...kinds, overlay: recipe.effect === "confetti" ? "confetti" as const : null, playingState };
}
