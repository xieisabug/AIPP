import { avatarClips, avatarRecipes, resolveRecipeState, restPose, type AvatarEffect, type AvatarPose, type AvatarState, type ClipName, type Ease, type RecipeState, type StateRecipe } from "./avatarRecipes";
import { BODY_RADIUS, coreMotion, isMorphKind, morphRadius, restCore, type CoreMotion, type MorphKind } from "./avatarMorphs";

const keys = Object.keys(restPose) as (keyof AvatarPose)[];
// 变形时这些身体姿态量随 amount 淡出（眼睑/表情不受影响，眼睛另行隐藏）。
const bodyKeys: (keyof AvatarPose)[] = ["x", "y", "roll", "yaw", "pitch", "sx", "sy", "zoom", "gazeX", "gazeY"];
/** 半隐式欧拉弹簧，ζ=1（临界阻尼），以 120Hz 固定子步推进。 */
interface Spring { x: number; v: number; t: number }
const stepSpring = (s: Spring, omega: number, dt: number) => { s.v += (-2 * omega * s.v - omega * omega * (s.x - s.t)) * dt; s.x += s.v * dt; };

export interface MorphFrame {
    /** 变形总进度：0 = 正常形象，1 = 完全变成圆核 + 装饰。 */
    amount: number;
    /** 当前装饰与上一个装饰的交叉淡化进度（静止值为 1）。 */
    swap: number;
    /** 进场/退场的绕竖轴转身（弧度，累加不回卷）。 */
    spin: number;
    kind: MorphKind | null;
    outgoing: MorphKind | null;
    kindTime: number;
    outgoingTime: number;
    /** 整个身体组的缩放（含缩成圆核）与圆核的位移/形变，已按权重混合。 */
    core: CoreMotion & { scale: number };
}
const clamp = (x: number, a: number, b: number) => Math.max(a, Math.min(b, x));
export function bezierEase(t: number, [x1, y1, x2, y2]: Ease): number {
    const curve = (u: number, a: number, b: number) => 3 * (1-u) ** 2 * u * a + 3 * (1-u) * u*u * b + u*u*u;
    let lo = 0, hi = 1, u = t;
    for (let i = 0; i < 16; i++) { u = (lo + hi) / 2; if (curve(u, x1, x2) < t) lo = u; else hi = u; }
    return t <= 0 ? 0 : t >= 1 ? 1 : curve(u, y1, y2);
}
export function sampleClip(name: ClipName, seconds: number): AvatarPose {
    const clip = avatarClips[name];
    const t = clamp(seconds, 0, clip.duration);
    let i = 0;
    while (i < clip.frames.length - 2 && clip.frames[i + 1].at <= t) i++;
    const a = clip.frames[i], b = clip.frames[i + 1];
    const mix = bezierEase((t - a.at) / Math.max(0.001, b.at - a.at), b.ease ?? [0.4, 0, 0.6, 1]);
    const pose = { ...restPose };
    for (const key of keys) pose[key] = (a.pose[key] ?? restPose[key]) + ((b.pose[key] ?? restPose[key]) - (a.pose[key] ?? restPose[key])) * mix;
    for (const blink of clip.blinks) {
        const phase = (t - blink) / 0.28;
        if (phase >= 0 && phase <= 1) {
            const lid = phase < 0.43 ? 1 - phase / 0.43 : (phase - 0.43) / 0.57;
            pose.left *= Math.max(0.04, lid); pose.right *= Math.max(0.04, lid);
        }
    }
    return pose;
}

export class AvatarPlayer {
    state: RecipeState;
    clip: ClipName;
    time = 0;
    elapsed = 0;
    pose = { ...restPose };
    amount = 0;
    private randomState: number;
    private bag: ClipName[] = [];
    private phase = 0;
    private bridge = 0;
    private from = { ...restPose };
    private morphAmount: Spring = { x: 0, v: 0, t: 0 };
    private morphSwap: Spring = { x: 1, v: 0, t: 1 };
    private morphSpin: Spring = { x: 0, v: 0, t: 0 };
    private morphOn = false;
    private spinDirection = 1;
    private kind: MorphKind | null = null;
    private outgoing: MorphKind | null = null;
    private kindTime = 0;
    private outgoingTime = 0;
    constructor(state: AvatarState = "idle", seed = 1) {
        this.state = resolveRecipeState(state); this.randomState = seed || 1;
        this.clip = avatarRecipes[this.state].clips[0];
        this.choose();
        if (this.state === "idle") this.phase = this.random() * avatarClips[this.clip].duration;
    }
    private random() {
        let n = this.randomState | 0; n ^= n << 13; n ^= n >>> 17; n ^= n << 5;
        this.randomState = n; return (n >>> 0) / 4294967296;
    }
    private choose() {
        if (!this.bag.length) {
            this.bag = [...avatarRecipes[this.state].clips];
            for (let i = this.bag.length - 1; i > 0; i--) { const j = Math.floor(this.random() * (i + 1)); [this.bag[i], this.bag[j]] = [this.bag[j], this.bag[i]]; }
        }
        if (this.bag.length > 1 && this.bag[this.bag.length - 1] === this.clip) [this.bag[0], this.bag[this.bag.length - 1]] = [this.bag[this.bag.length - 1], this.bag[0]];
        this.clip = this.bag.pop()!;
    }
    setState(state: AvatarState) {
        const resolved = resolveRecipeState(state);
        if (resolved === this.state) return;
        this.from = { ...this.pose }; this.bridge = 0.42;
        this.state = resolved; this.elapsed = 0; this.phase = 0; this.bag = []; this.choose();
    }
    advance(seconds: number, compact = false) {
        // Caller suspends offscreen; bound late frames so resuming cannot skip a performance.
        const dt = clamp(seconds, 0, 0.05);
        this.time += dt; this.elapsed += dt;
        let recipe: StateRecipe = avatarRecipes[this.state];
        if (recipe.next && this.elapsed >= (recipe.hold ?? Infinity)) {
            this.setState(recipe.next as RecipeState); recipe = avatarRecipes[this.state];
        }
        const effect: AvatarEffect = recipe.effect ?? "none";
        this.updateMorph(isMorphKind(effect) ? effect : null, dt);
        const amount = this.morphAmount.x;
        // 变形期间 clip 减速，避免缩小的圆核还在大幅摇头晃脑。
        this.phase += dt * (recipe.tempo ?? 1) * (1 - 0.85 * amount);
        if (this.phase >= avatarClips[this.clip].duration) { this.phase -= avatarClips[this.clip].duration; this.choose(); }
        const target = sampleClip(this.clip, this.phase);
        for (const key of keys) target[key] = restPose[key] + (target[key] - restPose[key]) * (recipe.strength ?? 1);
        for (const key of keys) {
            const face = recipe.face?.[key];
            if (face === undefined) continue;
            if (key === "left" || key === "right") target[key] *= face;
            else target[key] += face;
        }
        if (this.bridge > 0) {
            this.bridge = Math.max(0, this.bridge - dt);
            const t = bezierEase(1 - this.bridge / 0.42, [0.22, 0, 0.2, 1]);
            for (const key of keys) target[key] = this.from[key] + (target[key] - this.from[key]) * t;
        }
        for (const key of bodyKeys) target[key] = restPose[key] + (target[key] - restPose[key]) * (1 - amount);
        target.left = clamp(target.left, 0.03, 1.6); target.right = clamp(target.right, 0.03, 1.6);
        target.happy = clamp(target.happy, 0, 1); target.sad = clamp(target.sad, 0, 1);
        if (compact) {
            target.x *= 0.15; target.y *= 0.18; target.roll *= 0.4; target.yaw *= 0.6;
            target.sx = 1 + (target.sx - 1) * 0.25; target.sy = 1 + (target.sy - 1) * 0.25;
        }
        this.pose = target;
        this.amount = amount;
        return { pose: target, amount, effect, time: this.time, state: this.state, clip: this.clip, morph: this.morphFrame() };
    }

    /** 装饰行为器：amount 管「在不在」，swap 管「换没换」，spin 管「进退场转身」。 */
    private updateMorph(kind: MorphKind | null, dt: number) {
        if (kind && kind !== this.kind) {
            if (this.kind && this.morphAmount.x > 0.02) {
                this.outgoing = this.kind; this.outgoingTime = this.kindTime; this.morphSwap.x = 0;
            } else { this.outgoing = null; this.morphSwap.x = 1; }
            this.morphSwap.v = 0; this.kind = kind; this.kindTime = 0;
        }
        const on = kind !== null;
        this.morphAmount.t = on ? 1 : 0;
        if (on !== this.morphOn) {
            // 进场随机选方向转半圈，退场沿同方向再转半圈：一整圈，不回卷。
            if (on) this.spinDirection = this.random() < 0.5 ? 1 : -1;
            this.morphSpin.t += Math.PI * this.spinDirection; this.morphOn = on;
        }
        for (let remaining = dt; remaining > 0;) {
            const step = Math.min(remaining, 1 / 120);
            stepSpring(this.morphAmount, 14, step); stepSpring(this.morphSwap, 11, step); stepSpring(this.morphSpin, 14, step);
            remaining -= step;
        }
        this.morphAmount.x = clamp(this.morphAmount.x, 0, 1);
        this.kindTime += dt; this.outgoingTime += dt;
        if (this.morphSwap.x > 0.996) this.outgoing = null;
        if (!on && this.morphAmount.x < 0.004) { this.kind = null; this.outgoing = null; }
    }

    private morphFrame(): MorphFrame {
        const amount = this.morphAmount.x, swap = clamp(this.morphSwap.x, 0, 1);
        const { kind, outgoing } = this;
        const radius = kind ? (outgoing ? morphRadius[kind] * swap + morphRadius[outgoing] * (1 - swap) : morphRadius[kind]) : BODY_RADIUS;
        const current = kind ? coreMotion(kind, this.kindTime) : restCore;
        const previous = outgoing ? coreMotion(outgoing, this.outgoingTime) : restCore;
        const wc = amount * (outgoing ? swap : 1), wo = outgoing ? amount * (1 - swap) : 0;
        const mix = (key: keyof CoreMotion, rest: number) => rest + (current[key] - rest) * wc + (previous[key] - rest) * wo;
        return {
            amount, swap, spin: this.morphSpin.x, kind, outgoing, kindTime: this.kindTime, outgoingTime: this.outgoingTime,
            core: { scale: 1 - amount + amount * radius / BODY_RADIUS, dx: mix("dx", 0), dy: mix("dy", 0), sx: mix("sx", 1), sy: mix("sy", 1), opacity: mix("opacity", 1) },
        };
    }
}
