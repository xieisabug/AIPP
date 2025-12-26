import { PackageOpen, Settings, Store } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import AnimatedLogo from "./AnimatedLogo";
import { useLogoState } from "../hooks/useLogoState";
import { Button } from "./ui/button";
import { memo } from "react";

interface ChatUIInfomationProps {
    showArtifacts?: boolean;
    showPluginStore?: boolean;
    /** 移动端模式，使用内部视图切换而非多窗口 */
    isMobile?: boolean;
}

const ChatUIInfomation = memo(function ChatUIInfomation({
    showArtifacts = true,
    showPluginStore = true,
    isMobile = false,
}: ChatUIInfomationProps) {
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

    const openPluginStore = async () => {
        try {
            await invoke("open_plugin_store_window");
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
                <Button onClick={openConfig} variant={"ghost"}>
                    <Settings />
                </Button>
                {showArtifacts && (
                    <Button onClick={openArtifactsCollections} variant={"ghost"}>
                        <PackageOpen />
                    </Button>
                )}
                {showPluginStore && (
                    <Button onClick={openPluginStore} variant={"ghost"}>
                        <Store />
                    </Button>
                )}
            </div>
        </div>
    );
});

ChatUIInfomation.displayName = "ChatUIInfomation";

export default ChatUIInfomation;
