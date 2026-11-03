import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Suspense, lazy, useEffect, useMemo, useState } from "react";
import "./App.css";
import "./mobile/mobile.css";
import { useIsMobilePlatform } from "./hooks/use-platform";
import { useVisualViewportInsets } from "./hooks/useVisualViewportInsets";
import MobileShell from "./mobile/MobileShell";
import LoadingScreen from "./components/LoadingScreen";
import { Toaster } from "./components/ui/sonner.tsx";

// 按需加载各个窗口，避免一次性打包全部页面导致移动端首屏加载过慢
const windowLoaders: Record<string, () => Promise<{ default: React.ComponentType<any> }>> = {
    ask: () => import("./windows/AskWindow"),
    config: () => import("./windows/ConfigWindow"),
    chat_ui: () => import("./windows/ChatUIWindow"),
    butler_experiment: () => import("./windows/ButlerExperimentWindow"),
    artifact_preview: () => import("./windows/ArtifactPreviewWindow"),
    plugin: () => import("./windows/PluginWindow"),
    schedule: () => import("./windows/ScheduleWindow"),
    artifact_collections: () => import("./windows/ArtifactCollectionsWindow"),
    artifact: () => import("./windows/ArtifactWindow"),
    sidebar: () => import("./windows/SidebarWindow"),
};

function App() {
    const isMobilePlatform = useIsMobilePlatform();
    useVisualViewportInsets(isMobilePlatform);
    const [winLabel, setWinLabel] = useState<string>("chat_ui");

    useEffect(() => {
        try {
            const win = getCurrentWebviewWindow();
            if (win?.label) {
                setWinLabel(win.label);
            }
        } catch (err) {
            console.error("Failed to get current webview window label, fallback to chat_ui", err);
            setWinLabel("chat_ui");
        }
    }, []);

    const WindowComponent = useMemo(() => {
        const loader = windowLoaders[winLabel];
        return loader ? lazy(loader) : null;
    }, [winLabel]);

    return (
        <>
            <Suspense fallback={<LoadingScreen />}>{isMobilePlatform ? <MobileShell /> : WindowComponent ? <WindowComponent /> : <div>未知窗口类型: {winLabel}</div>}</Suspense>
            <Toaster richColors />
        </>
    );
}

export default App;
