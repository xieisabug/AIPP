import { toast } from "sonner";
import { takeSettingsRoute, type SettingsRouteRequest } from "@/lib/pendingSettingsRoute";
import React, { Suspense, useEffect, useState, useCallback, useMemo, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
    Bot,
    ChevronLeft,
    Cloud,
    Cpu,
    Database,
    Globe,
    Home,
    Info,
    Keyboard,
    LayoutGrid,
    ListChecks,
    Menu,
    MonitorSmartphone,
    Palette,
    Puzzle,
    Shield,
    Sparkles,
    Terminal,
    WandSparkles,
    type LucideIcon,
} from "lucide-react";
import { useTheme } from "../hooks/useTheme";
import { useIsMobile } from "../hooks/use-mobile";
import { useIsMobilePlatform, usePlatform } from "../hooks/use-platform";
import {
    MAC_TRAFFIC_LIGHTS_SAFE_LEFT,
    FloatingWindowControls,
    WindowTitleBar,
    dragRegionProps,
    useCustomWindowControls,
    useFramelessTitleBar,
    useMacOverlayTitleBar,
    useWindowControlsInset,
} from "../components/window/WindowTitleBar";
import { useFeatureAvailableOnPlatform } from "../lib/mobileUnsupported";
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from "../components/ui/sheet";
import { Button } from "../components/ui/button";
import { pluginRuntime } from "../services/PluginRuntime";
import {
    navigateMobile,
    popMobile,
    registerMobileBackHandler,
    subscribeMobileConfigNavigation,
} from "../mobile/navigation";
import { canLeaveConfigNow, requestConfigLeave } from "@/lib/configLeaveGuard";
import ConfigPageLoading from "../components/common/ConfigPageLoading";
import {
    backConfigSection,
    ConfigSectionContext,
    useConfigSectionDetailTitle,
} from "@/lib/configSectionNav";
import {
    resolveConfigNavigation,
    SETTINGS_GROUP_LABELS,
    type SettingsGroupId,
    type SettingsMenuId,
} from "@/lib/settingsNavigation";
import SettingsSearch from "../components/config/SettingsSearch";

// 各设置页按需动态加载，避免打开设置窗口时一次性解析全部页面代码
const LLMProviderConfig = React.lazy(() => import("../components/config/LLMProviderConfig"));
const AssistantConfig = React.lazy(() => import("../components/config/AssistantConfig"));
const ButlerPageConfig = React.lazy(() => import("../components/config/pages/ButlerPageConfig"));
const MCPConfig = React.lazy(() => import("../components/config/ConnectedAppsConfig"));
const SkillsConfig = React.lazy(() => import("../components/config/SkillsConfig"));
const PluginCenterConfig = React.lazy(() => import("../components/config/PluginCenterConfig"));
const AiAutomationConfig = React.lazy(() => import("../components/config/pages/AiAutomationConfig"));
const MobileTasksConfig = React.lazy(() => import("../components/config/MobileTasksConfig"));
const DeviceConnectionsConfig = React.lazy(() => import("../components/config/DeviceConnectionsConfig"));
const SecurityVaultConfig = React.lazy(() => import("../components/config/pages/SecurityVaultConfig"));
const DataSyncConfig = React.lazy(() => import("../components/config/pages/DataSyncConfig"));
const GeneralAppearanceConfig = React.lazy(() => import("../components/config/pages/GeneralAppearanceConfig"));
const ShortcutsPageConfig = React.lazy(() => import("../components/config/pages/ShortcutsPageConfig"));
const NetworkProxyConfig = React.lazy(() => import("../components/config/pages/NetworkProxyConfig"));
const RuntimePreviewConfig = React.lazy(() => import("../components/config/pages/RuntimePreviewConfig"));
const AboutPageConfig = React.lazy(() => import("../components/config/pages/AboutPageConfig"));

interface MenuItem {
    id: SettingsMenuId;
    name: string;
    icon: LucideIcon;
    group: SettingsGroupId;
}

const contentMap: Record<SettingsMenuId, React.ComponentType<any>> = {
    "llm-provider-config": LLMProviderConfig,
    "assistant-config": AssistantConfig,
    "butler-config": ButlerPageConfig,
    "mcp-config": MCPConfig,
    "skills-config": SkillsConfig,
    "plugins-config": PluginCenterConfig,
    "ai-automation-config": AiAutomationConfig,
    "mobile-tasks": MobileTasksConfig,
    "device-connections": DeviceConnectionsConfig,
    "vault-config": SecurityVaultConfig,
    "data-sync-config": DataSyncConfig,
    "general-config": GeneralAppearanceConfig,
    "shortcuts-config": ShortcutsPageConfig,
    "network-config": NetworkProxyConfig,
    "runtime-preview-config": RuntimePreviewConfig,
    "about-config": AboutPageConfig,
};

const GROUP_ORDER: SettingsGroupId[] = ["core", "capabilities", "privacy", "system"];

const configPageFallback = <ConfigPageLoading />;

/** Windows 宽屏设置页顶部 40px 可拖动，窗口按钮居中。 */
const WINDOWS_CONTENT_DRAG_HEIGHT = 40;
const WINDOWS_CONTROLS_CENTER_Y = 20;

function ConfigWindow() {
    useTheme("config");
    const isNarrow = useIsMobile();
    const isMobilePlatform = useIsMobilePlatform();
    const isMobile = isMobilePlatform || isNarrow;
    const platform = usePlatform();
    const framelessTitleBar = useFramelessTitleBar();
    const drag = dragRegionProps(framelessTitleBar);
    const macOverlayTitleBar = useMacOverlayTitleBar();
    const customWindowControls = useCustomWindowControls();
    const narrowHeaderInset = useWindowControlsInset(16);
    const skillsAvailable = useFeatureAvailableOnPlatform("skills_manager");
    const [sidebarOpen, setSidebarOpen] = useState(false);

    const menuList: Array<MenuItem> = useMemo(() => {
        const items: Array<MenuItem> = [
            // 核心配置
            { id: "llm-provider-config", name: "模型与提供商", icon: Cloud, group: "core" },
            { id: "assistant-config", name: "助手", icon: Bot, group: "core" },
            { id: "butler-config", name: "总管家", icon: Cpu, group: "core" },
            // 能力与扩展
            { id: "mcp-config", name: "应用与工具", icon: LayoutGrid, group: "capabilities" },
        ];
        if (skillsAvailable) {
            items.push({ id: "skills-config", name: "Skills", icon: Sparkles, group: "capabilities" });
        }
        items.push(
            { id: "plugins-config", name: "插件", icon: Puzzle, group: "capabilities" },
            { id: "ai-automation-config", name: "AI 与自动化", icon: WandSparkles, group: "capabilities" },
        );
        if (isMobilePlatform) {
            items.push({ id: "mobile-tasks", name: "本机任务", icon: ListChecks, group: "capabilities" });
        } else {
            items.push({ id: "device-connections", name: "设备互联", icon: MonitorSmartphone, group: "capabilities" });
        }
        // 隐私与数据
        items.push(
            { id: "vault-config", name: "安全与凭据", icon: Shield, group: "privacy" },
            { id: "data-sync-config", name: "数据与同步", icon: Database, group: "privacy" },
        );
        // 通用与系统
        items.push({ id: "general-config", name: "常规与外观", icon: Palette, group: "system" });
        if (!isMobilePlatform) {
            items.push({ id: "shortcuts-config", name: "快捷键", icon: Keyboard, group: "system" });
        }
        items.push({ id: "network-config", name: "网络与代理", icon: Globe, group: "system" });
        if (!isMobilePlatform) {
            items.push({ id: "runtime-preview-config", name: "运行环境与预览", icon: Terminal, group: "system" });
        }
        items.push({ id: "about-config", name: "关于", icon: Info, group: "system" });
        return items;
    }, [skillsAvailable, isMobilePlatform]);

    const availableMenuSet = useMemo(
        () => new Set<string>(menuList.map((item) => item.id)),
        [menuList]
    );

    const groupedMenus = useMemo(
        () =>
            GROUP_ORDER.map((groupId) => ({
                id: groupId,
                label: SETTINGS_GROUP_LABELS[groupId],
                items: menuList.filter((item) => item.group === groupId),
            })).filter((group) => group.items.length > 0),
        [menuList]
    );

    const [selectedMenu, setSelectedMenu] = useState<SettingsMenuId>("llm-provider-config");
    const selectedMenuRef = useRef<SettingsMenuId>(selectedMenu);
    selectedMenuRef.current = selectedMenu;
    const sectionDetailTitle = useConfigSectionDetailTitle(selectedMenu);

    const goToSection = useCallback((menu: SettingsMenuId) => {
        setSelectedMenu(menu);
    }, []);

    const [visitedMenus, setVisitedMenus] = useState<SettingsMenuId[]>(["llm-provider-config"]);
    useEffect(() => {
        if (isMobilePlatform) {
            setVisitedMenus((previous) =>
                previous.includes(selectedMenu) ? previous : [...previous, selectedMenu]
            );
        }
    }, [selectedMenu, isMobilePlatform]);

    useEffect(() => {
        window.dispatchEvent(new CustomEvent("aipp:config-menu-activated", { detail: selectedMenu }));
    }, [selectedMenu]);

    useEffect(() => {
        if (!menuList.some((item) => item.id === selectedMenu)) {
            setSelectedMenu(menuList[0]?.id ?? "llm-provider-config");
        }
    }, [menuList, selectedMenu]);

    const [subNav, setSubNav] = useState<string | undefined>(undefined);
    const [pluginList, setPluginList] = useState<any[]>([]);

    const navigateResolved = useCallback(
        (rawMenu: string, rawSubNav?: string) => {
            const resolved = resolveConfigNavigation(rawMenu, rawSubNav);
            if (!resolved || !contentMap[resolved.menu]) return;
            if (!availableMenuSet.has(resolved.menu)) return;
            requestConfigLeave(() => {
                setSubNav(resolved.subNav);
                goToSection(resolved.menu);
            });
        },
        [availableMenuSet, goToSection]
    );

    useEffect(() => {
        const unlistenHidden = listen("config-window-hidden", () => {
            setSelectedMenu("llm-provider-config");
            setSubNav(undefined);
            setSidebarOpen(false);
        });
        return () => {
            unlistenHidden.then((unlistenFn) => unlistenFn());
        };
    }, []);

    useEffect(() => {
        if (!isMobilePlatform) return;
        return subscribeMobileConfigNavigation(({ menu, subNav: sub }) => {
            navigateResolved(menu, sub);
        });
    }, [isMobilePlatform, navigateResolved]);

    const navigateRef = useRef(navigateResolved);
    navigateRef.current = navigateResolved;
    useEffect(() => {
        const configWindow = getCurrentWebviewWindow();
        if (configWindow.label !== "config") return;
        let active = true;
        const readDestination = async () => {
            if (!active) return;
            try {
                const target = await invoke<{ menu: string; subNav?: string } | null>("take_config_navigation");
                if (active && target) navigateRef.current(target.menu, target.subNav);
            } catch (error) {
                if (active) toast.error(`读取设置导航失败：${error}`);
            }
        };
        // 先订阅再读一次，覆盖窗口仍在启动和窗口已打开两种情况。
        const subscription = configWindow.listen("config-navigation-pending", readDestination);
        void subscription.then(readDestination).catch(error => {
            if (active) toast.error(`订阅设置导航失败：${error}`);
        });
        return () => {
            active = false;
            void subscription.then(unlisten => unlisten(), () => {});
        };
    }, []);

    useEffect(() => {
        let active = true;
        const consume = (event?: SettingsRouteRequest) => {
            if (!active) return;
            const route = takeSettingsRoute(event);
            if (!route) return;
            navigateResolved(route.menu, route.subNav);
            if (route.notice) toast.info(route.notice);
        };
        const unlistenNav = listen<SettingsRouteRequest>("config-navigate-to", (event) => consume(event.payload));
        void unlistenNav.then(() => consume());
        return () => {
            active = false;
            unlistenNav.then((fn) => fn());
        };
    }, [navigateResolved]);

    useEffect(() => {
        if (!isMobilePlatform) return;
        return registerMobileBackHandler("config", () =>
            backConfigSection(selectedMenuRef.current) || !canLeaveConfigNow(popMobile)
        );
    }, [isMobilePlatform]);

    useEffect(() => {
        let mounted = true;
        const loadPlugins = async (forceReload = false) => {
            try {
                const plugins = forceReload
                    ? await pluginRuntime.reloadPlugins()
                    : await pluginRuntime.loadPlugins();
                if (mounted) setPluginList(plugins);
            } catch (error) {
                console.error("[ConfigWindow] Failed to load plugins:", error);
                if (mounted) setPluginList([]);
            }
        };
        loadPlugins();

        const unlistenRegistryChanged = listen("plugin_registry_changed", () => {
            loadPlugins(true);
        });

        return () => {
            mounted = false;
            unlistenRegistryChanged.then((unlisten) => unlisten());
        };
    }, []);

    const SelectedComponent = contentMap[selectedMenu];

    const navigateTo = useCallback(
        (menuKey: string, nextSubNav?: string) => {
            navigateResolved(menuKey, nextSubNav);
        },
        [navigateResolved]
    );

    const stablePluginList = useMemo(() => pluginList, [pluginList]);

    const renderMenuItems = (onSelect: (id: SettingsMenuId, nextSubNav?: string) => void, compactSearch = false) => (
        <div className="flex flex-col gap-4">
            <SettingsSearch
                isMobilePlatform={isMobilePlatform}
                availableMenus={availableMenuSet}
                compact={compactSearch}
                onSelect={(menu, targetSubNav) => onSelect(menu, targetSubNav)}
            />
            <nav aria-label="设置分区" className="flex flex-col gap-4">
                {groupedMenus.map((group) => (
                    <div key={group.id} className="space-y-1">
                        <div className="px-2.5 text-[11px] font-medium tracking-wide text-muted-foreground/80 select-none">
                            {group.label}
                        </div>
                        <div className="flex flex-col gap-0.5">
                            {group.items.map((item) => {
                                const active = selectedMenu === item.id;
                                return (
                                    <button
                                        type="button"
                                        key={item.id}
                                        title={item.name}
                                        aria-current={active ? "page" : undefined}
                                        className={`
                                            aipp-icon-button relative flex w-full items-center text-left px-2.5 md:px-3 py-2 rounded-lg cursor-pointer
                                            transition-colors duration-150 font-medium ${isMobilePlatform ? "min-h-11 text-sm" : "text-xs md:text-[13px]"}
                                            select-none
                                            ${active
                                                ? "bg-primary/10 text-primary font-semibold"
                                                : "text-muted-foreground hover:bg-muted/60 hover:text-foreground"
                                            }
                                        `}
                                        onClick={() => onSelect(item.id)}
                                    >
                                        {active && (
                                            <div className="absolute left-0 top-1/2 -translate-y-1/2 w-0.5 h-4 bg-primary rounded-r-sm" />
                                        )}
                                        <item.icon className="mr-2.5 size-4 shrink-0" />
                                        <span className="truncate">{item.name}</span>
                                    </button>
                                );
                            })}
                        </div>
                    </div>
                ))}
            </nav>
        </div>
    );

    const handleBackHome = async () => {
        if (isMobilePlatform) {
            requestConfigLeave(() => navigateMobile("chat_ui"));
            return;
        }
        try {
            await invoke("open_chat_ui_window");
            const current = getCurrentWebviewWindow();
            await current.close();
        } catch (error) {
            console.error("Failed to open chat UI window:", error);
        }
    };

    const currentSectionName = menuList.find((item) => item.id === selectedMenu)?.name ?? "设置";
    const handleHeaderBack = () => {
        if (backConfigSection(selectedMenu)) return;
        requestConfigLeave(popMobile);
    };

    if (isMobile) {
        return (
            <div className="flex flex-col h-screen bg-background" data-aipp-window="config" data-aipp-slot="window-root">
                <div
                    className={`flex select-none items-center justify-between px-4 py-3 bg-secondary border-b border-border safe-top${platform === "android" ? " safe-top-android" : ""}`}
                    style={{
                        ...narrowHeaderInset,
                        ...(macOverlayTitleBar ? { paddingLeft: MAC_TRAFFIC_LIGHTS_SAFE_LEFT } : undefined),
                    }}
                    {...drag}
                    data-aipp-slot="config-mobile-header"
                >
                    <Sheet open={sidebarOpen} onOpenChange={setSidebarOpen}>
                        {isMobilePlatform ? (
                            <>
                                <Button
                                    variant="ghost"
                                    size="icon"
                                    onClick={handleHeaderBack}
                                    aria-label={sectionDetailTitle !== null ? "返回列表" : "退出设置"}
                                    data-aipp-slot="config-mobile-back"
                                >
                                    <ChevronLeft className="h-5 w-5" />
                                </Button>
                                <button
                                    type="button"
                                    className="min-h-11 min-w-0 flex-1 truncate px-2 text-center text-sm font-medium"
                                    onClick={() => setSidebarOpen(true)}
                                    aria-label={`${sectionDetailTitle || currentSectionName}，打开设置菜单`}
                                >
                                    {sectionDetailTitle ? (
                                        <span className="flex min-w-0 flex-col items-center leading-tight">
                                            <span className="max-w-full truncate">{sectionDetailTitle}</span>
                                            <span className="max-w-full truncate text-[11px] font-normal text-muted-foreground">
                                                {currentSectionName}
                                            </span>
                                        </span>
                                    ) : (
                                        currentSectionName
                                    )}
                                </button>
                                <SheetTrigger asChild>
                                    <Button variant="ghost" size="icon" aria-label="设置菜单">
                                        <Menu className="h-5 w-5" />
                                    </Button>
                                </SheetTrigger>
                            </>
                        ) : (
                            <SheetTrigger asChild>
                                <Button iconLabel="打开设置菜单" variant="ghost" size="icon">
                                    <Menu className="h-5 w-5" />
                                </Button>
                            </SheetTrigger>
                        )}
                        <SheetContent
                            side="left"
                            className={`w-[280px] p-0 flex flex-col safe-top${platform === "android" ? " safe-top-android" : ""}`}
                            aria-describedby={undefined}
                            hideCloseButton
                            data-aipp-slot="config-mobile-sidebar"
                        >
                            <SheetTitle className="sr-only">设置导航</SheetTitle>
                            <div
                                className="aipp-settings-menu-container flex-1 bg-muted/30 px-3 py-4 overflow-y-auto"
                                data-theme-slot="settings-menu-container"
                                data-aipp-slot="config-menu"
                            >
                                {renderMenuItems((id, nextSubNav) => {
                                    requestConfigLeave(() => {
                                        setSubNav(nextSubNav);
                                        goToSection(id);
                                        setSidebarOpen(false);
                                    });
                                }, true)}
                            </div>
                        </SheetContent>
                    </Sheet>
                    {!isMobilePlatform && (
                        <>
                            <span
                                className="font-medium text-sm truncate flex-1 text-center select-none"
                                {...drag}
                                data-aipp-slot="config-mobile-title"
                            >
                                {currentSectionName}
                            </span>
                            <Button variant="ghost" size="icon" onClick={handleBackHome} aria-label="返回首页">
                                <Home className="h-5 w-5" />
                            </Button>
                        </>
                    )}
                </div>
                <FloatingWindowControls centerY={30} right={16} />

                <div className="flex-1 min-h-0 overflow-hidden flex flex-col bg-card" data-aipp-slot="config-content">
                    <Suspense fallback={configPageFallback}>
                        {isMobilePlatform ? (
                            visitedMenus.map((menu) => {
                                const Page = contentMap[menu];
                                return (
                                    <div key={menu} hidden={menu !== selectedMenu} className="h-full min-h-0">
                                        <ConfigSectionContext.Provider value={menu}>
                                            <Page
                                                pluginList={stablePluginList}
                                                navigateTo={navigateTo}
                                                subNav={menu === selectedMenu ? subNav : undefined}
                                                onSubNavConsumed={() => setSubNav(undefined)}
                                            />
                                        </ConfigSectionContext.Provider>
                                    </div>
                                );
                            })
                        ) : (
                            <ConfigSectionContext.Provider value={selectedMenu}>
                                <SelectedComponent
                                    pluginList={stablePluginList}
                                    navigateTo={navigateTo}
                                    subNav={subNav}
                                    onSubNavConsumed={() => setSubNav(undefined)}
                                />
                            </ConfigSectionContext.Provider>
                        )}
                    </Suspense>
                </div>
            </div>
        );
    }

    return (
        <div className="flex justify-center items-center h-screen bg-background" data-aipp-window="config" data-aipp-slot="window-root">
            <div
                className="bg-card shadow-none w-full h-screen grid grid-cols-[220px_minmax(0,1fr)] lg:grid-cols-[240px_minmax(0,1fr)]"
                {...drag}
                data-aipp-slot="config-layout"
            >
                <div
                    className={`aipp-settings-menu-container bg-muted/30 border-r border-border px-3 ${macOverlayTitleBar ? "pt-0 pb-6" : customWindowControls ? "pt-5 pb-6" : "py-5"} overflow-y-auto`}
                    data-theme-slot="settings-menu-container"
                    data-aipp-slot="config-menu"
                >
                    {macOverlayTitleBar && <WindowTitleBar className="-mx-3 mb-2" data-aipp-slot="config-titlebar" />}
                    {renderMenuItems((id, nextSubNav) => {
                        requestConfigLeave(() => {
                            setSubNav(nextSubNav);
                            goToSection(id);
                        });
                    })}
                </div>

                <div className="bg-card min-h-0 overflow-hidden flex flex-col" data-aipp-slot="config-content">
                    {customWindowControls && (
                        <div
                            className="shrink-0"
                            style={{ height: WINDOWS_CONTENT_DRAG_HEIGHT }}
                            data-tauri-drag-region
                            data-aipp-slot="config-content-titlebar"
                        >
                            <FloatingWindowControls centerY={WINDOWS_CONTROLS_CENTER_Y} right={12} />
                        </div>
                    )}
                    <Suspense fallback={configPageFallback}>
                        <ConfigSectionContext.Provider value={selectedMenu}>
                            <SelectedComponent
                                pluginList={stablePluginList}
                                navigateTo={navigateTo}
                                subNav={subNav}
                                onSubNavConsumed={() => setSubNav(undefined)}
                            />
                        </ConfigSectionContext.Provider>
                    </Suspense>
                </div>
            </div>
        </div>
    );
}

export default ConfigWindow;
