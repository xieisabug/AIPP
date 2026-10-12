import { MessageSquare, Settings, Sparkles } from "lucide-react";
import { Suspense, lazy, useEffect, useMemo, useState, type ComponentType } from "react";

type MobileTab = "chat_ui" | "butler_experiment" | "config";

const TAB_IDS: MobileTab[] = ["chat_ui", "butler_experiment", "config"];

function isMobileTab(label: string): label is MobileTab {
    return (TAB_IDS as string[]).includes(label);
}

const tabLoaders: Record<MobileTab, () => Promise<{ default: ComponentType<any> }>> = {
    chat_ui: () => import("./ChatUIWindow"),
    butler_experiment: () => import("./ButlerExperimentWindow"),
    config: () => import("./ConfigWindow"),
};

const TABS: { id: MobileTab; label: string; icon: ComponentType<{ className?: string }> }[] = [
    { id: "chat_ui", label: "对话", icon: MessageSquare },
    { id: "butler_experiment", label: "总管家", icon: Sparkles },
    { id: "config", label: "设置", icon: Settings },
];

function LoadingPane() {
    return (
        <div className="flex h-full w-full items-center justify-center bg-background text-muted-foreground text-sm">
            正在加载...
        </div>
    );
}

/**
 * Mobile full-client chrome: bottom tabs 对话 / 总管家 / 设置.
 * Switches the same lazy window loaders inside one webview and keeps
 * `window.__setAppWindow` in sync so existing mobile back-navigation works.
 */
export default function MobileShell() {
    const [activeTab, setActiveTab] = useState<MobileTab>("chat_ui");

    useEffect(() => {
        const previous = (window as any).__setAppWindow as ((label: string) => void) | undefined;
        (window as any).__setAppWindow = (label: string) => {
            if (isMobileTab(label)) {
                setActiveTab(label);
            }
            previous?.(label);
        };
        return () => {
            if (previous) {
                (window as any).__setAppWindow = previous;
            }
        };
    }, []);

    const switchTab = (tab: MobileTab) => {
        setActiveTab(tab);
        const setAppWindow = (window as any).__setAppWindow as ((label: string) => void) | undefined;
        setAppWindow?.(tab);
    };

    const TabComponent = useMemo(() => lazy(tabLoaders[activeTab]), [activeTab]);

    return (
        <div className="flex h-screen flex-col bg-background" data-aipp-window="mobile_shell">
            <div className="min-h-0 flex-1 overflow-hidden">
                <Suspense fallback={<LoadingPane />}>
                    <TabComponent />
                </Suspense>
            </div>
            <nav
                className="flex flex-none items-stretch justify-around border-t border-border bg-secondary pb-[env(safe-area-inset-bottom)]"
                data-aipp-slot="mobile-shell-tabs"
            >
                {TABS.map((tab) => {
                    const Icon = tab.icon;
                    const selected = activeTab === tab.id;
                    return (
                        <button
                            key={tab.id}
                            type="button"
                            onClick={() => switchTab(tab.id)}
                            className={`flex flex-1 flex-col items-center gap-0.5 py-2 text-xs ${
                                selected ? "text-foreground" : "text-muted-foreground"
                            }`}
                            aria-current={selected ? "page" : undefined}
                        >
                            <Icon className="h-5 w-5" />
                            <span>{tab.label}</span>
                        </button>
                    );
                })}
            </nav>
        </div>
    );
}
