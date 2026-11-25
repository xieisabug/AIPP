import { useEffect } from "react";

export type SettingsGroupId = "core" | "capabilities" | "privacy" | "system";

export type SettingsMenuId =
    | "llm-provider-config"
    | "assistant-config"
    | "butler-config"
    | "mcp-config"
    | "skills-config"
    | "plugins-config"
    | "ai-automation-config"
    | "mobile-tasks"
    | "device-connections"
    | "vault-config"
    | "data-sync-config"
    | "general-config"
    | "shortcuts-config"
    | "network-config"
    | "runtime-preview-config"
    | "about-config";

export interface SettingsNavResolution {
    menu: SettingsMenuId;
    subNav?: string;
}

export interface ParsedSubNav {
    tab?: string;
    anchor?: string;
    raw?: string;
}

export const SETTINGS_GROUP_LABELS: Record<SettingsGroupId, string> = {
    core: "核心配置",
    capabilities: "能力与扩展",
    privacy: "隐私与数据",
    system: "通用与系统",
};

const CANONICAL_MENUS = new Set<string>([
    "llm-provider-config",
    "assistant-config",
    "butler-config",
    "mcp-config",
    "skills-config",
    "plugins-config",
    "ai-automation-config",
    "mobile-tasks",
    "device-connections",
    "vault-config",
    "data-sync-config",
    "general-config",
    "shortcuts-config",
    "network-config",
    "runtime-preview-config",
    "about-config",
]);

const SHORT_MENU_ALIASES: Record<string, SettingsMenuId> = {
    providers: "llm-provider-config",
    assistants: "assistant-config",
    butler: "butler-config",
    apps: "mcp-config",
    skills: "skills-config",
    plugins: "plugins-config",
    "ai-automation": "ai-automation-config",
    security: "vault-config",
    "data-sync": "data-sync-config",
    general: "general-config",
    shortcuts: "shortcuts-config",
    network: "network-config",
    "runtime-preview": "runtime-preview-config",
    about: "about-config",
};

/**
 * Resolve both canonical menu IDs and legacy routes (especially `feature-assistant-config`
 * + old feature subNavs) to the new flat top-level settings route.
 */
export function resolveConfigNavigation(
    menu: string,
    subNav?: string
): SettingsNavResolution | null {
    if (menu === "feature-assistant-config") {
        const [featureId = "display", anchor] = (subNav ?? "").split("#");
        const withAnchor = (prefix?: string) => {
            if (prefix && anchor) return `${prefix}#${anchor}`;
            if (prefix) return prefix;
            return anchor ? `#${anchor}` : undefined;
        };
        switch (featureId) {
            case "display":
                return { menu: "general-config", subNav: withAnchor() };
            case "conversation_summary":
                return { menu: "ai-automation-config", subNav: withAnchor() };
            case "preview":
                return { menu: "runtime-preview-config", subNav: withAnchor() };
            case "data_folder":
                return { menu: "data-sync-config", subNav: withAnchor() };
            case "network_config":
                return { menu: "network-config", subNav: withAnchor() };
            case "shortcuts":
                return { menu: "shortcuts-config", subNav: withAnchor() };
            case "experimental":
                if (anchor === "dynamic-mcp" || anchor === "conversation-summary") {
                    return { menu: "ai-automation-config", subNav: withAnchor() };
                }
                return { menu: "butler-config", subNav: withAnchor() };
            case "other":
                if (anchor === "anti-leakage") {
                    return { menu: "vault-config", subNav: "policies#anti-leakage" };
                }
                if (anchor === "tool-error-continue") {
                    return { menu: "ai-automation-config", subNav: "#tool-error-continue" };
                }
                return { menu: "general-config", subNav: withAnchor() };
            case "about":
                return { menu: "about-config", subNav: withAnchor() };
            default:
                return { menu: "general-config", subNav };
        }
    }

    if (CANONICAL_MENUS.has(menu)) {
        return { menu: menu as SettingsMenuId, subNav };
    }
    const alias = SHORT_MENU_ALIASES[menu];
    if (alias) {
        return { menu: alias, subNav };
    }
    return null;
}

/**
 * Parse `subNav` formatted as `tab#anchor`, `#anchor`, or `tab`.
 */
export function parseSubNav(subNav?: string): ParsedSubNav {
    if (!subNav) return {};
    const hashIndex = subNav.indexOf("#");
    if (hashIndex === -1) {
        return { tab: subNav || undefined, raw: subNav };
    }
    const tab = subNav.slice(0, hashIndex) || undefined;
    const anchor = subNav.slice(hashIndex + 1) || undefined;
    return { tab, anchor, raw: subNav };
}

/**
 * Scroll to an anchor element by ID and briefly highlight it.
 */
export function useSettingsAnchorHighlight(
    anchor: string | null | undefined,
    onDone?: () => void,
    depKey?: string
) {
    useEffect(() => {
        if (!anchor) return;
        let highlightTimer: number | undefined;
        const scrollTimer = window.setTimeout(() => {
            const target = document.getElementById(anchor);
            if (!target) {
                onDone?.();
                return;
            }
            const reduced =
                typeof window !== "undefined" &&
                window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
            target.scrollIntoView({
                behavior: reduced ? "auto" : "smooth",
                block: "center",
            });
            target.classList.add(
                "ring-2",
                "ring-primary/60",
                "ring-offset-2",
                "ring-offset-background",
                "rounded-lg",
                "transition-shadow"
            );
            highlightTimer = window.setTimeout(() => {
                target.classList.remove(
                    "ring-2",
                    "ring-primary/60",
                    "ring-offset-2",
                    "ring-offset-background"
                );
                onDone?.();
            }, 1800);
        }, 60);

        return () => {
            window.clearTimeout(scrollTimer);
            if (highlightTimer) window.clearTimeout(highlightTimer);
        };
    }, [anchor, depKey]);
}
