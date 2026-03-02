import { useCallback, useEffect, useState, useRef } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import ChatUIToolbar from "../components/ChatUIToolbar";
import ConversationList from "../components/ConversationList";
import ChatUIInfomation from "../components/ChatUIInfomation";
import ConversationSearchDialog from "../components/ConversationSearchDialog";
import ConversationUI, {
    ConversationUIRef,
    type InlineInteractionItem,
} from "../components/ConversationUI";
import {
    AcpPermissionDialog,
    OperationPermissionDialog,
} from "../components/OperationPermissionDialog";
import {
    AskUserQuestionCard,
    PreviewFileCard,
} from "../components/InlineInteractionCards";
import { Conversation, ConversationSearchHit } from "../data/Conversation";
import { useTheme } from "../hooks/useTheme";
import { useIsMobile } from "../hooks/use-mobile";
import {
    useAcpPermission,
    useOperationPermission,
} from "../hooks/useOperationPermission";
import { useAskUserQuestion, usePreviewFile } from "../hooks/useInlineInteraction";
import { useFeatureConfig } from "../hooks/feature/useFeatureConfig";
import { useAppShortcuts } from "../hooks/useAppShortcuts";
import { AntiLeakageProvider } from "../contexts/AntiLeakageContext";
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from "../components/ui/sheet";
import { Button } from "../components/ui/button";
import { Menu, Plus } from "lucide-react";

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { pluginRuntime } from "../services/PluginRuntime";

function ChatUIWindow() {
    // 集成主题系统
    useTheme("chat_ui");

    // 防泄露模式配置
    const { getConfigValue, loadFeatureConfig } = useFeatureConfig();
    const antiLeakageEnabled = getConfigValue("anti_leakage", "enabled") === "true";

    // 检测移动端
    const isMobile = useIsMobile();
    const [sidebarOpen, setSidebarOpen] = useState(false);
    const [conversationTitle, setConversationTitle] = useState("");
    const [searchOpen, setSearchOpen] = useState(false);
    const [pendingScrollMessageId, setPendingScrollMessageId] = useState<number | null>(null);

    const [pluginList, setPluginList] = useState<any[]>([]);

    const [selectedConversation, setSelectedConversation] = useState<string>("");
    const conversationUIRef = useRef<ConversationUIRef>(null);

    // 操作权限对话框
    const { pendingRequest, isDialogOpen, decisionError, handleDecision } = useOperationPermission({
        conversationId: selectedConversation ? parseInt(selectedConversation) : undefined,
    });
    const {
        pendingRequest: pendingAcpRequest,
        isDialogOpen: isAcpDialogOpen,
        decisionError: acpDecisionError,
        handleDecision: handleAcpDecision,
    } = useAcpPermission({
        conversationId: selectedConversation ? parseInt(selectedConversation) : undefined,
    });
    const {
        pendingRequest: pendingAskUserRequest,
        isDialogOpen: isAskUserDialogOpen,
        viewMode: askUserViewMode,
        completedAnswers: askUserCompletedAnswers,
        readOnly: isAskUserReadOnly,
        callId: askUserCallId,
        messageId: askUserMessageId,
        handleSubmit: handleAskUserSubmit,
        handleCancel: handleAskUserCancel,
    } = useAskUserQuestion({
        conversationId: selectedConversation ? parseInt(selectedConversation) : undefined,
    });
    const {
        pendingRequest: pendingPreviewFileRequest,
        isDialogOpen: isPreviewFileDialogOpen,
        callId: previewFileCallId,
        messageId: previewFileMessageId,
        handleOpenChange: handlePreviewFileOpenChange,
    } = usePreviewFile({
        conversationId: selectedConversation ? parseInt(selectedConversation) : undefined,
    });
    const inlineInteractionItems: InlineInteractionItem[] = [];
    if (isAskUserDialogOpen && pendingAskUserRequest) {
        inlineInteractionItems.push({
            key: `ask-user-question-${pendingAskUserRequest.request_id}`,
            callId: askUserCallId,
            messageId: askUserMessageId,
            content: (
                <AskUserQuestionCard
                    request={pendingAskUserRequest}
                    isOpen={isAskUserDialogOpen}
                    viewMode={askUserViewMode}
                    completedAnswers={askUserCompletedAnswers}
                    readOnly={isAskUserReadOnly}
                    onSubmit={handleAskUserSubmit}
                    onCancel={handleAskUserCancel}
                />
            ),
        });
    }
    if (isPreviewFileDialogOpen && pendingPreviewFileRequest) {
        inlineInteractionItems.push({
            key: `preview-file-${pendingPreviewFileRequest.request_id}`,
            callId: previewFileCallId,
            messageId: previewFileMessageId,
            content: (
                <PreviewFileCard
                    request={pendingPreviewFileRequest}
                    isOpen={isPreviewFileDialogOpen}
                    onOpenChange={handlePreviewFileOpenChange}
                />
            ),
        });
    }
    const hasInlineInteraction = inlineInteractionItems.length > 0;

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

    const handleSearchSelect = useCallback((hit: ConversationSearchHit) => {
        setSelectedConversation(hit.conversation_id.toString());
        if (hit.message_id) {
            setPendingScrollMessageId(hit.message_id);
        }
    }, []);

    // 应用内快捷键
    useAppShortcuts("chat", {
        new: handleNewConversation,
        search: () => setSearchOpen(true),
        stats: () => conversationUIRef.current?.openStats(),
        export: () => conversationUIRef.current?.openExport(),
        settings: () => conversationUIRef.current?.openSettings(),
        toggle_sidebar: () => conversationUIRef.current?.openSidebarWindow(),
        open_sidebar_window: () => conversationUIRef.current?.openSidebarWindow(),
    });

    useEffect(() => {
        if (!pendingScrollMessageId) {
            return;
        }
        if (!selectedConversation) {
            return;
        }
        let attempts = 0;
        const handle = window.setInterval(() => {
            attempts += 1;
            if (conversationUIRef.current) {
                conversationUIRef.current.scrollToMessage(pendingScrollMessageId);
                setPendingScrollMessageId(null);
                window.clearInterval(handle);
            } else if (attempts >= 10) {
                setPendingScrollMessageId(null);
                window.clearInterval(handle);
            }
        }, 200);
        return () => window.clearInterval(handle);
    }, [pendingScrollMessageId, selectedConversation]);

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
        let mounted = true;
        const loadPlugins = async (forceReload = false) => {
            try {
                const plugins = forceReload
                    ? await pluginRuntime.reloadPlugins()
                    : await pluginRuntime.loadPlugins();
                if (mounted) {
                    setPluginList(plugins);
                }
            } catch (error) {
                console.error("[ChatUIWindow] Failed to load plugins:", error);
                if (mounted) {
                    setPluginList([]);
                }
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
                                <ChatUIInfomation showArtifacts={false} showSchedule={false} isMobile={true} />
                                <ChatUIToolbar
                                    onNewConversation={handleNewConversation}
                                    onSearch={() => setSearchOpen(true)}
                                />
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
                            inlineInteractionItems={inlineInteractionItems}
                            inlineInteractionVisible={hasInlineInteraction}
                        />
                    </div>

                    {/* 操作权限对话框 */}
                    <ConversationSearchDialog
                        open={searchOpen}
                        onOpenChange={setSearchOpen}
                        onSelectResult={handleSearchSelect}
                    />
                    <OperationPermissionDialog
                        request={pendingRequest}
                        isOpen={isDialogOpen}
                        errorMessage={decisionError}
                        onDecision={handleDecision}
                    />
                    <AcpPermissionDialog
                        request={pendingAcpRequest}
                        isOpen={isAcpDialogOpen}
                        errorMessage={acpDecisionError}
                        onDecision={handleAcpDecision}
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
                    <ChatUIToolbar
                        onNewConversation={handleNewConversation}
                        onSearch={() => setSearchOpen(true)}
                    />
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
                        inlineInteractionItems={inlineInteractionItems}
                        inlineInteractionVisible={hasInlineInteraction}
                    />
                </div>

                <ConversationSearchDialog
                    open={searchOpen}
                    onOpenChange={setSearchOpen}
                    onSelectResult={handleSearchSelect}
                />

                {/* 操作权限对话框 */}
                <OperationPermissionDialog
                    request={pendingRequest}
                    isOpen={isDialogOpen}
                    errorMessage={decisionError}
                    onDecision={handleDecision}
                />
                <AcpPermissionDialog
                    request={pendingAcpRequest}
                    isOpen={isAcpDialogOpen}
                    errorMessage={acpDecisionError}
                    onDecision={handleAcpDecision}
                />
            </div>
        </AntiLeakageProvider>
    );
}

export default ChatUIWindow;
