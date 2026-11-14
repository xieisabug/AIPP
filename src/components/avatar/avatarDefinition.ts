import { avatarRecipes } from "./avatarRecipes";
export type { AvatarState } from "./avatarRecipes";
export type AvatarAction = "blink" | "smile" | "glasses" | "hat" | "peer" | "blush" | "jingle" | "butterfly";
export type AvatarShape = "aipp" | "pebble" | "pixel" | "whale" | "blossom";
export interface AvatarAnchor { x: number; y: number; scale: number; rotation?: number }
export interface AvatarDefinition {
    id: AvatarShape;
    label: string;
    // All artwork and anchors use the same 510-unit coordinate system.
    parts: { path: string; color: "primary" | "secondary" | "white"; outlineWidth?: number }[];
    /** Uniform artwork scale around the avatar center, including equipped accessories. */
    artworkScale?: number;
    eyeColor?: string;
    eyeScale?: number;
    face: AvatarAnchor;
    crown: AvatarAnchor;
    /** 蝴蝶结和小花的佩戴位置。 */
    clip: AvatarAnchor;
    propDock: AvatarAnchor;
    colorLabels: { primary?: string; secondary: string };
    capabilities: readonly ("eyes" | "headwear")[];
}

export const avatarDefinitions: Record<AvatarShape, AvatarDefinition> = {
    aipp: {
        id: "aipp", label: "AIPP",
        parts: [
            { color: "secondary", path: "M385 17C452.931 17 508 72.069 508 140V295C508 353.542 460.542 401 402 401H265V251.5C275.627 259.355 288.771 264 303 264C338.346 264 367 235.346 367 200C367 164.654 338.346 136 303 136C288.771 136 275.627 140.645 265 148.499V17H385Z" },
            { color: "primary", path: "M244 143.332C244.948 160.105 262.969 169.76 271.5 165.5C279.435 161.537 291.823 156 302 156C326.3 156 346 175.7 346 200C346 224.3 326.3 244 302 244C285.413 244 276.5 236.5 273 233.5C263.195 231.539 244.331 239.672 244 259.785V401H174.07L91.5 484C91.4905 484.013 83.3811 494.652 78 492.5C72.5621 490.325 74.4993 473.006 74.5 473V395.923C31.8569 382.201 1 342.205 1 295V140C1 72.069 56.069 17 124 17H244V143.332Z" },
        ],
        face: { x: 152, y: 199, scale: 1 }, crown: { x: 242, y: 25, scale: 1 }, clip: { x: 92, y: 30, scale: 1, rotation: -24 },
        propDock: { x: 415, y: 325, scale: 0.8, rotation: -28 },
        colorLabels: { primary: "左侧", secondary: "右侧" }, capabilities: ["eyes", "headwear"],
    },
    pixel: {
        id: "pixel", label: "像素仔", artworkScale: 507 / 456,
        parts: [{ color: "secondary", path: "M105 91H405Q429 91 429 115V205H465Q483 205 483 223V247Q483 265 465 265H429V379H405V319H375V379H351V319H159V379H135V319H105V379H81V265H45Q27 265 27 247V223Q27 205 45 205H81V115Q81 91 105 91Z" }],
        face: { x: 255, y: 178, scale: 1.23 }, crown: { x: 255, y: 91, scale: 1.08 },
        clip: { x: 114, y: 97, scale: 1, rotation: -16 }, propDock: { x: 380, y: 316, scale: 0.7, rotation: -20 },
        colorLabels: { secondary: "身体" }, capabilities: ["eyes", "headwear"],
    },
    whale: {
        id: "whale", label: "小鲸鱼", eyeColor: "#FFFFFF", eyeScale: 0.9,
        parts: [
            { color: "secondary", path: "M187 96C111 80 45 111 16 178C-18 255 6 347 69 400C131 452 246 463 316 404C337 413 373 415 396 405Q410 393 389 383L355 370C398 327 430 263 426 199C478 193 508 154 508 109Q508 94 498 104C469 137 447 111 415 143C412 107 373 115 363 83Q356 52 347 79C327 132 346 164 382 190L374 217C339 211 304 158 268 136Q239 119 264 95Q282 83 258 82C235 80 209 88 187 96Z" },
            { color: "white", path: "M37 210C88 192 165 229 207 288C237 330 268 382 307 400C255 409 229 374 191 353C161 336 158 346 170 368C184 392 146 397 116 378C60 344 37 278 34 225Q33 214 37 210Z" },
        ],
        face: { x: 125, y: 173, scale: 0.87 }, crown: { x: 147, y: 96, scale: 0.88 },
        clip: { x: 415, y: 157, scale: 0.78, rotation: -14 }, propDock: { x: 324, y: 353, scale: 0.72, rotation: -24 },
        colorLabels: { secondary: "身体" }, capabilities: ["eyes", "headwear"],
    },
    blossom: {
        id: "blossom", label: "花团",
        parts: [{
            color: "white", outlineWidth: 24,
            path: "M300 70C360 45 449 95 444 181C502 224 495 325 423 357C407 434 327 475 259 437C187 468 98 439 76 368C37 345 26 299 43 262C-1 206 24 126 99 112C128 31 231 8 300 70Z",
        }],
        face: { x: 251, y: 251, scale: 1.32 }, crown: { x: 231, y: 49, scale: 0.98 },
        clip: { x: 137, y: 77, scale: 0.94, rotation: -22 }, propDock: { x: 402, y: 348, scale: 0.8, rotation: -24 },
        colorLabels: { secondary: "轮廓" }, capabilities: ["eyes", "headwear"],
    },
    pebble: {
        id: "pebble", label: "圆团",
        parts: [
            { color: "secondary", path: "M255 35C384 35 465 125 465 257C465 387 382 463 255 463C128 463 45 387 45 257C45 125 126 35 255 35Z" },
            { color: "primary", path: "M255 75C357 75 419 149 419 250C419 351 353 401 255 401C157 401 91 351 91 250C91 149 153 75 255 75Z" },
        ],
        face: { x: 255, y: 228, scale: 1.25 }, crown: { x: 255, y: 43, scale: 0.95 }, clip: { x: 146, y: 72, scale: 1, rotation: -32 },
        propDock: { x: 415, y: 355, scale: 0.85, rotation: -28 },
        colorLabels: { primary: "脸部", secondary: "外圈" }, capabilities: ["eyes", "headwear"],
    },
};

export const accessoryDefinitions = {
    "round-glasses": { slot: "eyes", label: "圆框眼镜", capability: "eyes", action: "glasses" },
    sunglasses: { slot: "eyes", label: "墨镜", capability: "eyes", action: "peer" },
    blush: { slot: "eyes", label: "腮红", capability: "eyes", action: "blush" },
    beret: { slot: "head", label: "贝雷帽", capability: "headwear", action: "hat" },
    "bow-bell": { slot: "head", label: "铃铛蝴蝶结", capability: "headwear", action: "jingle" },
    flower: { slot: "head", label: "小花", capability: "headwear", action: "butterfly" },
} as const;
export type AccessoryId = keyof typeof accessoryDefinitions;
type AccessoryOf<S> = { [K in AccessoryId]: (typeof accessoryDefinitions)[K]["slot"] extends S ? K : never }[AccessoryId];
export type FaceAccessory = AccessoryOf<"eyes">;
export type HeadAccessory = AccessoryOf<"head">;
export const accessoriesOf = <S extends "eyes" | "head">(slot: S) =>
    (Object.keys(accessoryDefinitions) as AccessoryId[]).filter(id => accessoryDefinitions[id].slot === slot) as (S extends "eyes" ? FaceAccessory : HeadAccessory)[];

export const actionDurations: Record<AvatarAction, number> = { blink: 280, smile: 1300, glasses: 2400, hat: 1500, peer: 3000, blush: 2000, jingle: 1800, butterfly: 3400 };
export const avatarStateLabels = Object.fromEntries(Object.entries(avatarRecipes).map(([key, recipe]) => [key, recipe.label]));
