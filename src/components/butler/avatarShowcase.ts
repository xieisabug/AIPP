import { accessoryDefinitions, actionDuration, type AvatarShape, type AccessoryId, type AvatarAction, type AvatarState } from "@/components/avatar/avatarDefinition";

/** 编辑形象时自动轮播的内容：状态与动作交替出现。 */
export type ShowcaseStep = { kind: "state"; state: AvatarState } | { kind: "action"; action: AvatarAction };

export const showcaseStates: readonly AvatarState[] = [
    "idle", "listening", "thinking", "happy", "working", "curious", "waiting", "laughing",
    "searching", "playful", "writing", "surprised", "shy", "bouncing", "humming", "celebrate", "coding",
];

export const accessoryActionsOf = (items: (AccessoryId | null)[]) =>
    items.filter((item): item is AccessoryId => item !== null).flatMap(item => accessoryDefinitions[item].action ?? []);

/** 偶数步展示状态，奇数步展示动作；戴了装饰时，装饰动作占动作轮次的大半，方便看清效果。 */
export function showcaseStep(step: number, accessoryActions: AvatarAction[]): ShowcaseStep {
    if (step % 2 === 0) return { kind: "state", state: showcaseStates[(step / 2) % showcaseStates.length] };
    const actions: AvatarAction[] = [...accessoryActions, "smile", ...accessoryActions, "blink"];
    return { kind: "action", action: actions[((step - 1) / 2) % actions.length] };
}

/** 每一幕停留 3–5 秒（由序号决定，渲染之间保持稳定）；动作至少要完整播完。 */
export function showcaseDelay(current: ShowcaseStep, seed: number, shape?: AvatarShape) {
    const base = 3000 + (Math.abs(seed * 7919) % 2001);
    return current.kind === "action" ? Math.max(base, actionDuration(current.action, shape) + 600) : base;
}
