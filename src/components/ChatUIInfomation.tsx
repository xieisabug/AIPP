import { PackageOpen, Settings, CalendarClock, Eye, EyeOff } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import AnimatedLogo from "./AnimatedLogo";
import { useLogoState } from "../hooks/useLogoState";
import { useAntiLeakage } from "../contexts/AntiLeakageContext";
import { Button } from "./ui/button";
import { memo } from "react";

interface ChatUIInfomationProps {
    showArtifacts?: boolean;
    showSchedule?: boolean;
    /** 移动端模式，使用内部视图切换而非多窗口 */
    isMobile?: boolean;
}

const ChatUIInfomation = memo(function ChatUIInfomation({
    showArtifacts = true,
    showSchedule = true,
    isMobile = false,
}: ChatUIInfomationProps) {
    const { enabled: antiLeakageEnabled, isRevealed, toggleReveal } = useAntiLeakage();

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
        // 移动端使用内部视图切换
        if (isMobile) {
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
        <div className="flex justify-between py-4 px-5 border-border bg-secondary ">
            <div className="flex items-center gap-2 bg-secondary">
                <AnimatedLogo state={logoState} size={32} onClick={showNormal} />
            </div>
            <div className="flex items-center gap-2">
                {/* 防泄露模式眼睛图标按钮 */}
                {antiLeakageEnabled && (
                    <Button
                        onClick={toggleReveal}
                        variant={"ghost"}
                        title={isRevealed ? "隐藏原文" : "显示原文"}
                    >
                        {isRevealed ? <EyeOff /> : <Eye />}
                    </Button>
                )}
                <Button onClick={openConfig} variant={"ghost"}>
                    <Settings />
                </Button>
                {showArtifacts && (
                    <Button onClick={openArtifactsCollections} variant={"ghost"}>
                        <PackageOpen />
                    </Button>
                )}
                {showSchedule && (
                    <Button onClick={openScheduleWindow} variant={"ghost"}>
                        <CalendarClock />
                    </Button>
                )}
            </div>
        </div>
    );
});

ChatUIInfomation.displayName = "ChatUIInfomation";

export default ChatUIInfomation;
