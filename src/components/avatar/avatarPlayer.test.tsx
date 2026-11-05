import { describe, expect, it } from "vitest";
import { AvatarPlayer, bezierEase, sampleClip } from "./avatarPlayer";
import { avatarClips, avatarRecipes, type RecipeState } from "./avatarRecipes";
import { BODY_RADIUS, coreMotion, morphKinds, morphRadius } from "./avatarMorphs";

const sample0 = (state: RecipeState) => avatarRecipes[state].effect ?? "none";

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
                if (frame % 60 === 0) signature.push(sample.pose.y, sample.pose.roll, sample.pose.left, sample.amount, sample.morph.core.dy, sample.morph.core.sy);
            }
            signatures.add(sample0(state) + JSON.stringify(signature.map(value => Math.round(value*100))));
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

    it("should transition one-shot states and run functional morphs at every size", () => {
        const player = new AvatarPlayer("waking");
        for (let i=0;i<180;i++) player.advance(1/60);
        expect(player.state).toBe("idle");
        player.setState("thinking");
        let frame = player.advance(0, true);
        for (let i=0;i<120;i++) frame = player.advance(1/60,true);
        expect(frame.morph.kind).toBe("dots");
        expect(frame.morph.amount).toBeGreaterThan(0.95);
    });

    it("should shrink the body in place into the core and turn exactly once around", () => {
        const player = new AvatarPlayer("idle", 3);
        for (let i=0;i<60;i++) player.advance(1/60);
        player.setState("waiting");
        let frame = player.advance(0);
        let maxDrift = 0;
        for (let i=0;i<120;i++) { frame = player.advance(1/60); maxDrift = Math.max(maxDrift, Math.abs(frame.morph.core.dy), Math.abs(frame.pose.y)); }
        expect(frame.morph.core.scale).toBeCloseTo(morphRadius.orbit / BODY_RADIUS, 2);
        // 核心原地缩小，不再掉到底部
        expect(maxDrift).toBeLessThan(15);
        expect(Math.abs(frame.morph.spin)).toBeCloseTo(Math.PI, 1);
        const entry = Math.sign(frame.morph.spin);
        player.setState("idle");
        for (let i=0;i<120;i++) frame = player.advance(1/60);
        // 退场沿同方向再转半圈：一整圈，不回卷
        expect(frame.morph.spin * entry).toBeCloseTo(2*Math.PI, 1);
        expect(frame.morph.kind).toBeNull();
        expect(frame.morph.core.scale).toBeCloseTo(1, 2);
    });

    it("should crossfade between decorations without passing through the full body", () => {
        const player = new AvatarPlayer("thinking", 9);
        for (let i=0;i<90;i++) player.advance(1/60);
        player.setState("searching");
        let frame = player.advance(1/60);
        expect(frame.morph.outgoing).toBe("dots");
        expect(frame.morph.kind).toBe("radar");
        let minAmount = 1;
        for (let i=0;i<90;i++) { frame = player.advance(1/60); minAmount = Math.min(minAmount, frame.morph.amount); }
        expect(minAmount).toBeGreaterThan(0.95);
        expect(frame.morph.outgoing).toBeNull();
        expect(frame.morph.swap).toBeGreaterThan(0.99);
    });

    it("should keep every morph core finite and inside the frame", () => {
        for (const kind of morphKinds) for (let t = 0; t < 6; t += 1/30) {
            const core = coreMotion(kind, t);
            for (const value of Object.values(core)) expect(Number.isFinite(value), `${kind}@${t}`).toBe(true);
            expect(Math.abs(core.dx) + morphRadius[kind]).toBeLessThan(255);
            expect(Math.abs(core.dy) + morphRadius[kind]*core.sy).toBeLessThan(255);
            expect(core.opacity).toBeGreaterThanOrEqual(0);
        }
    });
});
