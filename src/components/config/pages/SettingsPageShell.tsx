import React from "react";
import { ConfigFooterLayout } from "@/components/common/ConfigSaveFooter";

interface SettingsPageShellProps {
    title: string;
    description?: string;
    headerExtra?: React.ReactNode;
    children: React.ReactNode;
}

/**
 * Unified full-width container for flat top-level settings pages.
 * Aligns on the exact same horizontal/vertical padding grid as split-pane and catalog pages.
 */
export const SettingsPageShell: React.FC<SettingsPageShellProps> = ({
    title,
    description,
    headerExtra,
    children,
}) => {
    return (
        <div className="flex h-full min-h-0 flex-col bg-background">
            <ConfigFooterLayout>
                <div className="min-h-0 flex-1 overflow-y-auto">
                    <div className="w-full space-y-6 px-4 py-6 sm:px-6">
                        <header className="flex flex-wrap items-start justify-between gap-4">
                            <div className="min-w-0">
                                <h2 className="text-xl font-semibold tracking-tight">{title}</h2>
                                {description ? (
                                    <p className="mt-1 text-sm text-muted-foreground">{description}</p>
                                ) : null}
                            </div>
                            {headerExtra ? <div className="flex shrink-0 items-center gap-2">{headerExtra}</div> : null}
                        </header>
                        {children}
                    </div>
                </div>
            </ConfigFooterLayout>
        </div>
    );
};

export default SettingsPageShell;
