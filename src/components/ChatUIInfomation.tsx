import { PackageOpen, Settings, CalendarClock, Eye, EyeOff } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import AnimatedLogo from "./AnimatedLogo";
import { useLogoState } from "../hooks/useLogoState";
import { useAntiLeakage } from "../contexts/AntiLeakageContext";
import { Button } from "./ui/button";
import { memo } from "react";
import { useIsMobilePlatform } from "../hooks/use-platform";
import { useFeatureAvailableOnPlatform } from "../lib/mobileUnsupported";

interface ChatUIInfomationProps {
    showArtifacts?: boolean;
    showSchedule?: boolean;
    /** 窄屏布局模式（来自宽度），用于样式；导航切换另看 preferInAppNavigation / 平台 */
    isMobile?: boolean;
    /**
     * 是否优先在单 WebView 内切换视图（移动平台必须为 true）。
     * 未传时回退到 useIsMobilePlatform()，避免仅靠宽度误开多窗口。
     */
    preferInAppNavigation?: boolean;
}

const ChatUIInfomation = memo(function ChatUIInfomation({
    showArtifacts = true,
    showSchedule = true,
    isMobile = false,
    preferInAppNavigation,
}: ChatUIInfomationProps) {
    const { enabled: antiLeakageEnabled, isRevealed, toggleReveal } = useAntiLeakage();
    const isMobilePlatform = useIsMobilePlatform();
    const multiWindowAvailable = useFeatureAvailableOnPlatform("desktop_multi_window");
    // 移动平台必须单 WebView 内切换；窄屏沿用既有 in-app 行为；显式 prefer 亦可强制开启
    const useInAppNav = isMobilePlatform || isMobile || preferInAppNavigation === true;

    // 移动平台隐藏桌面多窗口入口（即使父组件误传 show*=true）
    const canShowArtifacts = showArtifacts && multiWindowAvailable;
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
            const switchWindow = (window as any).__setAppWindow as ((label: string) => void) | undefined;
            if (switchWindow) {
                switchWindow("config");
                showHappy();
                return;
            }
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

    return (
        <div className="flex justify-between py-4 px-5 border-border bg-secondary " data-aipp-slot="chat-info-bar">
            <div className="flex items-center gap-2 bg-secondary" data-aipp-slot="chat-info-logo">
                <AnimatedLogo state={logoState} size={32} onClick={showNormal} />
            </div>
            <div className="flex items-center gap-2" data-aipp-slot="chat-info-actions">
                {/* 防泄露模式眼睛图标按钮 */}
                {antiLeakageEnabled && (
                    <Button
                        onClick={toggleReveal}
                        variant={"ghost"}
                        title={isRevealed ? "隐藏原文" : "显示原文"}
                        data-aipp-slot="chat-info-toggle-anti-leakage"
                    >
                        {isRevealed ? <EyeOff /> : <Eye />}
                    </Button>
                )}
                <Button onClick={openConfig} variant={"ghost"} data-aipp-slot="chat-info-open-config">
                    <Settings />
                </Button>
                {canShowArtifacts && (
                    <Button onClick={openArtifactsCollections} variant={"ghost"} data-aipp-slot="chat-info-open-artifacts">
                        <PackageOpen />
                    </Button>
                )}
                {canShowSchedule && (
                    <Button onClick={openScheduleWindow} variant={"ghost"} data-aipp-slot="chat-info-open-schedule">
                        <CalendarClock />
                    </Button>
                )}
            </div>
        </div>
    );
});

ChatUIInfomation.displayName = "ChatUIInfomation";

export default ChatUIInfomation;
