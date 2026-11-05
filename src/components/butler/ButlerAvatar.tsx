import { useContext, type CSSProperties } from "react";
import AippNormal from "@/assets/aipp-normal.svg?react";
import AippSmile from "@/assets/aipp-smile.svg?react";
import { appearanceKey, ButlerAppearanceContext, defaultAppearance, parseAppearance, type ButlerAppearance } from "./butlerAppearance";

const colors = ["#246bd5", "#bb3156", "#9353ba", "#147764", "#9a612e", "#5365a9", "#c14a28"];

export default function ButlerAvatar({ id, chief = false, size = 32, appearance }: { id: number; chief?: boolean; size?: number; appearance?: ButlerAppearance }) {
    const appearances = useContext(ButlerAppearanceContext);
    const selected = appearance ?? parseAppearance(appearances.get(appearanceKey(chief ? undefined : id))) ?? (chief ? defaultAppearance : undefined);
    if (selected) {
        return <span aria-hidden="true" className="group/butler-avatar relative inline-flex shrink-0 items-center justify-center"
            style={{ width: size, height: size, "--aipp-left": selected.left, "--aipp-right": selected.right } as CSSProperties}>
            <AippNormal className="h-full w-full group-hover/butler-avatar:invisible" />
            <AippSmile className="absolute inset-0 h-full w-full invisible group-hover/butler-avatar:visible" />
        </span>;
    }

    return <span aria-hidden="true" className="butler-avatar relative inline-flex shrink-0 items-center justify-center"
        style={{ width: size, height: size, backgroundColor: colors[Math.abs(id) % colors.length] }}>
        <span className="butler-eyes" />
    </span>;
}
