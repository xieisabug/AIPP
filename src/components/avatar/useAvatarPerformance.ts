import { useEffect, useMemo, useRef, useState } from "react";
import { motionValue } from "motion/react";
import { AvatarPlayer } from "./avatarPlayer";
import { avatarRecipes, restPose, resolveRecipeState, type AvatarPose, type AvatarState, type StateRecipe } from "./avatarRecipes";

export function useAvatarPerformance(state: AvatarState, active: boolean, compact: boolean, seed: string) {
    const values = useMemo(() => Object.fromEntries(Object.entries(restPose).map(([key, value]) => [key, motionValue(value)])) as { [K in keyof AvatarPose]: ReturnType<typeof motionValue<number>> }, []);
    const time = useMemo(() => motionValue(0), []);
    const amount = useMemo(() => motionValue(0), []);
    const player = useRef<AvatarPlayer | null>(null);
    if (!player.current) player.current = new AvatarPlayer(state, [...seed].reduce((sum, char) => sum * 31 + char.charCodeAt(0) | 0, 7));
    const [playingState, setPlayingState] = useState(resolveRecipeState(state));
    useEffect(() => { player.current!.setState(state); setPlayingState(resolveRecipeState(state)); }, [state]);
    useEffect(() => {
        if (!active) {
            for (const key of Object.keys(restPose) as (keyof AvatarPose)[]) values[key].set(restPose[key]);
            amount.set(0);
            return;
        }
        let frame = 0, last = 0, lastState = player.current!.state;
        const tick = (now: number) => {
            const sampled = player.current!.advance(last ? (now - last) / 1000 : 0, compact);
            last = now;
            for (const key of Object.keys(restPose) as (keyof AvatarPose)[]) values[key].set(sampled.pose[key]);
            amount.set(sampled.amount); time.set(sampled.time);
            if (sampled.state !== lastState) { lastState = sampled.state; setPlayingState(sampled.state); }
            frame = requestAnimationFrame(tick);
        };
        frame = requestAnimationFrame(tick);
        return () => cancelAnimationFrame(frame);
    }, [active, compact, values, amount, time]);
    const recipe: StateRecipe = avatarRecipes[playingState];
    return { values, amount, time, effect: recipe.effect ?? "none", playingState };
}
