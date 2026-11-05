import { avatarClips, avatarRecipes, resolveRecipeState, restPose, type AvatarPose, type AvatarState, type ClipName, type Ease, type RecipeState, type StateRecipe } from "./avatarRecipes";

const keys = Object.keys(restPose) as (keyof AvatarPose)[];
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
    private velocity = 0;
    private randomState: number;
    private bag: ClipName[] = [];
    private phase = 0;
    private bridge = 0;
    private from = { ...restPose };
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
        this.phase += dt * (recipe.tempo ?? 1);
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
        const effect = recipe.effect ?? "none";
        const targetAmount = !compact && effect !== "none" && effect !== "confetti" ? 1 : 0;
        for (let remaining = dt; remaining > 0;) {
            const step = Math.min(remaining, 1 / 120);
            this.velocity += (150 * (targetAmount - this.amount) - 22 * this.velocity) * step;
            this.amount += this.velocity * step; remaining -= step;
        }
        target.left = clamp(target.left, 0.03, 1.6); target.right = clamp(target.right, 0.03, 1.6);
        target.happy = clamp(target.happy, 0, 1); target.sad = clamp(target.sad, 0, 1);
        if (compact) {
            target.x *= 0.15; target.y *= 0.18; target.roll *= 0.4; target.yaw *= 0.6;
            target.sx = 1 + (target.sx - 1) * 0.25; target.sy = 1 + (target.sy - 1) * 0.25;
        }
        this.pose = target;
        return { pose: target, amount: this.amount, effect, time: this.time, state: this.state, clip: this.clip };
    }
}
