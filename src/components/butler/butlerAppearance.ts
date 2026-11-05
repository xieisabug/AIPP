import { createContext } from "react";

export interface ButlerAppearance {
    shape: "aipp";
    left: string;
    right: string;
}

export const defaultAppearance: ButlerAppearance = { shape: "aipp", left: "#FFFFFF", right: "#000000" };
export const ButlerAppearanceContext = createContext<Map<string, string>>(new Map());
export const appearanceKey = (id?: number) => `butler_appearance_${id === undefined ? "chief" : id}`;

export function parseAppearance(value?: string): ButlerAppearance | undefined {
    if (!value) return undefined;
    try {
        const parsed = JSON.parse(value);
        if (parsed.shape === "aipp" && /^#[0-9a-f]{6}$/i.test(parsed.left) && /^#[0-9a-f]{6}$/i.test(parsed.right)) {
            return { shape: "aipp", left: parsed.left, right: parsed.right };
        }
    } catch { /* Invalid stored appearance leaves the original avatar intact. */ }
    return undefined;
}
