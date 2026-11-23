import { PackageOpen, Settings, CalendarClock, Eye, EyeOff } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import AnimatedLogo from "./AnimatedLogo";
import { AppUpdateNotice } from "./AppUpdateNotice";
import { useLogoState } from "../hooks/useLogoState";
import { useAntiLeakage } from "../contexts/AntiLeakageContext";
import { Button } from "./ui/button";
import { memo, type ReactNode } from "react";
import { navigateMobile } from "../mobile/navigation";
import { useIsMobilePlatform } from "../hooks/use-platform";
import { useFeatureAvailableOnPlatform } from "../lib/mobileUnsupported";
import { WindowTitleBar, useWindowTitleBarLayout } from "./window/WindowTitleBar";
import { cn } from "../utils/utils";

interface ChatUIInfomationProps {
    onNavigate?: () => void;
    showArtifacts?: boolean;
    showSchedule?: boolean;
    /** 窄屏布局模式（来自宽度），用于样式；导航切换另看 preferInAppNavigation / 平台 */
    isMobile?: boolean;
    /**
     * 是否优先在单 WebView 内切换视图（移动平台必须为 true）。
     * 未传时回退到 useIsMobilePlatform()，避免仅靠宽度误开多窗口。
     */
    preferInAppNavigation?: boolean;
    /** 桌面标题栏行里的主操作（搜索、新对话）：macOS 与红黄绿按钮同排；移动端不渲染，由窗口单独放置。 */
    titleBarActions?: ReactNode;
}

const ChatUIInfomation = memo(function ChatUIInfomation({
    showArtifacts = true,
    showSchedule = true,
    onNavigate,
    isMobile = false,
    titleBarActions,
}: ChatUIInfomationProps) {
    const { enabled: antiLeakageEnabled, isRevealed, toggleReveal } = useAntiLeakage();
    const isMobilePlatform = useIsMobilePlatform();
    const multiWindowAvailable = useFeatureAvailableOnPlatform("desktop_multi_window");
    // 桌面统一布局下工具按钮与 Logo 同排，改用与 Logo 等高的紧凑图标按钮
    // 窄屏抽屉里仍用单行布局（搜索与新对话由抽屉单独成行）
    const titleBarLayout = useWindowTitleBarLayout() && !isMobile;
    const actionButtonProps = titleBarLayout ? { size: "icon" as const, className: "size-8" } : {};
    // 导航取决于运行平台，桌面窄窗口仍使用桌面窗口。
    const useInAppNav = isMobilePlatform;

    // 移动平台隐藏桌面多窗口入口（即使父组件误传 show*=true）
    const canShowArtifacts = showArtifacts && (multiWindowAvailable || isMobilePlatform);
    const canShowSchedule = showSchedule && multiWindowAvailable;

    const {
        state: logoState,
        showHappy,
        showError,
        showNormal,
    } = useLogoState({
        defaultState: "happy",
        autoReturnToNormal: true,
        autoReturnDelay: 3000,
    });

    const openConfig = async () => {
        // 移动平台 / 单 WebView：内部视图切换，禁止尝试创建第二窗口
        if (useInAppNav) {
            onNavigate?.();
            navigateMobile("config");
            showHappy();
            return;
        }

        // 桌面端使用多窗口
        try {
            await invoke("open_config_window");
            showHappy();
        } catch (error) {
            showError();
        }
    };

    const openArtifactsCollections = async () => {
        if (isMobilePlatform) { onNavigate?.(); navigateMobile("collections"); return; }
        try {
            await invoke("open_artifact_collections_window");
            showHappy();
        } catch (error) {
            showError();
        }
    };

    const openScheduleWindow = async () => {
        try {
            await invoke("open_schedule_window");
            showHappy();
        } catch (error) {
            showError();
        }
    };

    const logo = (
        <div className="flex items-center gap-2 bg-secondary" data-aipp-slot="chat-info-logo">
            <AnimatedLogo state={logoState} size={32} onClick={showNormal} />
        </div>
    );
    const actions = (
        <div className={cn("flex flex-wrap items-center justify-end", titleBarLayout ? "gap-0.5" : "gap-1")} data-aipp-slot="chat-info-actions">
            {/* 防泄露模式眼睛图标按钮 */}
            {antiLeakageEnabled && (
                <Button
                    onClick={toggleReveal}
                    variant={"ghost"}
                    title={isRevealed ? "隐藏原文" : "显示原文"}
                    {...actionButtonProps} data-aipp-slot="chat-info-toggle-anti-leakage"
                >
                    {isRevealed ? <EyeOff /> : <Eye />}
                </Button>
            )}
            <Button onClick={openConfig} variant={"ghost"} {...actionButtonProps} data-aipp-slot="chat-info-open-config">
                <Settings />
            </Button>
            <AppUpdateNotice />
            {canShowArtifacts && (
                <Button onClick={openArtifactsCollections} variant={"ghost"} {...actionButtonProps} data-aipp-slot="chat-info-open-artifacts">
                    <PackageOpen />
                </Button>
            )}
            {canShowSchedule && (
                <Button onClick={openScheduleWindow} variant={"ghost"} {...actionButtonProps} data-aipp-slot="chat-info-open-schedule">
                    <CalendarClock />
                </Button>
            )}
        </div>
    );

    if (titleBarLayout) {
        // 桌面统一布局：第一行是标题栏行（macOS 左侧为红黄绿按钮）+ 搜索与新对话，
        // 第二行左侧 Logo、右侧工具按钮；两行空白处都可拖动窗口。
        return (
            <div className="flex shrink-0 flex-col border-border bg-secondary" data-aipp-slot="chat-info-bar">
                <WindowTitleBar data-aipp-slot="chat-info-titlebar">{titleBarActions}</WindowTitleBar>
                <div className="flex items-center justify-between gap-2 pb-3 pl-5 pr-2" data-tauri-drag-region data-aipp-slot="chat-info-brand">
                    {logo}
                    {actions}
                </div>
            </div>
        );
    }

    return (
        <div className="flex justify-between py-4 px-5 border-border bg-secondary" data-aipp-slot="chat-info-bar">
            {logo}
            {actions}
        </div>
    );
});

ChatUIInfomation.displayName = "ChatUIInfomation";

export default ChatUIInfomation;
