import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { ArrowUpRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { navigateMobileConfig } from "@/mobile/navigation";
import type { SettingsMenuId } from "@/lib/settingsNavigation";

interface SettingsLinkProps {
    menu: SettingsMenuId | string;
    subNav?: string;
    children: React.ReactNode;
    /** Optional in-window navigate callback; falls back to `config-navigate-to` event. */
    navigateTo?: (menuKey: string, subNav?: string) => void;
    /** Set true when clicked from a non-config window (e.g., Butler window) to ensure ConfigWindow opens. */
    openWindow?: boolean;
    variant?: "link" | "outline" | "ghost";
    size?: "sm" | "default";
    className?: string;
}

export const SettingsLink: React.FC<SettingsLinkProps> = ({
    menu,
    subNav,
    children,
    navigateTo,
    openWindow = false,
    variant = "outline",
    size = "sm",
    className,
}) => {
    const isMobilePlatform = useIsMobilePlatform();

    const handleClick = async () => {
        if (navigateTo && !openWindow) {
            navigateTo(menu, subNav);
            return;
        }
        if (isMobilePlatform) {
            navigateMobileConfig(menu, subNav);
            return;
        }
        if (openWindow) {
            await emit("config-navigate-to", { menu, subNav });
            await invoke("open_config_window");
            window.setTimeout(() => {
                void emit("config-navigate-to", { menu, subNav });
            }, 60);
            return;
        }
        await emit("config-navigate-to", { menu, subNav });
    };

    if (variant === "link") {
        return (
            <button
                type="button"
                onClick={() => void handleClick()}
                className={`inline-flex items-center gap-1 text-xs font-medium text-foreground underline decoration-border underline-offset-4 hover:decoration-foreground ${className ?? ""}`}
            >
                {children}
                <ArrowUpRight className="h-3.5 w-3.5 shrink-0" />
            </button>
        );
    }

    return (
        <Button
            type="button"
            variant={variant}
            size={size}
            onClick={() => void handleClick()}
            className={`gap-1 ${className ?? ""}`}
        >
            {children}
            <ArrowUpRight className="h-3.5 w-3.5 shrink-0" />
        </Button>
    );
};

export default SettingsLink;
