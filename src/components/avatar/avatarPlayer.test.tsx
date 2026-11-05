import { describe, expect, it } from "vitest";
import { AvatarPlayer, bezierEase, sampleClip } from "./avatarPlayer";
import { avatarClips, avatarRecipes, type RecipeState } from "./avatarRecipes";

describe("avatar choreography player", () => {
    it("should produce finite distinct performances across the complete state catalogue", () => {
        expect(Object.keys(avatarRecipes)).toHaveLength(53);
        expect(Object.keys(avatarClips)).toHaveLength(28);
        const signatures = new Set<string>();
        for (const state of Object.keys(avatarRecipes) as RecipeState[]) {
            const player = new AvatarPlayer(state, 1942);
            const signature: number[] = [];
            for (let frame = 0; frame < 720; frame++) {
                const sample = player.advance(1 / 60);
                for (const value of Object.values(sample.pose)) expect(Number.isFinite(value), `${state} frame ${frame}`).toBe(true);
                expect(sample.pose.sx).toBeGreaterThan(0.5);
                expect(sample.pose.sy).toBeGreaterThan(0.5);
                if (frame % 60 === 0) signature.push(sample.pose.y, sample.pose.roll, sample.pose.left, sample.amount);
            }
            signatures.add(JSON.stringify(signature.map(value => Math.round(value*100))));
        }
        expect(signatures.size).toBeGreaterThan(45);
    });

    it("should repeat deterministically with the same seed and vary idle clips", () => {
        const a = new AvatarPlayer("idle", 55), b = new AvatarPlayer("idle", 55);
        const clips = new Set<string>();
        for (let i = 0; i < 2400; i++) {
            const first = a.advance(1/60), second = b.advance(1/60);
            expect(first).toEqual(second); clips.add(first.clip);
        }
        expect(clips.size).toBeGreaterThanOrEqual(3);
    });

    it("should bridge from the current pose when interrupted mid-performance", () => {
        const player = new AvatarPlayer("excited", 7);
        for (let i = 0; i < 50; i++) player.advance(1/60);
        const before = { ...player.pose };
        player.setState("sad");
        const start = player.advance(0).pose;
        expect(start).toEqual(before);
        const next = player.advance(1/60).pose;
        expect(Math.abs(next.y-before.y)).toBeLessThan(10);
        expect(Math.abs(next.roll-before.roll)).toBeLessThan(10);
    });

    it("should close eyelids at embedded blink times and preserve easing overshoot", () => {
        const blink = avatarClips.doubleBlink.blinks[0];
        expect(sampleClip("doubleBlink", blink+0.28*0.43).left).toBeLessThan(0.08);
        expect(sampleClip("doubleBlink", blink+0.35).left).toBeGreaterThan(0.5);
        const values = Array.from({length:101},(_,i)=>bezierEase(i/100,[0.3,-0.22,0.22,1.24]));
        expect(Math.min(...values)).toBeLessThan(0);
        expect(Math.max(...values)).toBeGreaterThan(1);
    });

    it("should transition one-shot states and suppress functional morphs at compact sizes", () => {
        const player = new AvatarPlayer("waking");
        for (let i=0;i<180;i++) player.advance(1/60);
        expect(player.state).toBe("idle");
        player.setState("thinking");
        for (let i=0;i<120;i++) player.advance(1/60,true);
        expect(player.amount).toBe(0);
        for (let i=0;i<120;i++) player.advance(1/60,false);
        expect(player.amount).toBeGreaterThan(0.95);
    });
});
