import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Suspense, lazy, useEffect, useMemo, useState } from "react";
import "./App.css";
import { Toaster } from "./components/ui/sonner.tsx";

// 按需加载各个窗口，避免一次性打包全部页面导致移动端首屏加载过慢
const windowLoaders: Record<string, () => Promise<{ default: React.ComponentType<any> }>> = {
    ask: () => import("./windows/AskWindow"),
    config: () => import("./windows/ConfigWindow"),
    chat_ui: () => import("./windows/ChatUIWindow"),
    artifact_preview: () => import("./windows/ArtifactPreviewWindow"),
    plugin: () => import("./windows/PluginWindow"),
    schedule: () => import("./windows/ScheduleWindow"),
    artifact_collections: () => import("./windows/ArtifactCollectionsWindow"),
    artifact: () => import("./windows/ArtifactWindow"),
    sidebar: () => import("./windows/SidebarWindow"),
};

function App() {
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

        // 提供可切换窗口的全局方法，便于移动端在单 webview 内切换视图
        (window as any).__setAppWindow = (label: string) => {
            setWinLabel(label);
        };

        return () => {
            if ((window as any).__setAppWindow) {
                delete (window as any).__setAppWindow;
            }
        };
    }, []);

    const WindowComponent = useMemo(() => {
        const loader = windowLoaders[winLabel];
        return loader ? lazy(loader) : null;
    }, [winLabel]);

    const LoadingScreen = () => (
        <div
            style={{
                width: "100vw",
                height: "100vh",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                background: "var(--loading-bg)",
                color: "var(--loading-text)",
                transition: "background-color 0.2s ease, color 0.2s ease",
            }}
        >
            <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: "12px" }}>
                <div
                    style={{
                        width: "42px",
                        height: "42px",
                        border: "4px solid var(--loading-spinner-bg)",
                        borderTop: "4px solid var(--loading-spinner-accent)",
                        borderRadius: "50%",
                        animation: "spin 1s linear infinite",
                    }}
                />
                <div style={{ fontSize: "14px" }}>正在加载...</div>
            </div>
        </div>
    );

    return (
        <>
            <Suspense fallback={<LoadingScreen />}>{WindowComponent ? <WindowComponent /> : <div>未知窗口类型: {winLabel}</div>}</Suspense>
            <Toaster richColors />
        </>
    );
}

export default App;
