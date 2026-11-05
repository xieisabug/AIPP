import { createContext } from "react";
import { avatarDefinitions, type AvatarShape } from "@/components/avatar/avatarDefinition";

export interface ButlerAppearance {
    version: 2;
    shape: AvatarShape;
    colors: { primary: string; secondary: string };
    accessories: { eyes: "round-glasses" | null; head: "beret" | null };
    motion: boolean;
}

export const defaultAppearance: ButlerAppearance = {
    version: 2, shape: "aipp", colors: { primary: "#FFFFFF", secondary: "#000000" },
    accessories: { eyes: null, head: null }, motion: true,
};
export const ButlerAppearanceContext = createContext<Map<string, string>>(new Map());
export const appearanceKey = (id?: number) => `butler_appearance_${id === undefined ? "chief" : id}`;
const isColor = (value: unknown): value is string => typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value);

export function parseAppearance(value?: string): ButlerAppearance | undefined {
    if (!value) return undefined;
    try {
        const parsed = JSON.parse(value);
        if (!parsed || typeof parsed !== "object") return undefined;
        // Existing installations migrate in memory; persistence happens only on save.
        if (parsed.version === undefined && parsed.shape === "aipp" && isColor(parsed.left) && isColor(parsed.right)) {
            return { ...defaultAppearance, colors: { primary: parsed.left, secondary: parsed.right } };
        }
        if (parsed.version !== 2 || !Object.hasOwn(avatarDefinitions, parsed.shape)) return undefined;
        if (!isColor(parsed.colors?.primary) || !isColor(parsed.colors?.secondary)) return undefined;
        if (![null, "round-glasses"].includes(parsed.accessories?.eyes) || ![null, "beret"].includes(parsed.accessories?.head)) return undefined;
        if (typeof parsed.motion !== "boolean") return undefined;
        return {
            version: 2, shape: parsed.shape, colors: { primary: parsed.colors.primary, secondary: parsed.colors.secondary },
            accessories: { eyes: parsed.accessories.eyes, head: parsed.accessories.head }, motion: parsed.motion,
        };
    } catch { return undefined; }
}
