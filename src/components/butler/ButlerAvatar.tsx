import { useContext } from "react";
import AnimatedAvatar, { type AnimatedAvatarProps } from "@/components/avatar/AnimatedAvatar";
import { appearanceKey, ButlerAppearanceContext, defaultAppearance, parseAppearance, type ButlerAppearance } from "./butlerAppearance";

const colors = ["#246bd5", "#bb3156", "#9353ba", "#147764", "#9a612e", "#5365a9", "#c14a28"];

export default function ButlerAvatar({ id, chief = false, size = 32, appearance, ...animation }: { id: number; chief?: boolean; size?: number; appearance?: ButlerAppearance } & Omit<AnimatedAvatarProps, "appearance" | "size">) {
    const appearances = useContext(ButlerAppearanceContext);
    const selected = appearance ?? parseAppearance(appearances.get(appearanceKey(chief ? undefined : id))) ?? (chief ? defaultAppearance : animation.animated ? {
        ...defaultAppearance, shape: "pebble" as const,
        colors: { primary: colors[Math.abs(id) % colors.length], secondary: colors[Math.abs(id) % colors.length] },
    } : undefined);
    if (selected) {
        return <AnimatedAvatar key={selected.shape} appearance={selected} size={size} {...animation} />;
    }

    return <span aria-hidden="true" className="butler-avatar relative inline-flex shrink-0 items-center justify-center"
        style={{ width: size, height: size, backgroundColor: colors[Math.abs(id) % colors.length] }}>
        <span className="butler-eyes" />
    </span>;
}
