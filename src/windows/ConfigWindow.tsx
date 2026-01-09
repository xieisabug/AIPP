import React, { ReactNode, useEffect, useState, useCallback, useMemo } from "react";
import ReactDOM from "react-dom";
import { listen } from "@tauri-apps/api/event";
import LLMProviderConfig from "../components/config/LLMProviderConfig";
import AssistantConfig from "../components/config/AssistantConfig";
import FeatureAssistantConfig from "../components/config/FeatureAssistantConfig";
import MCPConfig from "../components/config/MCPConfig";
import SkillsConfig from "../components/config/SkillsConfig";
import { appDataDir } from "@tauri-apps/api/path";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Bot, ServerCrash, Settings, Sparkles } from "lucide-react";
import { useTheme } from "../hooks/useTheme";
import { useIsMobile } from "../hooks/use-mobile";
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from "../components/ui/sheet";
import { Button } from "../components/ui/button";
import { Home, Menu } from "lucide-react";
import MCP from "../assets/mcp.svg?react";

interface MenuItem {
    id: string;
    name: string;
    icon: ReactNode;
    iconSelected: ReactNode;
}

// 将 contentMap 修改为映射到组件而不是元素
const contentMap: Record<string, React.ComponentType<any>> = {
    "llm-provider-config": LLMProviderConfig,
    "assistant-config": AssistantConfig,
    "feature-assistant-config": FeatureAssistantConfig,
    "mcp-config": MCPConfig,
    "skills-config": SkillsConfig,
};

function ConfigWindow() {
    // 集成主题系统
    useTheme();
    const isMobile = useIsMobile();
    const [sidebarOpen, setSidebarOpen] = useState(false);

    const menuList: Array<MenuItem> = [
        {
            id: "llm-provider-config",
            name: "模型提供商",
            icon: <ServerCrash className="w-full h-full text-muted-foreground" />,
            iconSelected: <ServerCrash className="w-full h-full text-foreground" />,
        },
        {
            id: "assistant-config",
            name: "个人助手",
            icon: <Bot className="w-full h-full text-muted-foreground" />,
            iconSelected: <Bot className="w-full h-full text-foreground" />,
        },
        {
            id: "mcp-config",
            name: "MCP",
            icon: <MCP className="w-full h-full text-muted-foreground" />,
            iconSelected: <MCP className="w-full h-full text-foreground" />,
        },
        {
            id: "skills-config",
            name: "Skills",
            icon: <Sparkles className="w-full h-full text-muted-foreground" />,
            iconSelected: <Sparkles className="w-full h-full text-foreground" />,
        },
        {
            id: "feature-assistant-config",
            name: "程序功能",
            icon: <Settings className="w-full h-full text-muted-foreground" />,
            iconSelected: <Settings className="w-full h-full text-foreground" />,
        },
    ];

    const [selectedMenu, setSelectedMenu] = useState<string>("llm-provider-config");
    const [pluginList, setPluginList] = useState<any[]>([]);

    // 监听窗口隐藏事件，重置状态准备下次打开
    useEffect(() => {
        const unlistenHidden = listen("config-window-hidden", () => {
            console.log("ConfigWindow hidden, resetting state");
            // 重置到默认菜单
            setSelectedMenu("llm-provider-config");
            setSidebarOpen(false);
        });

        return () => {
            unlistenHidden.then((unlistenFn) => unlistenFn());
        };
    }, []);

    useEffect(() => {
        // 为可能使用 UMD 构建的插件提供全局 React/ReactDOM（与 PluginWindow 保持一致）
        (window as any).React = React;
        (window as any).ReactDOM = ReactDOM;

        const toPascalCase = (str: string) =>
            str
                .replace(/(^|[-_\s]+)([a-zA-Z0-9])/g, (_, __, c) => (c ? String(c).toUpperCase() : ""))
                .replace(/[^a-zA-Z0-9]/g, "");

        const pluginLoadList = [
            {
                name: "代码生成",
                code: "code-generate",
                pluginType: ["assistantType"],
                instance: null,
            },
            {
                name: "DeepResearch",
                code: "deepresearch",
                pluginType: ["assistantType"],
                instance: null,
            }
        ];

        const initPlugin = async () => {
            const dirPath = await appDataDir();
            pluginLoadList.forEach(async (plugin) => {
                const convertFilePath = dirPath + "/plugin/" + plugin.code + "/dist/main.js";

                // 加载脚本
                const script = document.createElement("script");
                script.src = convertFileSrc(convertFilePath);
                script.onload = () => {
                    // 脚本加载完成后，插件应该可以在全局范围内使用
                    const g: any = window as any;
                    const candidates = [g.SamplePlugin, g[plugin.code], g[toPascalCase(plugin.code)]];
                    const PluginCtor = candidates.find((c) => typeof c === "function");
                    if (PluginCtor) {
                        try {
                            const instance = new PluginCtor();
                            plugin.instance = instance;
                            console.debug(`[PluginLoader][Config] '${plugin.code}' instance created`);
                        } catch (e) {
                            console.error(`[PluginLoader][Config] Failed to instantiate '${plugin.code}':`, e);
                        }
                    } else {
                        console.warn(
                            `[PluginLoader][Config] No global constructor for '${plugin.code}'. Checked: SamplePlugin, ${plugin.code}, ${toPascalCase(
                                plugin.code
                            )}`
                        );
                    }
                };
                document.body.appendChild(script);
            });

            setPluginList(pluginLoadList);
        };

        initPlugin();
    }, []);

    // 获取选中的组件
    const SelectedComponent = contentMap[selectedMenu];

    // 导航函数 - 使用 useCallback 稳定化引用
    const navigateTo = useCallback((menuKey: string) => {
        if (contentMap[menuKey]) {
            setSelectedMenu(menuKey);
        }
    }, []);

    // 稳定化 pluginList 引用
    const stablePluginList = useMemo(() => pluginList, [pluginList]);

    const renderMenuItems = (onSelect: (id: string) => void) => (
        <div className="flex flex-col gap-1 mt-2">
            {menuList.map((item, index) => (
                <div
                    key={index}
                    className={`
                                    relative flex items-center px-3 md:px-4 lg:px-5 py-3 md:py-3.5 rounded-lg cursor-pointer
                                    transition-all duration-200 ease-out font-medium text-xs md:text-sm
                                    select-none hover:translate-x-0.5
                                    ${selectedMenu === item.id
                            ? "bg-primary/10 text-primary font-semibold shadow-sm"
                            : "text-muted-foreground hover:bg-muted/50 hover:text-foreground"
                        }
                                `}
                    onClick={() => onSelect(item.id)}
                >
                    {/* 选中状态的左侧指示条 */}
                    {selectedMenu === item.id && (
                        <div className="absolute left-0 top-1/2 -translate-y-1/2 w-0.5 h-5 bg-primary rounded-r-sm" />
                    )}
                    <div className="flex items-center">
                        <div className="w-4 h-4 md:w-5 md:h-5 flex-shrink-0 mr-2 md:mr-3 lg:mr-3.5">
                            {selectedMenu === item.id ? item.iconSelected : item.icon}
                        </div>
                        <span className="truncate">{item.name}</span>
                    </div>
                </div>
            ))}
        </div>
    );

    const handleBackHome = async () => {
        // 移动端优先在单 webview 内直接切换视图，避免多窗口体验割裂
        if (isMobile) {
            const switchWindow = (window as any).__setAppWindow as ((label: string) => void) | undefined;
            if (switchWindow) {
                switchWindow("chat_ui");
                return;
            }
        }

        // 桌面端保持原先多窗口逻辑
        try {
            await invoke("open_chat_ui_window");

            // 关闭当前设置窗口以返回 Chat UI（比 hide 更可靠，适配移动端）
            const current = getCurrentWebviewWindow();
            await current.close();
        } catch (error) {
            console.error("Failed to open chat UI window:", error);
        }
    };

    // 移动端布局：顶部栏 + 侧滑菜单
    if (isMobile) {
        return (
            <div className="flex flex-col h-screen bg-background">
                <div
                    className="flex items-center justify-between px-4 py-3 bg-secondary border-b border-border"
                    data-tauri-drag-region
                >
                    <Sheet open={sidebarOpen} onOpenChange={setSidebarOpen}>
                        <SheetTrigger asChild>
                            <Button variant="ghost" size="icon">
                                <Menu className="h-5 w-5" />
                            </Button>
                        </SheetTrigger>
                        <SheetContent
                            side="left"
                            className="w-[280px] p-0 flex flex-col"
                            aria-describedby={undefined}
                            hideCloseButton
                        >
                            <SheetTitle className="sr-only">设置导航</SheetTitle>
                            <div className="bg-muted/30 border-border border-b px-3 py-4 overflow-y-auto">
                                {renderMenuItems((id) => {
                                    setSelectedMenu(id);
                                    setSidebarOpen(false);
                                })}
                            </div>
                        </SheetContent>
                    </Sheet>
                    <span className="font-medium text-sm truncate flex-1 text-center">设置</span>
                    <Button variant="ghost" size="icon" onClick={handleBackHome} aria-label="返回首页">
                        <Home className="h-5 w-5" />
                    </Button>
                </div>

                <div className="flex-1 overflow-auto bg-card px-4 py-4">
                    <SelectedComponent
                        pluginList={stablePluginList}
                        navigateTo={navigateTo}
                    />
                </div>
            </div>
        );
    }

    return (
        <div className="flex justify-center items-center h-screen bg-background">
            <div
                className="bg-card shadow-lg w-full h-screen grid grid-cols-[1fr_3fr] md:grid-cols-[1fr_4fr] lg:grid-cols-[1fr_5fr]"
                data-tauri-drag-region
            >
                {/* 侧边栏 */}
                <div className="bg-muted/30 border-r border-border px-3 md:px-4 py-6 overflow-y-auto">
                    {renderMenuItems(setSelectedMenu)}
                </div>

                {/* 内容区域 */}
                <div className="bg-card px-4 md:px-6 lg:px-8 py-6 overflow-y-auto max-h-screen">
                    {/* 配置组件内容 */}
                    <SelectedComponent pluginList={stablePluginList} navigateTo={navigateTo} />
                </div>
            </div>
        </div>
    );
}

export default ConfigWindow;
