import React, { useCallback, useEffect, useState, useRef } from "react";
import ReactDOM from "react-dom";
import { emit, listen } from "@tauri-apps/api/event";
import ChatUIToolbar from "../components/ChatUIToolbar";
import ConversationList from "../components/ConversationList";
import ChatUIInfomation from "../components/ChatUIInfomation";
import ConversationUI, { ConversationUIRef } from "../components/ConversationUI";
import { OperationPermissionDialog } from "../components/OperationPermissionDialog";
import { Conversation } from "../data/Conversation";
import { useTheme } from "../hooks/useTheme";
import { useIsMobile } from "../hooks/use-mobile";
import { useOperationPermission } from "../hooks/useOperationPermission";
import { useFeatureConfig } from "../hooks/feature/useFeatureConfig";
import { AntiLeakageProvider } from "../contexts/AntiLeakageContext";
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from "../components/ui/sheet";
import { Button } from "../components/ui/button";
import { Menu, Plus } from "lucide-react";

import { appDataDir } from "@tauri-apps/api/path";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
// 导入内置 ACP 插件
import acpAssistantTypePlugin from "../plugins/acp/AcpAssistantTypePlugin";

function ChatUIWindow() {
    // 集成主题系统
    useTheme();

    // 防泄露模式配置
    const { getConfigValue, loadFeatureConfig } = useFeatureConfig();
    const antiLeakageEnabled = getConfigValue("anti_leakage", "enabled") === "true";

    // 检测移动端
    const isMobile = useIsMobile();
    const [sidebarOpen, setSidebarOpen] = useState(false);
    const [conversationTitle, setConversationTitle] = useState("");

    const [pluginList, setPluginList] = useState<any[]>([]);

    const [selectedConversation, setSelectedConversation] = useState<string>("");
    const conversationUIRef = useRef<ConversationUIRef>(null);

    // 操作权限对话框
    const { pendingRequest, isDialogOpen, handleDecision } = useOperationPermission({
        conversationId: selectedConversation ? parseInt(selectedConversation) : undefined,
    });

    // 移动端选择对话后自动关闭侧边栏
    const handleSelectConversation = useCallback(
        (conversationId: string) => {
            setSelectedConversation(conversationId);
            if (isMobile) {
                setSidebarOpen(false);
            }
        },
        [isMobile]
    );

    // 移动端新对话后自动关闭侧边栏
    const handleNewConversation = useCallback(() => {
        setSelectedConversation("");
        if (isMobile) {
            setSidebarOpen(false);
        }
    }, [isMobile]);

    const handleConversationChange = useCallback((conv?: Conversation) => {
        setConversationTitle(conv?.name || "");
    }, []);

    // 组件挂载完成后，发送窗口加载事件，通知 AskWindow
    useEffect(() => {
        emit("chat-ui-window-load");

        // 监听 AskWindow 发来的选中对话事件 (只注册一次)
        const unlisten = getCurrentWebviewWindow().listen("select_conversation", (event) => {
            const convId = event.payload;
            if (convId && convId !== "") {
                setSelectedConversation(convId as string);
            }
        });

        // 监听窗口焦点变化
        const windowFocusUnlisten = getCurrentWebviewWindow().onFocusChanged(({ payload: focused }) => {
            if (focused) {
                // 窗口获得焦点时，使用 requestAnimationFrame 聚焦到输入框
                requestAnimationFrame(() => {
                    conversationUIRef.current?.focus();
                });
            }
        });

        // 监听窗口隐藏事件，重置状态准备下次打开
        const unlistenHidden = listen("chat-ui-window-hidden", () => {
            console.log("ChatUIWindow hidden, resetting state");
            // 重置选中的对话，下次打开时显示新对话界面
            setSelectedConversation("");
            setConversationTitle("");
            setSidebarOpen(false);
        });

        return () => {
            unlisten.then((unlisten) => unlisten());
            windowFocusUnlisten.then((unlistenFn) => unlistenFn());
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
            },
            // 内置 ACP 插件
            {
                name: "ACP Assistant Type",
                code: "acp",
                pluginType: ["assistantType"],
                instance: acpAssistantTypePlugin,
            },
        ];

        const initPlugin = async () => {
            const dirPath = await appDataDir();
            const loadPromises = pluginLoadList.map(async (plugin) => {
                const convertFilePath = dirPath + "/plugin/" + plugin.code + "/dist/main.js";

                return new Promise<void>((resolve) => {
                    const script = document.createElement("script");
                    script.src = convertFileSrc(convertFilePath);
                    script.onload = () => {
                        const g: any = window as any;
                        const candidates = [g.SamplePlugin, g[plugin.code], g[toPascalCase(plugin.code)]];
                        const PluginCtor = candidates.find((c) => typeof c === "function");
                        if (PluginCtor) {
                            try {
                                plugin.instance = new PluginCtor();
                                console.debug(`[PluginLoader] '${plugin.code}' instance created via global constructor`);
                            } catch (e) {
                                console.error(`[PluginLoader] Failed to instantiate '${plugin.code}' SamplePlugin:`, e);
                            }
                        } else {
                            console.warn(
                                `[PluginLoader] No global plugin constructor found for '${plugin.code}'. Checked: SamplePlugin, ${plugin.code}, ${toPascalCase(
                                    plugin.code
                                )}. Check plugin UMD global name.`
                            );
                        }
                        resolve();
                    };
                    script.onerror = (error) => {
                        console.error("Failed to load plugin script", plugin.name, error);
                        resolve();
                    };
                    document.body.appendChild(script);
                });
            });

            // 等待所有插件加载完成
            await Promise.all(loadPromises);

            // 所有插件实例都准备好后再更新状态
            setPluginList([...pluginLoadList]);
        };

        initPlugin();
    }, []);

    // 监听配置变更事件，当 Settings 窗口修改配置后重新加载
    useEffect(() => {
        const unlisten = listen("feature_config_changed", () => {
            console.log("[ChatUI] feature_config_changed event received, reloading config");
            loadFeatureConfig();
        });
        return () => {
            unlisten.then((f) => f());
        };
    }, [loadFeatureConfig]);

    // 移动端布局
    if (isMobile) {
        const mobileTitle = conversationTitle || "新会话";

        return (
            <AntiLeakageProvider enabled={antiLeakageEnabled}>
                <div className="flex flex-col h-screen bg-background">
                    {/* 移动端顶部栏 */}
                    <div className="flex-none flex items-center justify-between px-4 py-3 bg-secondary border-b border-border">
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
                                <SheetTitle className="sr-only">导航菜单</SheetTitle>
                                <ChatUIInfomation showArtifacts={false} showPluginStore={false} isMobile={true} />
                                <ChatUIToolbar onNewConversation={handleNewConversation} />
                                <ConversationList
                                    conversationId={selectedConversation}
                                    onSelectConversation={handleSelectConversation}
                                />
                            </SheetContent>
                        </Sheet>
                        <span className="font-medium text-sm truncate flex-1 text-center mx-3">{mobileTitle}</span>
                        <Button variant="ghost" size="icon" onClick={handleNewConversation} aria-label="新建对话">
                            <Plus className="h-5 w-5" />
                        </Button>
                    </div>

                    {/* 主内容区域 */}
                    <div className="flex-1 overflow-hidden">
                        <ConversationUI
                            ref={conversationUIRef}
                            pluginList={pluginList}
                            conversationId={selectedConversation}
                            onChangeConversationId={setSelectedConversation}
                            isMobile={true}
                            onConversationChange={handleConversationChange}
                        />
                    </div>

                    {/* 操作权限对话框 */}
                    <OperationPermissionDialog
                        request={pendingRequest}
                        isOpen={isDialogOpen}
                        onDecision={handleDecision}
                    />
                </div>
            </AntiLeakageProvider>
        );
    }

    // 桌面端布局
    return (
        <AntiLeakageProvider enabled={antiLeakageEnabled}>
            <div className="flex h-screen bg-background">
                <div className="flex-none w-[280px] flex flex-col shadow-lg box-border rounded-r-xl mb-2 mr-2">
                    <ChatUIInfomation />
                    <ChatUIToolbar onNewConversation={handleNewConversation} />
                    <ConversationList
                        conversationId={selectedConversation}
                        onSelectConversation={handleSelectConversation}
                    />
                </div>

                <div className="flex-1 bg-background overflow-auto rounded-xl m-2 ml-0 shadow-lg">
                    <ConversationUI
                        ref={conversationUIRef}
                        pluginList={pluginList}
                        conversationId={selectedConversation}
                        onChangeConversationId={setSelectedConversation}
                    />
                </div>

                {/* 操作权限对话框 */}
                <OperationPermissionDialog
                    request={pendingRequest}
                    isOpen={isDialogOpen}
                    onDecision={handleDecision}
                />
            </div>
        </AntiLeakageProvider>
    );
}

export default ChatUIWindow;
