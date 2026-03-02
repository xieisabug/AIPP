import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import mermaid from "mermaid";
import ReactMarkdown from "react-markdown";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import remarkMath from "remark-math";
import remarkBreaks from "remark-breaks";
import remarkCustomCompenent from "@/react-markdown/remarkCustomComponent";
import remarkCodeBlockMeta from "@/react-markdown/remarkCodeBlockMeta";
import rehypeKatex from "rehype-katex";
import rehypeRaw from "rehype-raw";
import TipsComponent from "@/react-markdown/components/TipsComponent";
import { customUrlTransform } from "@/constants/markdown";
import { resolveCodeBlockMeta } from "@/react-markdown/remarkCodeBlockMeta";
import "../styles/ArtifactPreviewWIndow.css";
import "katex/dist/katex.min.css";
import EnvironmentInstallDialog from "../components/EnvironmentInstallDialog";
import { useTheme } from "../hooks/useTheme";
import { formatIconDisplay } from "@/utils/emojiUtils";
import { useArtifactEvents, ArtifactData, EnvironmentCheckData } from "../hooks/useArtifactEvents";
import { useArtifactBridge } from "../hooks/useArtifactBridge";

interface ArtifactInfo {
    id: number;
    name: string;
    icon: string;
    description: string;
    type: string;
    original_code: string;
    tags?: string;
    created_time: string;
    last_used_time?: string;
    use_count: number;
}

/**
 * 仅用于 "artifact" 窗口。
 * - 监听后端发出的 artifact-log / artifact-error / artifact-success 事件并展示。
 * - 使用 iframe 沙盒展示预览内容，避免页面跳转导致监听器失效。
 * - 显示模式：先显示加载界面，预览准备好后切换到全屏预览
 */
export default function ArtifactWindow() {
    // 集成主题系统
    useTheme("artifact");

    const [previewUrl, setPreviewUrl] = useState<string | null>(null);
    const [isPreviewReady, setIsPreviewReady] = useState(false);
    const [currentView, setCurrentView] = useState<"loading" | "preview">("loading");
    const [previewType, setPreviewType] = useState<
        "react" | "vue" | "mermaid" | "html" | "svg" | "xml" | "markdown" | "md" | null
    >(null);
    const [artifactInfo, setArtifactInfo] = useState<ArtifactInfo | null>(null);
    const previewTypeRef = useRef<"react" | "vue" | "mermaid" | "html" | "svg" | "xml" | "markdown" | "md" | null>(
        null
    );
    const mermaidContainerRef = useRef<HTMLDivElement | null>(null);
    const [mermaidContent, setMermaidContent] = useState<string>("");
    const [htmlContent, setHtmlContent] = useState<string>("");
    const [markdownContent, setMarkdownContent] = useState<string>("");
    const [mermaidScale, setMermaidScale] = useState<number>(1);
    const [mermaidPosition, setMermaidPosition] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
    const [isDragging, setIsDragging] = useState<boolean>(false);
    const [dragStart, setDragStart] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
    const [isSpacePressed, setIsSpacePressed] = useState<boolean>(false);
    const isInstalling = useRef<boolean>(false);
    const previewIframeRef = useRef<HTMLIFrameElement>(null);

    // 环境安装相关状态
    const [showEnvironmentDialog, setShowEnvironmentDialog] = useState<boolean>(false);
    const [environmentTool, setEnvironmentTool] = useState<string>("");
    const [environmentMessage, setEnvironmentMessage] = useState<string>("");
    const [currentLang, setCurrentLang] = useState<string>("");
    const [currentInputStr, setCurrentInputStr] = useState<string>("");

    // Artifact Bridge 配置
    const [bridgeConfig, setBridgeConfig] = useState<{ db_id?: string; assistant_id?: number }>({});

    // 使用 refs 来存储最新的值，避免闭包陷阱
    const currentLangRef = useRef<string>("");
    const currentInputStrRef = useRef<string>("");

    // 同步 previewType 到 ref
    useEffect(() => {
        previewTypeRef.current = previewType;
    }, [previewType]);

    // 同步 currentLang 和 currentInputStr 到 refs
    useEffect(() => {
        currentLangRef.current = currentLang;
        currentInputStrRef.current = currentInputStr;
    }, [currentLang, currentInputStr]);

    // ====== Artifact Bridge ======
    // 集成 postMessage 桥接，允许 artifact 访问数据库和 AI 助手
    useArtifactBridge({
        iframeRef: previewIframeRef,
        config: bridgeConfig,
        allowedOrigins: ['http://localhost', 'http://127.0.0.1'],
    });

    // 处理 artifact 数据
    const handleArtifactData = useCallback((data: ArtifactData) => {
        console.log("[ArtifactWindow] 接收到 artifact 数据：", data);
        
        // 存储完整的 artifact 信息
        setArtifactInfo(data as unknown as ArtifactInfo);

        // 更新 bridge 配置（用于数据库和 AI 助手访问）
        setBridgeConfig({
            db_id: data.db_id,
            assistant_id: data.assistant_id,
        });

        if (data.original_code && data.type) {
            switch (data.type) {
                case "vue":
                case "react":
                    setPreviewType(data.type as "vue" | "react");
                    break;
                case "mermaid":
                    setPreviewType("mermaid");
                    setMermaidContent(data.original_code);
                    setIsPreviewReady(true);
                    break;
                case "html":
                    setPreviewType("html");
                    setHtmlContent(data.original_code);
                    setIsPreviewReady(true);
                    break;
                case "svg":
                    setPreviewType("svg");
                    setHtmlContent(data.original_code);
                    setIsPreviewReady(true);
                    break;
                case "xml":
                    setPreviewType("xml");
                    setHtmlContent(data.original_code);
                    setIsPreviewReady(true);
                    break;
                case "markdown":
                case "md":
                    setPreviewType(data.type as "markdown" | "md");
                    setMarkdownContent(data.original_code);
                    setIsPreviewReady(true);
                    break;
                default:
                    break;
            }
        }
    }, []);

    // 处理重定向
    const handleRedirect = useCallback((url: string) => {
        setPreviewUrl(url);
        setIsPreviewReady(true);
    }, []);

    // 处理环境检查
    const handleEnvironmentCheck = useCallback((data: EnvironmentCheckData) => {
        setEnvironmentTool(data.tool);
        setEnvironmentMessage(data.message);
        setCurrentLang(data.lang);
        setCurrentInputStr(data.input_str);
        setShowEnvironmentDialog(true);
    }, []);

    // 处理环境安装开始
    const handleEnvironmentInstallStarted = useCallback((data: { tool: string; lang: string; input_str: string }) => {
        setCurrentLang(data.lang);
        setCurrentInputStr(data.input_str);
        isInstalling.current = true;
        setShowEnvironmentDialog(false);
    }, []);

    // 处理 Bun 安装完成
    const handleBunInstallFinished = useCallback((success: boolean) => {
        console.log("🔧 [ArtifactWindow] 收到Bun安装完成事件:", success, isInstalling.current);
        if (success && isInstalling.current) {
            artifactEvents.addLog("success", "Bun 安装成功，正在重新启动预览...");
            invoke("retry_preview_after_install", {
                lang: currentLangRef.current,
                inputStr: currentInputStrRef.current,
            })
                .then(() => {
                    isInstalling.current = false;
                })
                .catch((error) => {
                    artifactEvents.addLog("error", `重新启动预览失败: ${error}`);
                    isInstalling.current = false;
                });
        } else if (!success) {
            artifactEvents.addLog("error", "Bun 安装失败");
            isInstalling.current = false;
        }
    }, []);

    // 处理 uv 安装完成
    const handleUvInstallFinished = useCallback((success: boolean) => {
        if (success && isInstalling.current) {
            artifactEvents.addLog("success", "uv 安装成功，正在重新启动预览...");
            invoke("retry_preview_after_install", {
                lang: currentLangRef.current,
                inputStr: currentInputStrRef.current,
            })
                .then(() => {
                    isInstalling.current = false;
                })
                .catch((error) => {
                    artifactEvents.addLog("error", `重新启动预览失败: ${error}`);
                    isInstalling.current = false;
                });
        } else if (!success) {
            artifactEvents.addLog("error", "uv 安装失败");
            isInstalling.current = false;
        }
    }, []);

    // 使用统一的事件处理 hook
    const artifactEvents = useArtifactEvents({
        windowType: "artifact",
        onArtifactData: handleArtifactData,
        onRedirect: handleRedirect,
        onEnvironmentCheck: handleEnvironmentCheck,
        onEnvironmentInstallStarted: handleEnvironmentInstallStarted,
        onBunInstallFinished: handleBunInstallFinished,
        onUvInstallFinished: handleUvInstallFinished,
    });

    // 初始化 mermaid - 根据主题动态配置
    useEffect(() => {
        // 检测当前主题
        const isDark = document.documentElement.classList.contains("dark");

        mermaid.initialize({
            startOnLoad: false,
            theme: isDark ? "dark" : "default",
            securityLevel: "loose",
            fontFamily: "monospace",
            themeVariables: {
                darkMode: isDark,
            },
        });
    }, []);

    // 渲染 mermaid 图表
    useEffect(() => {
        // 确保在预览视图且是 mermaid 类型时才渲染
        if (previewType === "mermaid" && currentView === "preview" && mermaidContent && mermaidContainerRef.current) {
            const renderMermaid = async () => {
                try {
                    const container = mermaidContainerRef.current;
                    if (!container) return;

                    // 找到内部的可缩放容器
                    const innerContainer = container.querySelector("div > div") as HTMLDivElement;
                    if (!innerContainer) return;

                    // 清空容器
                    innerContainer.innerHTML = "";

                    // 创建一个唯一的ID
                    const id = `mermaid-${Date.now()}`;

                    // 验证 mermaid 内容
                    if (!mermaidContent.trim()) {
                        innerContainer.innerHTML = '<div class="text-red-500 p-4">Mermaid 内容为空</div>';
                        return;
                    }

                    // 渲染图表
                    const { svg } = await mermaid.render(id, mermaidContent.trim());
                    innerContainer.innerHTML = svg;

                    // 设置 SVG 样式以适应容器
                    const svgElement = innerContainer.querySelector("svg");
                    if (svgElement) {
                        svgElement.style.maxWidth = "none";
                        svgElement.style.maxHeight = "none";
                        svgElement.style.width = "auto";
                        svgElement.style.height = "auto";
                    }
                } catch (error) {
                    const container = mermaidContainerRef.current;
                    if (container) {
                        const innerContainer = container.querySelector("div > div") as HTMLDivElement;
                        if (innerContainer) {
                            innerContainer.innerHTML = `<div class="text-red-500 p-4">渲染失败: ${error}</div>`;
                        }
                    }
                }
            };

            // 延迟渲染，确保 DOM 已准备好
            setTimeout(renderMermaid, 200);
        }
    }, [previewType, currentView, mermaidContent]);

    // 处理Mermaid图表的交互事件
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.code === "Space" && previewType === "mermaid" && currentView === "preview") {
                e.preventDefault();
                setIsSpacePressed(true);
            }
        };

        const handleKeyUp = (e: KeyboardEvent) => {
            if (e.code === "Space") {
                setIsSpacePressed(false);
                setIsDragging(false);
            }
        };

        const handleWheel = (e: WheelEvent) => {
            if (
                previewType === "mermaid" &&
                currentView === "preview" &&
                mermaidContainerRef.current?.contains(e.target as Node)
            ) {
                e.preventDefault();
                const delta = e.deltaY > 0 ? -0.1 : 0.1;
                setMermaidScale((prevScale) => Math.max(0.1, Math.min(3, prevScale + delta)));
            }
        };

        document.addEventListener("keydown", handleKeyDown);
        document.addEventListener("keyup", handleKeyUp);
        document.addEventListener("wheel", handleWheel, { passive: false });

        return () => {
            document.removeEventListener("keydown", handleKeyDown);
            document.removeEventListener("keyup", handleKeyUp);
            document.removeEventListener("wheel", handleWheel);
        };
    }, [previewType, currentView]);

    // 处理鼠标拖动
    const handleMouseDown = (e: React.MouseEvent) => {
        if (isSpacePressed && previewType === "mermaid") {
            setIsDragging(true);
            setDragStart({ x: e.clientX - mermaidPosition.x, y: e.clientY - mermaidPosition.y });
        }
    };

    const handleMouseMove = (e: React.MouseEvent) => {
        if (isDragging && isSpacePressed) {
            setMermaidPosition({
                x: e.clientX - dragStart.x,
                y: e.clientY - dragStart.y,
            });
        }
    };

    const handleMouseUp = () => {
        setIsDragging(false);
    };

    // 重置Mermaid缩放和位置
    const resetMermaidView = () => {
        setMermaidScale(1);
        setMermaidPosition({ x: 0, y: 0 });
    };

    // 处理环境安装确认
    const handleEnvironmentInstallConfirm = async () => {
        try {
            await invoke("confirm_environment_install", {
                tool: environmentTool,
                confirmed: true,
                lang: currentLangRef.current,
                inputStr: currentInputStrRef.current,
            });
        } catch (error) {
            artifactEvents.addLog("error", `确认安装失败: ${error}`);
        }
    };

    // 处理环境安装取消
    const handleEnvironmentInstallCancel = async () => {
        try {
            await invoke("confirm_environment_install", {
                tool: environmentTool,
                confirmed: false,
                lang: currentLangRef.current,
                inputStr: currentInputStrRef.current,
            });
            setShowEnvironmentDialog(false);
        } catch (error) {
            artifactEvents.addLog("error", `取消安装失败: ${error}`);
        }
    };

    // 当预览准备好时，切换到预览视图
    useEffect(() => {
        if (
            isPreviewReady &&
            (previewUrl ||
                previewType === "mermaid" ||
                previewType === "html" ||
                previewType === "svg" ||
                previewType === "xml" ||
                previewType === "markdown" ||
                previewType === "md")
        ) {
            setCurrentView("preview");
        }
    }, [isPreviewReady, previewUrl, previewType]);

    // 监听窗口关闭事件，清理预览服务器
    useEffect(() => {
        const currentWindow = getCurrentWebviewWindow();
        let unlistenCloseRequested: (() => void) | null = null;
        let isCleanupDone = false;

        const cleanup = async () => {
            // 避免重复清理
            if (isCleanupDone) return;
            isCleanupDone = true;

            try {
                // 根据预览类型调用相应的关闭函数
                if (previewTypeRef.current === "vue") {
                    await invoke("close_vue_artifact", { previewId: "vue" });
                } else if (
                    previewTypeRef.current === "mermaid" ||
                    previewTypeRef.current === "html" ||
                    previewTypeRef.current === "svg" ||
                    previewTypeRef.current === "xml" ||
                    previewTypeRef.current === "markdown" ||
                    previewTypeRef.current === "md"
                ) {
                    // Mermaid/HTML/SVG/XML/Markdown 不需要服务器清理，只需要清除DOM
                } else {
                    await invoke("close_react_artifact", { previewId: "react" });
                }

                artifactEvents.clearLogs();
                setPreviewUrl(null);
                setIsPreviewReady(false);
                setCurrentView("loading");
                setPreviewType(null);
                setMermaidContent("");
                setHtmlContent("");
                setMarkdownContent("");
            } catch (error) { }
        };

        // 监听窗口关闭事件 - Tauri v2 的正确用法
        const setupCloseListener = async () => {
            try {
                unlistenCloseRequested = await currentWindow.onCloseRequested(cleanup);
            } catch (error) { }
        };

        setupCloseListener();

        // 添加组件卸载时的清理
        return () => {
            if (unlistenCloseRequested) {
                unlistenCloseRequested();
            }
            // 组件卸载时也执行清理
            if (!isCleanupDone) {
                cleanup();
            }
        };
    }, []);

    // 添加切换视图的按钮（可选）

    // 刷新iframe
    const handleRefresh = () => {
        if (previewUrl) {
            // 移除现有的_refresh参数，然后添加新的时间戳
            const url = new URL(previewUrl);
            url.searchParams.set("_refresh", Date.now().toString());
            setPreviewUrl(url.toString());
        }
    };

    // 在新窗口中查看 Mermaid（全屏/独立）
    const handleOpenMermaidInNewWindow = async () => {
        if (!mermaidContent) return;
        try {
            await invoke("run_artifacts", { lang: "mermaid", inputStr: mermaidContent });
        } catch (error) {
            artifactEvents.addLog("error", `打开预览窗口失败: ${String(error)}`);
        }
    };

    return (
        <div className="flex h-screen bg-background">
            <div className="flex flex-1 flex-col">
                <div className="flex-1 flex flex-col">
                    {currentView === "loading" ? (
                        /* Loading 视图 - 美观的加载界面 */
                        <div className="flex-1 flex flex-col items-center justify-center p-8 bg-gradient-to-br from-background to-muted/20">
                            {/* Artifact Logo 和标题 */}
                            <div className="flex flex-col items-center mb-8">
                                {/* Logo 容器 */}
                                <div className="relative mb-4">
                                    <div className="w-24 h-24 bg-primary/10 rounded-2xl flex items-center justify-center shadow-lg border border-primary/20">
                                        {/* 图标内容区域 - 只有当 artifactInfo 存在时才显示 */}
                                        <div
                                            className={`transition-all duration-500 ease-out ${artifactInfo ? "opacity-100 scale-100" : "opacity-0 scale-75"
                                                }`}
                                        >
                                            {artifactInfo?.icon && (
                                                <div className="text-4xl">
                                                    {(() => {
                                                        const iconDisplay = formatIconDisplay(artifactInfo.icon);
                                                        return iconDisplay.isImage ? (
                                                            <img
                                                                src={iconDisplay.display}
                                                                alt={`Icon for ${artifactInfo.name}`}
                                                                className="w-16 h-16 object-cover"
                                                            />
                                                        ) : (
                                                            iconDisplay.display
                                                        );
                                                    })()}
                                                </div>
                                            )}
                                        </div>
                                    </div>
                                </div>

                                {/* 标题和描述 - 只有当 artifactInfo 存在时才显示 */}
                                <div
                                    className={`text-center transition-all duration-700 ease-out delay-200 ${artifactInfo ? "opacity-100 translate-y-0" : "opacity-0 translate-y-4"
                                        }`}
                                >
                                    {artifactInfo && (
                                        <>
                                            <h1 className="text-3xl font-bold text-foreground mb-2">
                                                {artifactInfo.name}
                                            </h1>

                                            {/* 副标题 - 显示描述 */}
                                            {artifactInfo.description && (
                                                <p className="text-lg text-muted-foreground">
                                                    {artifactInfo.description}
                                                </p>
                                            )}
                                        </>
                                    )}
                                </div>
                            </div>

                            {/* Log 信息展示区域 */}
                            <div className="w-full max-w-2xl">
                                <div className="bg-card border border-border rounded-lg shadow-none overflow-hidden">
                                    <div className="px-4 py-3 text-center">
                                        {artifactEvents.logs.length === 0 ? (
                                            <div className="text-muted-foreground text-sm py-2">等待启动...</div>
                                        ) : (
                                            <div
                                                className={`text-sm font-medium transition-all duration-300 ${artifactEvents.logs[artifactEvents.logs.length - 1].type === "error"
                                                    ? "text-destructive"
                                                    : artifactEvents.logs[artifactEvents.logs.length - 1].type === "success"
                                                        ? "text-green-600 dark:text-green-400"
                                                        : "text-foreground"
                                                    }`}
                                            >
                                                {artifactEvents.logs[artifactEvents.logs.length - 1].message}
                                            </div>
                                        )}
                                    </div>
                                </div>
                            </div>

                            {/* 如果预览准备好了，显示成功状态 */}
                            {isPreviewReady &&
                                (previewUrl ||
                                    previewType === "mermaid" ||
                                    previewType === "html" ||
                                    previewType === "svg" ||
                                    previewType === "xml" ||
                                    previewType === "markdown" ||
                                    previewType === "md") && (
                                    <div className="mt-6 flex items-center space-x-3 px-4 py-3 bg-green-50 dark:bg-green-950/50 border border-green-200 dark:border-green-800 rounded-lg">
                                        <div className="w-5 h-5 bg-green-500 rounded-full flex items-center justify-center">
                                            <svg className="w-3 h-3 text-white" fill="currentColor" viewBox="0 0 20 20">
                                                <path
                                                    fillRule="evenodd"
                                                    d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z"
                                                    clipRule="evenodd"
                                                />
                                            </svg>
                                        </div>
                                        <p className="text-green-700 dark:text-green-400 font-medium">
                                            预览准备完成，即将自动切换...
                                        </p>
                                    </div>
                                )}
                        </div>
                    ) : (
                        /* 预览视图 - 根据类型显示不同内容 */
                        <div className="flex-1 flex flex-col relative">
                            {/* 悬浮刷新按钮 - 仅在支持刷新的类型中显示 */}
                            {previewType !== "mermaid" &&
                                previewType !== "html" &&
                                previewType !== "svg" &&
                                previewType !== "xml" &&
                                previewType !== "markdown" &&
                                previewType !== "md" && (
                                    <button
                                        onClick={handleRefresh}
                                        className="fixed bottom-4 right-4 w-12 h-12 bg-primary hover:bg-primary/90 text-primary-foreground shadow-lg hover:shadow-xl transition-all rounded-full flex items-center justify-center z-50"
                                        title="刷新预览"
                                    >
                                        <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                            <path
                                                strokeLinecap="round"
                                                strokeLinejoin="round"
                                                strokeWidth={2}
                                                d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
                                            />
                                        </svg>
                                    </button>
                                )}

                            {previewType === "mermaid" ? (
                                /* Mermaid 图表预览 */
                                <div className="flex-1 flex flex-col p-4">
                                    <div className="flex justify-between items-center mb-2">
                                        <div className="text-sm text-muted-foreground">
                                            缩放: {Math.round(mermaidScale * 100)}% | 提示: 滚轮缩放，空格键+拖动
                                        </div>
                                        <div className="flex items-center gap-2">
                                            <button
                                                onClick={handleOpenMermaidInNewWindow}
                                                className="px-3 py-1 bg-primary hover:bg-primary/90 text-primary-foreground text-xs rounded transition-colors"
                                            >
                                                在新窗口打开
                                            </button>
                                            <button
                                                onClick={resetMermaidView}
                                                className="px-3 py-1 bg-secondary hover:bg-secondary/80 text-secondary-foreground text-xs rounded transition-colors"
                                            >
                                                重置视图
                                            </button>
                                        </div>
                                    </div>
                                    <div
                                        ref={mermaidContainerRef}
                                        className={`flex-1 bg-background border border-border rounded-lg shadow-sm overflow-hidden relative ${isSpacePressed ? "cursor-grab" : "cursor-default"
                                            } ${isDragging ? "cursor-grabbing" : ""}`}
                                        onMouseDown={handleMouseDown}
                                        onMouseMove={handleMouseMove}
                                        onMouseUp={handleMouseUp}
                                        onMouseLeave={handleMouseUp}
                                        style={{
                                            minHeight: "400px",
                                            maxHeight: "calc(100vh - 200px)",
                                            overflow: "auto",
                                        }}
                                    >
                                        <div
                                            style={{
                                                transform: `scale(${mermaidScale}) translate(${mermaidPosition.x}px, ${mermaidPosition.y}px)`,
                                                transformOrigin: "center center",
                                                transition: isDragging ? "none" : "transform 0.1s ease-out",
                                                display: "flex",
                                                justifyContent: "center",
                                                alignItems: "center",
                                                minWidth: "100%",
                                                minHeight: "100%",
                                                padding: "20px",
                                            }}
                                        >
                                            {/* Mermaid SVG 将被渲染在这里 */}
                                        </div>
                                    </div>
                                </div>
                            ) : previewType === "markdown" || previewType === "md" ? (
                                /* Markdown 预览 */
                                <div className="flex-1 overflow-auto bg-background p-6">
                                    <div className="prose prose-lg max-w-none dark:prose-invert">
                                        {(() => {
                                            const mdComponents: any = {
                                                tipscomponent: TipsComponent,
                                                code({ className, children, node, ...props }: any) {
                                                    const match = /language-(\w+)/.exec(className || "");
                                                    const meta = resolveCodeBlockMeta(props as Record<string, unknown>, node);
                                                    const dataLanguage = typeof (props as Record<string, unknown>)["data-language"] === "string"
                                                        ? (props as Record<string, unknown>)["data-language"] as string
                                                        : undefined;
                                                    const language = match?.[1] ?? dataLanguage ?? "text";
                                                    const isInline = !match && !meta && !dataLanguage;
                                                    const metaLabel = meta
                                                        ? [meta.title || meta.filename, meta.line ? `line ${meta.line}` : null, meta.highlight ? `highlight ${meta.highlight}` : null]
                                                            .filter(Boolean)
                                                            .join(" · ")
                                                        : null;
                                                    return !isInline ? (
                                                        <div>
                                                            {metaLabel && (
                                                                <div className="mb-2 text-xs text-muted-foreground font-mono truncate" title={metaLabel}>
                                                                    {metaLabel}
                                                                </div>
                                                            )}
                                                            <SyntaxHighlighter
                                                                style={oneDark as any}
                                                                language={language}
                                                                PreTag="div"
                                                                {...props}
                                                            >
                                                                {String(children).replace(/\n$/, "")}
                                                            </SyntaxHighlighter>
                                                        </div>
                                                    ) : (
                                                        <code className={className} {...props}>
                                                            {children}
                                                        </code>
                                                    );
                                                },
                                            };
                                            return (
                                                <ReactMarkdown
                                                    remarkPlugins={[remarkMath, remarkBreaks, remarkCodeBlockMeta, remarkCustomCompenent]}
                                                    rehypePlugins={[rehypeKatex, rehypeRaw]}
                                                    components={mdComponents}
                                                    urlTransform={customUrlTransform}
                                                >
                                                    {markdownContent}
                                                </ReactMarkdown>
                                            );
                                        })()}
                                    </div>
                                </div>
                            ) : previewType === "html" || previewType === "svg" || previewType === "xml" ? (
                                /* HTML/SVG/XML 预览 */
                                <iframe
                                    srcDoc={htmlContent}
                                    className="flex-1 w-full border-0 bg-background"
                                    sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                                    style={{
                                        minHeight: "400px",
                                    }}
                                />
                            ) : (
                                /* iframe 预览 - 用于 React 和 Vue */
                                <iframe
                                    ref={previewIframeRef}
                                    src={previewUrl || ""}
                                    className="flex-1 w-full border-0"
                                    sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                                    onLoad={() => { }}
                                    onError={() => { }}
                                />
                            )}
                        </div>
                    )}
                </div>
            </div>

            {/* 环境安装确认对话框 */}
            <EnvironmentInstallDialog
                tool={environmentTool}
                message={environmentMessage}
                isOpen={showEnvironmentDialog}
                onConfirm={handleEnvironmentInstallConfirm}
                onCancel={handleEnvironmentInstallCancel}
            />
        </div>
    );
}
