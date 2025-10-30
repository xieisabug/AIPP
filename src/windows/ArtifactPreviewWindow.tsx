import { useEffect, useRef, useState } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { open } from '@tauri-apps/plugin-shell';
import mermaid from 'mermaid';
import { Streamdown } from 'streamdown';
// Removed explicit syntax highlighter; use Streamdown's built-in Shiki
import remarkMath from 'remark-math';
import remarkBreaks from 'remark-breaks';
import remarkCustomCompenent from '@/react-markdown/remarkCustomComponent';
// Use Streamdown default rehype plugins (harden/raw/katex)
import TipsComponent from '@/react-markdown/components/TipsComponent';
import '../styles/ArtifactPreviewWIndow.css';
import 'katex/dist/katex.min.css';
import EnvironmentInstallDialog from '../components/EnvironmentInstallDialog';
import SaveArtifactDialog from '../components/SaveArtifactDialog';
import { useTheme } from '../hooks/useTheme';
import { Button } from '@/components/ui/button';

interface LogLine {
    type: 'log' | 'error' | 'success';
    message: string;
}

/**
 * 仅用于 "artifact_preview" 窗口。
 * - 监听后端发出的 artifact-log / artifact-error / artifact-success 事件并展示。
 * - 使用 iframe 沙盒展示预览内容，避免页面跳转导致监听器失效。
 * - 显示模式：先显示日志，预览准备好后切换到全屏预览
 */
export default function ArtifactPreviewWindow() {
    // 集成主题系统
    useTheme();

    const [logs, setLogs] = useState<LogLine[]>([]);
    const [previewUrl, setPreviewUrl] = useState<string | null>(null);
    const [isPreviewReady, setIsPreviewReady] = useState(false);
    const [currentView, setCurrentView] = useState<'logs' | 'preview'>('logs');
    const [previewType, setPreviewType] = useState<'react' | 'vue' | 'mermaid' | 'html' | 'svg' | 'xml' | 'markdown' | 'md' | null>(null);
    const logsEndRef = useRef<HTMLDivElement | null>(null);
    const unlistenersRef = useRef<(() => void)[]>([]);
    const isRegisteredRef = useRef(false);
    const previewTypeRef = useRef<'react' | 'vue' | 'mermaid' | 'html' | 'svg' | 'xml' | 'markdown' | 'md' | null>(null);
    const mermaidContainerRef = useRef<HTMLDivElement | null>(null);
    const [mermaidContent, setMermaidContent] = useState<string>('');
    const [htmlContent, setHtmlContent] = useState<string>('');
    const [markdownContent, setMarkdownContent] = useState<string>('');
    const [mermaidScale, setMermaidScale] = useState<number>(1);
    const [mermaidPosition, setMermaidPosition] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
    const [isDragging, setIsDragging] = useState<boolean>(false);
    const [dragStart, setDragStart] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
    const [isSpacePressed, setIsSpacePressed] = useState<boolean>(false);
    const isInstalling = useRef<boolean>(false);

    // 环境安装相关状态
    const [showEnvironmentDialog, setShowEnvironmentDialog] = useState<boolean>(false);
    const [environmentTool, setEnvironmentTool] = useState<string>('');
    const [environmentMessage, setEnvironmentMessage] = useState<string>('');
    const [currentLang, setCurrentLang] = useState<string>('');
    const [currentInputStr, setCurrentInputStr] = useState<string>('');

    // 保存 artifact 相关状态
    const [showSaveDialog, setShowSaveDialog] = useState<boolean>(false);
    const [originalCode, setOriginalCode] = useState<string>(''); // 存储原始代码

    // 使用 refs 来存储最新的值，避免闭包陷阱
    const currentLangRef = useRef<string>('');
    const currentInputStrRef = useRef<string>('');

    // 同步 previewType 到 ref
    useEffect(() => {
        previewTypeRef.current = previewType;
    }, [previewType]);

    // 同步 currentLang 和 currentInputStr 到 refs
    useEffect(() => {
        currentLangRef.current = currentLang;
        currentInputStrRef.current = currentInputStr;
    }, [currentLang, currentInputStr]);

    // 通知其他窗口：预览窗口已准备就绪（避免事件丢失）
    useEffect(() => {
        // 仅在挂载后发一次就绪事件
        emit('artifact-preview-ready');
    }, []);

    // 初始化 mermaid - 根据主题动态配置
    useEffect(() => {
        // 检测当前主题
        const isDark = document.documentElement.classList.contains('dark');

        mermaid.initialize({
            startOnLoad: false,
            theme: isDark ? 'dark' : 'default',
            securityLevel: 'loose',
            fontFamily: 'monospace',
            themeVariables: {
                darkMode: isDark,
            },
            // 确保 SVG 有明确的尺寸
            flowchart: {
                useMaxWidth: false,
            }
        });
    }, []);

    // 自动滚动到底部
    useEffect(() => {
        logsEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [logs]);

    // 渲染 mermaid 图表
    useEffect(() => {

        // 确保在预览视图且是 mermaid 类型时才渲染
        if (previewType === 'mermaid' && currentView === 'preview' && mermaidContent && mermaidContainerRef.current) {
            const renderMermaid = async () => {
                try {
                    const container = mermaidContainerRef.current;
                    if (!container) return;

                    // 找到内部的可缩放容器
                    const innerContainer = container.querySelector('div > div') as HTMLDivElement;
                    if (!innerContainer) return;

                    // 清空容器
                    innerContainer.innerHTML = '';

                    // 创建一个唯一的ID
                    const id = `mermaid-${Date.now()}`;

                    // 验证 mermaid 内容
                    if (!mermaidContent.trim()) {
                        innerContainer.innerHTML = '<div class="text-red-500 p-4">Mermaid 内容为空</div>';
                        return;
                    }

                    // 渲染图表
                    const { svg } = await mermaid.render(id, mermaidContent.trim());
                    
                    // 设置 innerHTML 前先确保容器可见
                    innerContainer.style.width = '100%';
                    innerContainer.style.minHeight = '400px';
                    innerContainer.innerHTML = svg;

                    // 设置 SVG 样式以适应容器
                    const svgElement = innerContainer.querySelector('svg');
                    if (svgElement) {
                        // 确保 SVG 可见：保留原始尺寸或设置默认尺寸
                        const width = svgElement.getAttribute('width');
                        const height = svgElement.getAttribute('height');
                        const viewBox = svgElement.getAttribute('viewBox');
                        
                        // 如果没有 viewBox，尝试从 width/height 创建
                        if (!viewBox && width && height) {
                            svgElement.setAttribute('viewBox', `0 0 ${width} ${height}`);
                        }
                        
                        // 移除固定的 width 和 height 属性，让 CSS 控制
                        svgElement.removeAttribute('width');
                        svgElement.removeAttribute('height');
                        
                        // 设置样式以确保 SVG 可见且响应式
                        svgElement.style.width = '100%';
                        svgElement.style.height = 'auto';
                        svgElement.style.maxWidth = '100%';
                        svgElement.style.display = 'block';
                        svgElement.style.margin = '0 auto';
                    }
                } catch (error) {
                    const container = mermaidContainerRef.current;
                    if (container) {
                        const innerContainer = container.querySelector('div > div') as HTMLDivElement;
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
            if (e.code === 'Space' && previewType === 'mermaid' && currentView === 'preview') {
                e.preventDefault();
                setIsSpacePressed(true);
            }
        };

        const handleKeyUp = (e: KeyboardEvent) => {
            if (e.code === 'Space') {
                setIsSpacePressed(false);
                setIsDragging(false);
            }
        };

        const handleWheel = (e: WheelEvent) => {
            if (previewType === 'mermaid' && currentView === 'preview' && mermaidContainerRef.current?.contains(e.target as Node)) {
                e.preventDefault();
                const delta = e.deltaY > 0 ? -0.1 : 0.1;
                setMermaidScale(prevScale => Math.max(0.1, Math.min(3, prevScale + delta)));
            }
        };

        document.addEventListener('keydown', handleKeyDown);
        document.addEventListener('keyup', handleKeyUp);
        document.addEventListener('wheel', handleWheel, { passive: false });

        return () => {
            document.removeEventListener('keydown', handleKeyDown);
            document.removeEventListener('keyup', handleKeyUp);
            document.removeEventListener('wheel', handleWheel);
        };
    }, [previewType, currentView]);

    // 处理鼠标拖动
    const handleMouseDown = (e: React.MouseEvent) => {
        if (isSpacePressed && previewType === 'mermaid') {
            setIsDragging(true);
            setDragStart({ x: e.clientX - mermaidPosition.x, y: e.clientY - mermaidPosition.y });
        }
    };

    const handleMouseMove = (e: React.MouseEvent) => {
        if (isDragging && isSpacePressed) {
            setMermaidPosition({
                x: e.clientX - dragStart.x,
                y: e.clientY - dragStart.y
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
            await invoke('confirm_environment_install', {
                tool: environmentTool,
                confirmed: true,
                lang: currentLangRef.current,
                inputStr: currentInputStrRef.current
            });
        } catch (error) {
            setLogs(prev => [...prev, { type: 'error', message: `确认安装失败: ${error}` }]);
        }
    };

    // 处理环境安装取消
    const handleEnvironmentInstallCancel = async () => {
        try {
            await invoke('confirm_environment_install', {
                tool: environmentTool,
                confirmed: false,
                lang: currentLangRef.current,
                inputStr: currentInputStrRef.current
            });
            setShowEnvironmentDialog(false);
        } catch (error) {
            setLogs(prev => [...prev, { type: 'error', message: `取消安装失败: ${error}` }]);
        }
    };

    // 当预览准备好时，切换到预览视图
    useEffect(() => {
        if (isPreviewReady && (previewUrl || previewType === 'mermaid' || previewType === 'html' || previewType === 'svg' || previewType === 'xml' || previewType === 'markdown' || previewType === 'md')) {
            setCurrentView('preview');
        }
    }, [isPreviewReady, previewUrl, previewType]);

    // 注册事件监听
    useEffect(() => {
        let isCancelled = false;

        const registerListeners = async () => {
            // 在函数执行一开始就检查并设置标志位，避免竞争条件
            if (isRegisteredRef.current || isCancelled) {
                return;
            }
            isRegisteredRef.current = true;

            const addLog = (type: LogLine['type']) => (event: { payload: any }) => {
                const message = event.payload as string;
                setLogs(prev => [...prev, { type, message }]);
            };

            const handleArtifactData = (event: { payload: any }) => {
                const data = event.payload;
                if (data.original_code && data.type) {
                    switch (data.type) {
                        case 'vue':
                        case 'react':
                            setPreviewType(data.type);
                            break;
                        case 'mermaid':
                            setPreviewType('mermaid');
                            setMermaidContent(data.original_code);
                            setIsPreviewReady(true);
                            break;
                        case 'html':
                            setPreviewType('html');
                            setHtmlContent(data.original_code);
                            setIsPreviewReady(true);
                            break;
                        case 'svg':
                            setPreviewType('svg');
                            setHtmlContent(data.original_code);
                            setIsPreviewReady(true);
                            break;
                        case 'xml':
                            setPreviewType('xml');
                            setHtmlContent(data.original_code);
                            setIsPreviewReady(true);
                            break;
                        case 'markdown':
                        case 'md':
                            setPreviewType(data.type);
                            setMarkdownContent(data.original_code);
                            setIsPreviewReady(true);
                            break;
                        default:
                            break;
                    }
                    setOriginalCode(data.original_code);
                }
            };

            const handleRedirect = (event: { payload: any }) => {
                const url = event.payload as string;
                setPreviewUrl(url);
                setIsPreviewReady(true);
            };

            const handleEnvironmentCheck = (event: { payload: any }) => {
                const data = event.payload;
                setEnvironmentTool(data.tool);
                setEnvironmentMessage(data.message);
                setCurrentLang(data.lang);
                setCurrentInputStr(data.input_str);
                setShowEnvironmentDialog(true);
            };

            const handleEnvironmentInstallStarted = (event: { payload: any }) => {
                const data = event.payload;
                setCurrentLang(data.lang);
                setCurrentInputStr(data.input_str);
                isInstalling.current = true;
                setShowEnvironmentDialog(false);
            };

            const handleBunInstallFinished = (event: { payload: any }) => {
                const success = event.payload as boolean;
                console.log('🔧 [ArtifactPreviewWindow] 收到Bun安装完成事件:', success, isInstalling);
                if (success && isInstalling.current) {
                    setLogs(prev => [...prev, { type: 'success', message: 'Bun 安装成功，正在重新启动预览...' }]);
                    // 重新启动预览
                    invoke('retry_preview_after_install', {
                        lang: currentLangRef.current,
                        inputStr: currentInputStrRef.current
                    }).then(() => {
                        isInstalling.current = false;
                    }).catch(error => {
                        setLogs(prev => [...prev, { type: 'error', message: `重新启动预览失败: ${error}` }]);
                        isInstalling.current = false;
                    });
                } else if (!success) {
                    setLogs(prev => [...prev, { type: 'error', message: 'Bun 安装失败' }]);
                    isInstalling.current = false;
                }
            };

            const handleUvInstallFinished = (event: { payload: any }) => {
                const success = event.payload as boolean;
                if (success && isInstalling.current) {
                    setLogs(prev => [...prev, { type: 'success', message: 'uv 安装成功，正在重新启动预览...' }]);
                    // 重新启动预览
                    invoke('retry_preview_after_install', {
                        lang: currentLangRef.current,
                        inputStr: currentInputStrRef.current
                    }).then(() => {
                        isInstalling.current = false;
                    }).catch(error => {
                        setLogs(prev => [...prev, { type: 'error', message: `重新启动预览失败: ${error}` }]);
                        isInstalling.current = false;
                    });
                } else if (!success) {
                    setLogs(prev => [...prev, { type: 'error', message: 'uv 安装失败' }]);
                    isInstalling.current = false;
                }
            };


            try {
                const unlisteners = await Promise.all([
                    listen('artifact-preview-data', handleArtifactData),
                    listen('artifact-preview-log', addLog('log')),
                    listen('artifact-preview-error', addLog('error')),
                    listen('artifact-preview-success', addLog('success')),
                    listen('artifact-preview-redirect', handleRedirect),
                    listen('environment-check', handleEnvironmentCheck),
                    listen('environment-install-started', handleEnvironmentInstallStarted),
                    listen('bun-install-finished', handleBunInstallFinished),
                    listen('uv-install-finished', handleUvInstallFinished)
                ]);

                console.log("🔧 [ArtifactPreviewWindow] 监听器注册成功");

                // 检查是否已被取消
                if (isCancelled) {
                    unlisteners.forEach((fn) => fn());
                    return;
                }

                unlistenersRef.current = unlisteners;
            } catch (error) {
                isRegisteredRef.current = false;
            }
        };

        registerListeners();

        return () => {
            isCancelled = true;
            unlistenersRef.current.forEach((fn) => fn());
            unlistenersRef.current = [];
            isRegisteredRef.current = false;
        };
    }, []);

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
                if (previewTypeRef.current === 'vue') {
                    await invoke('close_vue_preview', { previewId: 'vue' });
                } else if (previewTypeRef.current === 'mermaid' || previewTypeRef.current === 'html' || previewTypeRef.current === 'svg' || previewTypeRef.current === 'xml' || previewTypeRef.current === 'markdown' || previewTypeRef.current === 'md') {
                    // Mermaid/HTML/SVG/XML/Markdown 不需要服务器清理，只需要清除DOM
                } else {
                    await invoke('close_react_preview', { previewId: 'react' });
                }

                setLogs([]);
                setPreviewUrl(null);
                setIsPreviewReady(false);
                setCurrentView('logs');
                setPreviewType(null);
                setMermaidContent('');
                setHtmlContent('');
                setMarkdownContent('');

            } catch (error) {
            }
        };

        // 监听窗口关闭事件 - Tauri v2 的正确用法
        const setupCloseListener = async () => {
            try {
                unlistenCloseRequested = await currentWindow.onCloseRequested(cleanup);
            } catch (error) {
            }
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
    const handleToggleView = () => {
        setCurrentView(current => current === 'logs' ? 'preview' : 'logs');
    };

    // 在浏览器中打开预览页面
    const handleOpenInBrowser = async () => {
        if (previewUrl) {
            try {
                await open(previewUrl);
            } catch (error) {
            }
        }
    };

    // 刷新iframe
    const handleRefresh = () => {
        if (previewUrl) {
            // 移除现有的_refresh参数，然后添加新的时间戳
            const url = new URL(previewUrl);
            url.searchParams.set('_refresh', Date.now().toString());
            setPreviewUrl(url.toString());
        }
    };

    // 保存当前 artifact 到合集
    const handleSaveArtifact = () => {
        const currentType = previewTypeRef.current;
        if (currentType && (currentType === 'vue' || currentType === 'react' || currentType === 'html')) {
            setShowSaveDialog(true);
        }
    };

    // 检查是否可以保存（仅支持 vue, react, html）
    const canSave = previewTypeRef.current && ['vue', 'react', 'html'].includes(previewTypeRef.current);

    return (
        <div className="flex h-screen bg-background">
            <div className="flex flex-col flex-1 bg-background rounded-xl m-2 shadow-lg border border-border">
                {/* 顶部工具栏 */}
                {isPreviewReady && (previewUrl || previewType === 'mermaid' || previewType === 'html' || previewType === 'svg' || previewType === 'xml' || previewType === 'markdown' || previewType === 'md') && (
                    <div className="flex-shrink-0 p-4 border-b border-border flex items-center justify-between">
                        <div className="text-sm text-muted-foreground">
                            {currentView === 'logs' ? '日志视图' :
                                previewType === 'mermaid' ? 'Mermaid 图表预览' :
                                    previewType === 'html' ? 'HTML 预览' :
                                        previewType === 'svg' ? 'SVG 预览' :
                                            previewType === 'xml' ? 'XML 预览' :
                                                previewType === 'markdown' || previewType === 'md' ? 'Markdown 预览' :
                                                    `预览地址: ${previewUrl}`}
                        </div>
                        <div className="flex gap-2">
                            {/* 保存按钮 - 仅在预览模式且可保存时显示 */}
                            {currentView === 'preview' && canSave && (
                                <Button
                                    onClick={handleSaveArtifact}
                                    variant="default"
                                    size="sm"
                                    title="保存到合集"
                                >
                                    保存
                                </Button>
                            )}
                            {previewType !== 'mermaid' && previewType !== 'html' && previewType !== 'svg' && previewType !== 'xml' && previewType !== 'markdown' && previewType !== 'md' && (
                                <>
                                    <Button
                                        onClick={handleRefresh}
                                        variant="outline"
                                        size="sm"
                                        title="刷新预览"
                                    >
                                        刷新
                                    </Button>
                                    <Button
                                        onClick={handleOpenInBrowser}
                                        variant="outline"
                                        size="sm"
                                        title="在浏览器中打开"
                                    >
                                        打开浏览器
                                    </Button>
                                </>
                            )}
                            <Button
                                onClick={handleToggleView}
                                variant="default"
                                size="sm"
                            >
                                {currentView === 'logs' ? '查看预览' : '查看日志'}
                            </Button>
                        </div>
                    </div>
                )}

                {/* 主要内容区域 */}
                <div className="flex-1 flex flex-col">
                    {currentView === 'logs' ? (
                        /* 日志视图 - 全屏显示 */
                        <div className="flex-1 flex flex-col p-4">
                            <h2 className="text-lg font-semibold mb-2 text-foreground">Artifact Preview Logs</h2>
                            <div className="flex-1 overflow-y-auto rounded border border-border p-2 bg-muted text-sm font-mono">
                                {logs.map((log, idx) => (
                                    <div
                                        key={idx}
                                        className={
                                            log.type === 'error'
                                                ? 'text-destructive'
                                                : log.type === 'success'
                                                    ? 'text-green-600 dark:text-green-400'
                                                    : 'text-foreground'
                                        }
                                    >
                                        {log.message}
                                    </div>
                                ))}
                                <div ref={logsEndRef} />
                            </div>

                            {/* 如果预览准备好了，显示提示 */}
                            {isPreviewReady && (previewUrl || previewType === 'mermaid' || previewType === 'html' || previewType === 'svg' || previewType === 'xml' || previewType === 'markdown' || previewType === 'md') && (
                                <div className="mt-4 p-3 bg-green-50 dark:bg-green-950 border border-green-200 dark:border-green-800 rounded">
                                    <p className="text-green-700 dark:text-green-400 text-sm">
                                        ✅ 预览准备完成，即将自动切换到预览视图...
                                    </p>
                                </div>
                            )}
                        </div>
                    ) : (
                        /* 预览视图 - 根据类型显示不同内容 */
                        <div className="flex-1 flex flex-col">
                            {previewType === 'mermaid' ? (
                                /* Mermaid 图表预览 */
                                <div className="flex-1 flex flex-col p-4">
                                    <div className="flex justify-between items-center mb-2">
                                        <div className="text-sm text-muted-foreground">
                                            缩放: {Math.round(mermaidScale * 100)}% | 提示: 滚轮缩放，空格键+拖动
                                        </div>
                                        <Button
                                            onClick={resetMermaidView}
                                            variant="ghost"
                                            size="sm"
                                        >
                                            重置视图
                                        </Button>
                                    </div>
                                    <div
                                        ref={mermaidContainerRef}
                                        className={`flex-1 bg-background border border-border rounded-lg shadow-sm overflow-hidden relative ${isSpacePressed ? 'cursor-grab' : 'cursor-default'
                                            } ${isDragging ? 'cursor-grabbing' : ''}`}
                                        onMouseDown={handleMouseDown}
                                        onMouseMove={handleMouseMove}
                                        onMouseUp={handleMouseUp}
                                        onMouseLeave={handleMouseUp}
                                        style={{
                                            minHeight: '400px',
                                            maxHeight: 'calc(100vh - 200px)',
                                            overflow: 'auto'
                                        }}
                                    >
                                        <div
                                            style={{
                                                transform: `scale(${mermaidScale}) translate(${mermaidPosition.x}px, ${mermaidPosition.y}px)`,
                                                transformOrigin: 'center center',
                                                transition: isDragging ? 'none' : 'transform 0.1s ease-out',
                                                display: 'flex',
                                                justifyContent: 'center',
                                                alignItems: 'center',
                                                minWidth: '100%',
                                                minHeight: '100%',
                                                padding: '20px'
                                            }}
                                        >
                                            {/* Mermaid SVG 将被渲染在这里 */}
                                        </div>
                                    </div>
                                </div>
                            ) : previewType === 'markdown' || previewType === 'md' ? (
                                /* Markdown 预览 */
                                <div className="flex-1 overflow-auto bg-background p-6">
                                    <div className="prose prose-lg max-w-none dark:prose-invert">
                                        {(() => {
                                            const mdComponents: any = {
                                                // 仅保留自定义组件，避免覆盖 Streamdown 的 code 渲染
                                                tipscomponent: TipsComponent,
                                            };
                                            return (
                                                <Streamdown
                                                    remarkPlugins={[remarkMath, remarkBreaks, remarkCustomCompenent]}
                                                    // 使用 Streamdown 默认 rehype（包含 harden/raw/katex），避免 sanitize 或自定义覆盖破坏高亮
                                                    components={mdComponents}
                                                >
                                                    {markdownContent}
                                                </Streamdown>
                                            );
                                        })()}
                                    </div>
                                </div>
                            ) : previewType === 'html' || previewType === 'svg' || previewType === 'xml' ? (
                                /* HTML/SVG/XML 预览 */
                                <iframe
                                    srcDoc={htmlContent}
                                    className="flex-1 w-full border-0 bg-background"
                                    sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                                    style={{
                                        minHeight: '400px'
                                    }}
                                />
                            ) : (
                                /* iframe 预览 - 用于 React 和 Vue */
                                <iframe
                                    src={previewUrl || ''}
                                    className="flex-1 w-full border-0"
                                    sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                                    onLoad={() => {
                                    }}
                                    onError={() => {
                                    }}
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

            {/* Artifact 保存对话框 */}
            {previewTypeRef.current && (
                <SaveArtifactDialog
                    isOpen={showSaveDialog}
                    onClose={() => setShowSaveDialog(false)}
                    artifactType={previewTypeRef.current}
                    code={originalCode || htmlContent || mermaidContent || markdownContent}
                />
            )}
        </div>
    );
} 
