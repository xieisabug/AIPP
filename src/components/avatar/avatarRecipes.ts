// Original AIPP choreography. Times are seconds, distances use the 510-unit rig.
export const restPose = {
    x: 0, y: 0, roll: 0, yaw: 0, pitch: 0, sx: 1, sy: 1, zoom: 1,
    left: 1, right: 1, happy: 0, sad: 0, tilt: 0, gazeX: 0, gazeY: 0,
};
export type AvatarPose = typeof restPose;
export type PoseKey = keyof AvatarPose;
export type Ease = readonly [number, number, number, number];
export interface PoseFrame { at: number; pose: Partial<AvatarPose>; ease?: Ease }
export interface AvatarClip { duration: number; frames: PoseFrame[]; blinks: number[] }
const gentle: Ease = [0.4, 0, 0.6, 1];
const bounce: Ease = [0.3, -0.22, 0.22, 1.24];
const clip = (duration: number, frames: [number, Partial<AvatarPose>, Ease?][], blinks: number[] = []): AvatarClip => ({
    duration, frames: [[0, {}], ...frames, [duration, {}]].map(([at, pose, ease]) => ({ at, pose, ease: ease ?? gentle })) as PoseFrame[], blinks,
});

export const avatarClips = {
    breathe: clip(5.8, [[1.6, { y: -9, sx: 0.985, sy: 1.035 }], [3.1, { y: 3, sx: 1.02, sy: 0.98 }], [4.5, { yaw: -9, gazeX: -4 }]], [1.1, 4.6]),
    glance: clip(5.2, [[0.7, { yaw: -24, roll: -5, gazeX: -8 }], [1.7, { yaw: -24, pitch: 9, left: 0.75 }], [2.5, { yaw: 27, roll: 5 }, bounce], [3.6, { yaw: 27, right: 0.7 }], [4.4, { pitch: -8 }]], [2.2, 4.6]),
    doubleBlink: clip(4.6, [[0.9, { pitch: 8, y: 4 }], [2.1, { roll: -8, yaw: -12, right: 0.8 }], [3.4, { roll: 3, y: -6 }, bounce]], [1.25, 1.65, 3.7]),
    stretch: clip(5.5, [[1, { sx: 1.12, sy: 0.85, y: 20, left: 0.55, right: 0.55 }], [1.8, { sx: 0.88, sy: 1.16, y: -23, pitch: -16 }, bounce], [2.8, { roll: 8, yaw: 10 }], [4.2, { y: 4, happy: 0.5 }]], [3.5]),
    listen: clip(4.2, [[0.6, { roll: -12, yaw: -16, zoom: 1.05, pitch: 7 }], [1.9, { roll: -12, yaw: -16, left: 1.15, right: 0.85 }], [2.7, { roll: 5, pitch: 13 }, bounce]], [3.4]),
    nod: clip(2.8, [[0.45, { pitch: -12, y: -7 }], [0.85, { pitch: 22, y: 13, sy: 0.93 }, bounce], [1.3, { pitch: -8, y: -5 }], [1.7, { pitch: 14, y: 8 }], [2.2, { happy: 0.7 }]], [1.9]),
    ponder: clip(5.7, [[0.8, { yaw: 22, pitch: -17, roll: 9, gazeY: -8 }], [2.1, { yaw: 22, pitch: -17, left: 0.5, right: 1.12 }], [3, { yaw: -18, roll: -7, gazeX: -10 }], [4.3, { yaw: -18, pitch: 10, right: 0.6 }]], [2.5, 4.9]),
    scan: clip(4.5, [[0.6, { yaw: -30, gazeX: -10 }], [1.5, { yaw: -30, pitch: 12 }], [2.4, { yaw: 30, gazeX: 10 }], [3.3, { yaw: 30, pitch: -10 }]], [1.8, 3.9]),
    read: clip(4.8, [[0.5, { pitch: 16, yaw: -18, gazeY: 10 }], [1.5, { pitch: 16, yaw: 20, gazeY: 10 }], [1.8, { pitch: 23, yaw: -18, y: 5 }], [2.9, { pitch: 23, yaw: 20, y: 5 }], [3.8, { pitch: 4, left: 0.7, right: 0.7 }]], [3.3]),
    workLeft: clip(3.8, [[0.45, { yaw: -22, roll: -9, x: -12, sy: 0.94 }], [1.1, { yaw: -22, pitch: 14, x: -12, left: 0.65 }], [1.65, { yaw: 14, roll: 7, x: 9, sy: 1.05 }, bounce], [2.5, { pitch: -10, right: 0.7 }]], [2.8]),
    workRight: clip(4.3, [[0.7, { yaw: 25, roll: 10, x: 15, pitch: 10 }], [1.4, { yaw: 25, sy: 0.88, sx: 1.07, y: 15 }], [2, { yaw: -16, roll: -6, y: -12, sy: 1.08 }, bounce], [3.1, { pitch: 12, left: 0.8, right: 0.6 }]], [0.3, 3.7]),
    tap: clip(3.3, [[0.5, { pitch: 18, y: 12, sy: 0.94 }], [0.9, { pitch: -8, y: -10 }], [1.3, { pitch: 20, y: 14, sy: 0.93 }], [1.8, { yaw: 16, roll: 5 }], [2.5, { yaw: -12, left: 0.6 }]], [2.8]),
    talk: clip(3.6, [[0.45, { zoom: 1.06, y: -10, pitch: -8 }], [0.9, { sy: 0.94, y: 6 }], [1.4, { roll: -8, yaw: -13, zoom: 1.04 }], [2, { roll: 7, yaw: 17, y: -9 }], [2.7, { happy: 0.6 }]], [3]),
    wait: clip(5.8, [[1.1, { yaw: -18, left: 0.7, right: 0.7 }], [2.3, { pitch: 12, sy: 0.97, y: 8 }], [3.5, { yaw: 20, roll: 6 }], [4.8, { yaw: 20, left: 0.55, right: 0.55 }]], [1.8, 4.2]),
    shrug: clip(3.7, [[0.5, { sy: 0.91, sx: 1.07, y: 10, left: 0.65 }], [1.1, { y: -20, roll: -12, right: 1.15 }, bounce], [2.1, { roll: 13, yaw: 18, left: 1.1, right: 0.6 }], [2.9, { y: 7, sad: 0.4 }]], [3.1]),
    bounce: clip(2.7, [[0.35, { sy: 0.78, sx: 1.17, y: 30 }], [0.85, { sy: 1.13, sx: 0.9, y: -65, happy: 1 }, bounce], [1.35, { sy: 0.83, sx: 1.12, y: 25 }], [1.8, { y: -24, sy: 1.06, happy: 1 }], [2.2, { sy: 0.97, y: 5 }]], [2.4]),
    laugh: clip(3.2, [[0.35, { happy: 1, pitch: -13, sy: 1.08, y: -10 }], [0.7, { happy: 1, sy: 0.88, sx: 1.07, y: 16 }], [1, { happy: 1, sy: 1.08, y: -13 }], [1.35, { happy: 1, sy: 0.88, y: 16 }], [1.7, { happy: 1, roll: -9, y: -10 }], [2.4, { happy: 0.7, roll: 7 }]]),
    proud: clip(4.4, [[0.8, { pitch: -20, zoom: 1.08, roll: -7, happy: 0.7 }], [2.3, { pitch: -20, yaw: 20, left: 0.6, right: 0.6 }], [3.4, { roll: 7, happy: 0.5 }]], [3.8]),
    peek: clip(4.2, [[0.6, { yaw: -30, roll: -15, x: -17, left: 1.15, right: 0.65 }], [1.8, { yaw: -30, zoom: 1.08, pitch: -8 }], [2.5, { yaw: 26, roll: 12, x: 15, left: 0.7, right: 1.15 }, bounce]], [3.5]),
    wink: clip(3.5, [[0.5, { roll: 10, yaw: 10, happy: 0.3 }], [1.1, { left: 0.05, right: 1.1, roll: 13, x: 8 }], [1.8, { left: 0.05, roll: 13, happy: 0.25 }], [2.5, { happy: 0.8 }]], [3]),
    shy: clip(4.7, [[0.8, { pitch: 20, yaw: -17, roll: -8, zoom: 0.92, left: 0.65, right: 0.65 }], [2, { pitch: 20, yaw: -17, happy: 0.5 }], [3.1, { pitch: -5, yaw: 12, right: 1.1 }], [3.8, { pitch: 13, happy: 0.7 }]], [1.4, 4.1]),
    startle: clip(2.5, [[0.22, { y: -38, sy: 1.17, sx: 0.9, left: 1.45, right: 1.45, zoom: 1.07 }, bounce], [0.8, { y: -22, roll: -8, left: 1.4, right: 1.4 }], [1.5, { y: 8, sy: 0.93, yaw: 16 }]], [2]),
    doubt: clip(4.3, [[0.7, { yaw: 20, roll: -13, left: 0.35, right: 1.15, tilt: 0.4 }], [2.1, { yaw: 20, roll: -13, left: 0.35, right: 1.15 }], [3.1, { yaw: -15, roll: 9, left: 1, right: 0.4 }]], [3.8]),
    shake: clip(3, [[0.4, { yaw: -25, roll: -7, tilt: 0.7, left: 0.6, right: 0.6 }], [0.85, { yaw: 25, roll: 7, tilt: 0.7 }], [1.3, { yaw: -20, roll: -6, tilt: 0.6 }], [1.75, { yaw: 17, roll: 5 }], [2.3, { pitch: 10, sad: 0.4 }]], [2.6]),
    droop: clip(6, [[1.2, { y: 20, sy: 0.93, pitch: 20, sad: 0.8, left: 0.7, right: 0.7 }], [2.8, { y: 24, roll: -8, yaw: -12, sad: 1 }], [4.5, { y: 18, roll: 5, pitch: 15, left: 0.5, right: 0.5 }]], [3.5, 5.1]),
    doze: clip(7, [[1.5, { left: 0.35, right: 0.35, pitch: 10, y: 10 }], [3.2, { left: 0.05, right: 0.05, pitch: 25, y: 24, sy: 0.93 }], [4, { left: 1.2, right: 1.2, pitch: -9, y: -10 }, bounce], [5.6, { left: 0.4, right: 0.4, roll: 9, y: 12 }]], [6.2]),
    wake: clip(2.8, [[0.35, { sy: 0.88, sx: 1.08, left: 0.1, right: 0.1, y: 15 }], [1, { sy: 1.16, sx: 0.91, y: -27, left: 1.4, right: 1.4 }, bounce], [1.8, { yaw: -20, roll: -6 }], [2.3, { yaw: 16, happy: 0.3 }]], [2]),
    flourish: clip(4.6, [[0.6, { sy: 0.86, sx: 1.08, y: 19 }], [1.4, { roll: -25, yaw: -32, y: -35, zoom: 1.08, happy: 1 }, bounce], [2.2, { roll: 23, yaw: 30, y: -25, happy: 1 }], [3.2, { roll: -8, y: 7, sy: 0.94 }], [3.8, { pitch: 16, happy: 0.7 }]], [4.1]),
};
export type ClipName = keyof typeof avatarClips;
export type AvatarEffect = "none" | "dots" | "orbit" | "radar" | "progress" | "spark" | "bars" | "send" | "receive" | "upload" | "pencil" | "alert" | "ball" | "power" | "confetti";
export interface StateRecipe { label: string; group: string; clips: ClipName[]; face?: Partial<AvatarPose>; effect?: AvatarEffect; tempo?: number; strength?: number; next?: string; hold?: number }
const state = (label: string, group: string, clips: ClipName[], options: Omit<StateRecipe, "label" | "group" | "clips"> = {}): StateRecipe => ({ label, group, clips, ...options });
export const avatarRecipes = {
    idle: state("待机", "日常", ["breathe", "glance", "doubleBlink", "stretch"]),
    listening: state("聆听", "日常", ["listen", "nod", "peek"], { face: { left: 0.9, right: 0.9 } }),
    thinking: state("思考", "日常", ["ponder", "doubleBlink"], { effect: "dots" }),
    streaming: state("输出", "日常", ["talk", "nod", "workRight"]),
    searching: state("搜索", "日常", ["scan", "peek"], { effect: "radar" }),
    reading: state("阅读", "日常", ["read", "glance"], { face: { pitch: 8 } }),
    working: state("工作", "日常", ["workLeft", "tap", "workRight", "read"]),
    coding: state("编程", "日常", ["tap", "workLeft", "scan"], { tempo: 1.18, face: { left: 0.8, right: 0.8 } }),
    running: state("运行", "日常", ["workRight", "tap"], { tempo: 1.4, strength: 1.1 }),
    computing: state("计算", "日常", ["ponder", "scan", "tap"], { effect: "dots", tempo: 1.3 }),
    controlling: state("操控", "日常", ["scan", "workLeft", "nod"], { face: { pitch: 10 } }),
    waiting: state("等待", "日常", ["wait", "glance"], { effect: "orbit" }),
    blocked: state("受阻", "日常", ["shrug", "wait", "doubt"], { next: "drowsy", hold: 120 }),
    finished: state("完成", "日常", ["bounce", "nod"], { face: { happy: 0.8 }, next: "idle", hold: 4 }),
    failed: state("失败", "日常", ["shake", "droop"], { face: { sad: 0.5 }, next: "idle", hold: 5 }),
    sleeping: state("睡眠", "日常", ["breathe"], { face: { left: 0.04, right: 0.04, pitch: 15 }, tempo: 0.65 }),
    waking: state("苏醒", "日常", ["wake"], { next: "idle", hold: 2.8 }),
    happy: state("开心", "情绪", ["bounce", "proud"], { face: { happy: 0.8 } }),
    excited: state("兴奋", "情绪", ["bounce", "flourish"], { tempo: 1.25, strength: 1.15 }),
    laughing: state("大笑", "情绪", ["laugh", "bounce"], { face: { happy: 1 } }),
    proud: state("得意", "情绪", ["proud", "wink"], { face: { happy: 0.4 } }),
    curious: state("好奇", "情绪", ["peek", "ponder"], { face: { right: 1.15 } }),
    playful: state("顽皮", "情绪", ["wink", "flourish", "peek"], { tempo: 1.2 }),
    shy: state("害羞", "情绪", ["shy", "doubleBlink"], { face: { happy: 0.3 }, strength: 0.85 }),
    surprised: state("惊讶", "情绪", ["startle", "peek"], { face: { left: 1.25, right: 1.25 } }),
    suspicious: state("怀疑", "情绪", ["doubt", "scan"], { face: { left: 0.45, tilt: 0.35 } }),
    angry: state("生气", "情绪", ["shake", "tap"], { face: { tilt: 0.9, left: 0.55, right: 0.55 }, tempo: 1.15 }),
    confused: state("困惑", "情绪", ["shrug", "doubt", "ponder"]),
    sad: state("难过", "情绪", ["droop", "shy"], { face: { sad: 1 }, tempo: 0.8 }),
    drowsy: state("困倦", "情绪", ["doze", "droop"], { face: { left: 0.55, right: 0.55 } }),
    bored: state("无聊", "情绪", ["wait", "droop"], { face: { left: 0.45, right: 0.45 }, tempo: 0.75 }),
    scared: state("害怕", "情绪", ["startle", "shake"], { face: { left: 1.3, right: 1.3 }, tempo: 1.3, strength: 0.8 }),
    celebrate: state("庆祝", "情绪", ["flourish", "bounce", "laugh"], { effect: "confetti", face: { happy: 1 } }),
    approving: state("认可", "情绪", ["nod"], { face: { happy: 0.6 }, next: "idle", hold: 2.8 }),
    orbit: state("环绕", "图形", ["breathe"], { effect: "orbit" }),
    radar: state("雷达", "图形", ["scan"], { effect: "radar" }),
    progress: state("进度", "图形", ["breathe"], { effect: "progress" }),
    spawning: state("生成", "图形", ["stretch"], { effect: "spark" }),
    loading: state("加载", "图形", ["breathe", "doubleBlink"], { effect: "progress", tempo: 1.4 }),
    dictating: state("听写", "图形", ["listen"], { effect: "bars" }),
    sending: state("发送", "图形", ["nod"], { effect: "send" }),
    receiving: state("接收", "图形", ["listen"], { effect: "receive" }),
    uploading: state("上传", "图形", ["stretch"], { effect: "upload" }),
    writing: state("书写", "图形", ["read", "tap"], { effect: "pencil" }),
    alerting: state("提醒", "图形", ["startle"], { effect: "alert" }),
    bouncing: state("弹球", "图形", ["bounce"], { effect: "ball" }),
    "powering-down": state("休眠", "图形", ["droop"], { effect: "power", next: "sleeping", hold: 3.5 }),
    humming: state("哼歌", "互动", ["laugh", "listen"], { effect: "bars", tempo: 0.85, face: { happy: 0.5 } }),
    notifying: state("通知", "互动", ["startle", "nod"], { effect: "alert", next: "idle", hold: 4 }),
    dragging: state("晃动", "互动", ["shake", "flourish"], { face: { left: 1.2, right: 1.2 }, strength: 1.15 }),
    logo: state("标志", "展示", ["breathe"], { strength: 0.1 }),
    showcase: state("展示", "展示", ["flourish", "proud", "stretch"]),
    turntable: state("转头", "展示", ["scan", "peek"], { strength: 1.4, tempo: 0.7 }),
};
export type RecipeState = keyof typeof avatarRecipes;
export type AvatarState = RecipeState | "responding" | "success" | "error";
export const resolveRecipeState = (state: AvatarState): RecipeState => state === "responding" ? "streaming" : state === "success" ? "finished" : state === "error" ? "failed" : state;
