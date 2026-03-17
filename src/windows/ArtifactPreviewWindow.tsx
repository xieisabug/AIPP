import { useEffect, useRef, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { openUrl } from '@tauri-apps/plugin-opener';
import { save } from '@tauri-apps/plugin-dialog';
import { writeFile } from '@tauri-apps/plugin-fs';
import mermaid from 'mermaid';
import ReactMarkdown from 'react-markdown';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';
import remarkMath from 'remark-math';
import remarkBreaks from 'remark-breaks';
import remarkCustomCompenent from '@/react-markdown/remarkCustomComponent';
import remarkCodeBlockMeta from '@/react-markdown/remarkCodeBlockMeta';
import rehypeKatex from 'rehype-katex';
import rehypeRaw from 'rehype-raw';
import TipsComponent from '@/react-markdown/components/TipsComponent';
import { resolveCodeBlockMeta } from '@/react-markdown/remarkCodeBlockMeta';
import '../styles/ArtifactPreviewWIndow.css';
import 'katex/dist/katex.min.css';
import EnvironmentInstallDialog from '../components/EnvironmentInstallDialog';
import SaveArtifactDialog from '../components/SaveArtifactDialog';
import { useTheme } from '../hooks/useTheme';
import { useArtifactEvents, ArtifactData, EnvironmentCheckData } from '../hooks/useArtifactEvents';
import { useArtifactBridge } from '../hooks/useArtifactBridge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Bot, Database, Settings2, Loader2 } from 'lucide-react';
import { AssistantBasicInfo } from '@/data/ArtifactCollection';
import ArtifactPreviewCodeBlock from '@/components/ArtifactPreviewCodeBlock';
import {
    generateRandomDbId,
    loadArtifactRuntimeConfig,
    normalizeDbId,
    persistArtifactRuntimeConfig,
} from '@/utils/artifactConfig';

// localStorage 键名：用于缓存当前 artifact 信息，实现刷新后恢复
const ARTIFACT_CACHE_KEY = 'artifact_preview_cache';

/**
 * 仅用于 "artifact_preview" 窗口。
 * - 监听后端发出的 artifact-log / artifact-error / artifact-success 事件并展示。
 * - 使用 iframe 沙盒展示预览内容，避免页面跳转导致监听器失效。
 * - 显示模式：先显示日志，预览准备好后切换到全屏预览
 */
export default function ArtifactPreviewWindow() {
    // 集成主题系统
    useTheme("artifact_preview");

    const [previewUrl, setPreviewUrl] = useState<string | null>(null);
    const [isPreviewReady, setIsPreviewReady] = useState(false);
    const [currentView, setCurrentView] = useState<'logs' | 'preview' | 'code'>('logs');
    const [previewType, setPreviewType] = useState<'react' | 'vue' | 'mermaid' | 'html' | 'svg' | 'xml' | 'markdown' | 'md' | 'drawio' | null>(null);
    const logsEndRef = useRef<HTMLDivElement | null>(null);
    const previewTypeRef = useRef<'react' | 'vue' | 'mermaid' | 'html' | 'svg' | 'xml' | 'markdown' | 'md' | 'drawio' | null>(null);
    const mermaidContainerRef = useRef<HTMLDivElement | null>(null);
    const [mermaidContent, setMermaidContent] = useState<string>('');
    const [htmlContent, setHtmlContent] = useState<string>('');
    const [markdownContent, setMarkdownContent] = useState<string>('');
    const [drawioXmlContent, setDrawioXmlContent] = useState<string>('');
    const [mermaidScale, setMermaidScale] = useState<number>(1);
    const [mermaidPosition, setMermaidPosition] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
    const [isDragging, setIsDragging] = useState<boolean>(false);
    const [dragStart, setDragStart] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
    const [isSpacePressed, setIsSpacePressed] = useState<boolean>(false);
    const isInstalling = useRef<boolean>(false);
    const drawioIframeRef = useRef<HTMLIFrameElement>(null);
    const previewIframeRef = useRef<HTMLIFrameElement>(null);

    // 环境安装相关状态
    const [showEnvironmentDialog, setShowEnvironmentDialog] = useState<boolean>(false);
    const [environmentTool, setEnvironmentTool] = useState<string>('');
    const [environmentMessage, setEnvironmentMessage] = useState<string>('');
    const [currentLang, setCurrentLang] = useState<string>('');
    const [currentInputStr, setCurrentInputStr] = useState<string>('');

    // 保存 artifact 相关状态
    const [showSaveDialog, setShowSaveDialog] = useState<boolean>(false);
    const [originalCode, setOriginalCode] = useState<string>(''); // 存储原始代码

    // Artifact Bridge 配置
    const [bridgeConfig, setBridgeConfig] = useState<{ db_id?: string; assistant_id?: number }>({});
    // Track current conversation for session-level config
    const [currentConversationId, setCurrentConversationId] = useState<number | undefined>(undefined);
    const [runtimeConfig, setRuntimeConfig] = useState<{ db_id?: string; assistant_id?: number }>({});
    const [assistants, setAssistants] = useState<AssistantBasicInfo[]>([]);

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

    // ====== Artifact Bridge ======
    // 集成 postMessage 桥接，允许 artifact 访问数据库和 AI 助手
    const { sendConfig } = useArtifactBridge({
        iframeRef: previewIframeRef,
        config: bridgeConfig,
        allowedOrigins: ['http://localhost', 'http://127.0.0.1'],
    });

    useEffect(() => {
        invoke<AssistantBasicInfo[]>('artifact_get_assistants')
            .then(setAssistants)
            .catch(() => {
                setAssistants([]);
            });
    }, []);

    // ====== 监听 artifact 更新事件，实现实时刷新 ======
    useEffect(() => {
        if (currentConversationId === undefined) return;

        const unlistenPromise = listen<{ conversation_id: number; artifact: { artifact_key: string } }>(
            'artifact-manifest-updated',
            (event) => {
                // 只处理当前对话的 artifact 更新
                if (event.payload.conversation_id !== currentConversationId) return;

                console.log('🔧 [ArtifactPreviewWindow] 检测到 artifact 更新，自动刷新预览:', event.payload.artifact.artifact_key);

                // 调用后端恢复命令，重新加载缓存的 artifact
                invoke<string | null>('restore_artifact_preview', { sourceWindow: 'artifact_preview' })
                    .then((result) => {
                        if (result) {
                            console.log('🔧 [ArtifactPreviewWindow] Artifact 预览已刷新');
                        }
                    })
                    .catch((error) => {
                        console.error('[ArtifactPreviewWindow] 刷新 artifact 预览失败:', error);
                    });
            }
        );

        return () => {
            unlistenPromise.then((unlisten) => unlisten());
        };
    }, [currentConversationId]);

    // ====== 缓存相关函数 ======

    // 保存当前 artifact 到 localStorage 缓存
    const saveArtifactToCache = useCallback((type: string, code: string) => {
        try {
            const cache = {
                type,
                code,
                timestamp: Date.now(),
            };
            localStorage.setItem(ARTIFACT_CACHE_KEY, JSON.stringify(cache));
            console.log('🔧 [ArtifactPreviewWindow] 已缓存 artifact:', type);
        } catch (e) {
            console.warn('缓存 artifact 失败:', e);
        }
    }, []);

    // 从缓存加载 artifact（刷新恢复）
    const loadArtifactFromCache = useCallback(async () => {
        try {
            const cached = localStorage.getItem(ARTIFACT_CACHE_KEY);
            if (!cached) return false;

            const cache = JSON.parse(cached);

            // 检查缓存是否过期（24小时）
            const CACHE_EXPIRY = 24 * 60 * 60 * 1000;
            if (Date.now() - cache.timestamp > CACHE_EXPIRY) {
                localStorage.removeItem(ARTIFACT_CACHE_KEY);
                return false;
            }

            console.log('🔧 [ArtifactPreviewWindow] 从缓存恢复 artifact:', cache.type);

            // 调用后端恢复命令
            const result = await invoke<string | null>('restore_artifact_preview', {
                sourceWindow: 'artifact_preview',
            });
            return result !== null;
        } catch (e) {
            console.warn('从缓存恢复 artifact 失败:', e);
            return false;
        }
    }, []);

    // 标记是否已尝试从缓存恢复，防止无限循环
    const hasTriedRestoreRef = useRef(false);
    const restoreAbortRef = useRef(false);

    // ====== 重置函数 ======

    // 完整的状态重置函数 - 在切换 artifact 时调用
    const resetPreviewState = useCallback(async () => {
        console.log('🔧 [ArtifactPreviewWindow] 重置预览状态');

        // 设置恢复标记，防止切换 artifact 时触发缓存恢复
        hasTriedRestoreRef.current = true;
        restoreAbortRef.current = true;

        // 1. 清理旧的预览服务器
        const currentType = previewTypeRef.current;
        if (currentType === 'vue') {
            try {
                await invoke('close_vue_preview', { previewId: 'vue' });
                console.log('🔧 已关闭 Vue 预览服务器');
            } catch (e) {
                console.warn('关闭 Vue 预览失败:', e);
            }
        } else if (currentType === 'react') {
            try {
                await invoke('close_react_preview', { previewId: 'react' });
                console.log('🔧 已关闭 React 预览服务器');
            } catch (e) {
                console.warn('关闭 React 预览失败:', e);
            }
        }

        // 2. 清除所有内容状态
        setPreviewUrl(null);
        setPreviewType(null);
        setMermaidContent('');
        setHtmlContent('');
        setMarkdownContent('');
        setDrawioXmlContent('');
        setOriginalCode('');
        setIsPreviewReady(false);

        // 3. 切换到日志视图（显示加载状态）
        setCurrentView('logs');

        console.log('🔧 [ArtifactPreviewWindow] 状态重置完成');
    }, []);

    // ====== 事件处理函数 ======

    // 处理 artifact 数据
    const handleArtifactData = useCallback((data: ArtifactData) => {
        if (data.original_code && data.type) {
            restoreAbortRef.current = true;
            // 保存到缓存，用于刷新恢复
            saveArtifactToCache(data.type, data.original_code);

            // Update conversation ID and load session-level config
            const convId = typeof data.conversation_id === 'number' ? data.conversation_id : undefined;
            setCurrentConversationId(convId);

            // Load config for this conversation (or global if no conversation)
            const sessionConfig = loadArtifactRuntimeConfig(convId);

            // If data has explicit overrides, use them; otherwise use session config
            const hasRuntimeOverride = Boolean(data.db_id) || typeof data.assistant_id === 'number';
            const nextRuntime = hasRuntimeOverride ? {
                db_id: data.db_id || sessionConfig.db_id,
                assistant_id: typeof data.assistant_id === 'number' ? data.assistant_id : sessionConfig.assistant_id,
            } : sessionConfig;

            setRuntimeConfig(nextRuntime);
            if (hasRuntimeOverride) {
                persistArtifactRuntimeConfig(nextRuntime, convId);
            }
            setBridgeConfig({
                db_id: nextRuntime.db_id,
                assistant_id: nextRuntime.assistant_id,
            });

            switch (data.type) {
                case 'vue':
                case 'react':
                    setPreviewType(data.type as 'vue' | 'react');
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
                case 'drawio':
                    setPreviewType('drawio');
                    setDrawioXmlContent(data.original_code);
                    setIsPreviewReady(true);
                    break;
                case 'markdown':
                case 'md':
                    setPreviewType(data.type as 'markdown' | 'md');
                    setMarkdownContent(data.original_code);
                    setIsPreviewReady(true);
                    break;
                default:
                    break;
            }
            setOriginalCode(data.original_code);
        }
    }, [saveArtifactToCache]);

    // 处理重定向
    const handleRedirect = useCallback((url: string) => {
        restoreAbortRef.current = true;
        setPreviewUrl(url);
        setIsPreviewReady(true);
    }, []);

    // 处理环境检查
    const handleEnvironmentCheck = useCallback((data: EnvironmentCheckData) => {
        restoreAbortRef.current = true;
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
        console.log('🔧 [ArtifactPreviewWindow] 收到Bun安装完成事件:', success, isInstalling.current);
        if (success && isInstalling.current) {
            artifactEvents.addLog('success', 'Bun 安装成功，正在重新启动预览...');
            invoke('retry_preview_after_install', {
                lang: currentLangRef.current,
                inputStr: currentInputStrRef.current
            }).then(() => {
                isInstalling.current = false;
            }).catch(error => {
                artifactEvents.addLog('error', `重新启动预览失败: ${error}`);
                isInstalling.current = false;
            });
        } else if (!success) {
            artifactEvents.addLog('error', 'Bun 安装失败');
            isInstalling.current = false;
        }
    }, []);

    // 处理 uv 安装完成
    const handleUvInstallFinished = useCallback((success: boolean) => {
        if (success && isInstalling.current) {
            artifactEvents.addLog('success', 'uv 安装成功，正在重新启动预览...');
            invoke('retry_preview_after_install', {
                lang: currentLangRef.current,
                inputStr: currentInputStrRef.current
            }).then(() => {
                isInstalling.current = false;
            }).catch(error => {
                artifactEvents.addLog('error', `重新启动预览失败: ${error}`);
                isInstalling.current = false;
            });
        } else if (!success) {
            artifactEvents.addLog('error', 'uv 安装失败');
            isInstalling.current = false;
        }
    }, []);

    // 使用统一的事件处理 hook
    const artifactEvents = useArtifactEvents({
        windowType: 'preview',
        readyWindow: 'artifact_preview',
        onArtifactData: handleArtifactData,
        onRedirect: handleRedirect,
        onEnvironmentCheck: handleEnvironmentCheck,
        onEnvironmentInstallStarted: handleEnvironmentInstallStarted,
        onBunInstallFinished: handleBunInstallFinished,
        onUvInstallFinished: handleUvInstallFinished,
        onReset: resetPreviewState,
    });

    useEffect(() => {
        setBridgeConfig({
            db_id: runtimeConfig.db_id,
            assistant_id: runtimeConfig.assistant_id,
        });
    }, [runtimeConfig]);

    useEffect(() => {
        sendConfig();
    }, [sendConfig, runtimeConfig.db_id, runtimeConfig.assistant_id]);

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

    // 组件初始化时尝试从缓存恢复（用于刷新后恢复预览）
    // 使用 ref 防止重复执行，避免无限循环
    useEffect(() => {
        const initFromCache = async () => {
            // 只在首次加载且没有任何数据时尝试恢复
            const hasData = previewUrl || previewType || mermaidContent || htmlContent || markdownContent || drawioXmlContent;
            if (!hasData && !artifactEvents.hasReceivedData && !hasTriedRestoreRef.current) {
                hasTriedRestoreRef.current = true;  // 标记已尝试
                await new Promise(resolve => setTimeout(resolve, 300));
                if (!restoreAbortRef.current) {
                    const restored = await loadArtifactFromCache();
                    if (restored) {
                        artifactEvents.addLog('log', '正在恢复上次的预览...');
                    }
                }
            }
        };

        initFromCache();
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [loadArtifactFromCache]);

    // 自动滚动到底部
    useEffect(() => {
        logsEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [artifactEvents.logs]);

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
            artifactEvents.addLog('error', `确认安装失败: ${error}`);
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
            artifactEvents.addLog('error', `取消安装失败: ${error}`);
        }
    };

    // 当预览准备好时，切换到预览视图
    useEffect(() => {
        if (isPreviewReady && (previewUrl || previewType === 'mermaid' || previewType === 'html' || previewType === 'svg' || previewType === 'xml' || previewType === 'markdown' || previewType === 'md' || previewType === 'drawio')) {
            setCurrentView('preview');
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
                if (previewTypeRef.current === 'vue') {
                    await invoke('close_vue_preview', { previewId: 'vue' });
                } else if (previewTypeRef.current === 'mermaid' || previewTypeRef.current === 'html' || previewTypeRef.current === 'svg' || previewTypeRef.current === 'xml' || previewTypeRef.current === 'markdown' || previewTypeRef.current === 'md' || previewTypeRef.current === 'drawio') {
                    // Mermaid/HTML/SVG/XML/Markdown/Draw.io 不需要服务器清理，只需要清除DOM
                } else {
                    await invoke('close_react_preview', { previewId: 'react' });
                }

                artifactEvents.clearLogs();
                setPreviewUrl(null);
                setIsPreviewReady(false);
                setCurrentView('logs');
                setPreviewType(null);
                setMermaidContent('');
                setHtmlContent('');
                setMarkdownContent('');
                setDrawioXmlContent('');

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
    // 在浏览器中打开预览页面
    const handleOpenInBrowser = async () => {
        if (previewUrl) {
            try {
                await openUrl(previewUrl);
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

    const postDrawioLoad = useCallback((xml: string) => {
        drawioIframeRef.current?.contentWindow?.postMessage(
            JSON.stringify({
                action: 'load',
                xml,
                autosave: 1,
            }),
            '*'
        );
    }, []);

    const handleDrawioExport = useCallback(async (xmlOverride?: string) => {
        if (previewTypeRef.current !== 'drawio') return;
        const xml = (xmlOverride ?? drawioXmlContent).trim();
        if (!xml) {
            artifactEvents.addLog('error', '保存 Draw.io 文件失败：当前没有可保存的图表内容');
            return;
        }

        try {
            const filePath = await save({
                defaultPath: 'diagram.drawio',
                filters: [{ name: 'Draw.io', extensions: ['drawio', 'xml'] }],
            });
            if (!filePath || Array.isArray(filePath)) {
                return;
            }

            await writeFile(filePath, new TextEncoder().encode(xml));
            artifactEvents.addLog('success', `Draw.io 文件已保存到: ${filePath}`);
        } catch (error) {
            artifactEvents.addLog('error', `保存 Draw.io 文件失败: ${error}`);
        }
    }, [artifactEvents, drawioXmlContent]);

    // 检查是否可以保存（仅支持 vue, react, html）
    const canSave = previewTypeRef.current && ['vue', 'react', 'html'].includes(previewTypeRef.current);

    const handleRuntimeDbChange = (value: string) => {
        const normalized = normalizeDbId(value);
        setRuntimeConfig(prev => {
            const next = { ...prev, db_id: normalized || undefined };
            persistArtifactRuntimeConfig(next, currentConversationId);
            return next;
        });
    };

    const handleAssistantChange = (value: string) => {
        const assistantId = value && value !== 'none' ? parseInt(value, 10) : undefined;
        setRuntimeConfig(prev => {
            const next = { ...prev, assistant_id: assistantId };
            persistArtifactRuntimeConfig(next, currentConversationId);
            return next;
        });
    };

    const handleGenerateDbId = () => {
        const dbId = generateRandomDbId();
        setRuntimeConfig(prev => {
            const next = { ...prev, db_id: dbId };
            persistArtifactRuntimeConfig(next, currentConversationId);
            return next;
        });
    };

    // draw.io postMessage 通信
    useEffect(() => {
        if (previewType === 'drawio' && drawioXmlContent) {
            console.log('[Draw.io] drawioXmlContent 已就绪:', drawioXmlContent.substring(0, 100));
            let loaded = false;

            const handleMessage = (evt: MessageEvent) => {
                // 忽略非字符串消息
                if (typeof evt.data !== 'string' || evt.data.length === 0) {
                    return;
                }

                try {
                    const msg = JSON.parse(evt.data);
                    console.log('[Draw.io] 收到消息:', msg);

                    // configure 事件 - 需要回复配置
                    if (msg.event === 'configure') {
                        drawioIframeRef.current?.contentWindow?.postMessage(
                            JSON.stringify({
                                action: 'configure',
                                config: {
                                    defaultFonts: ['Humor Sans', 'Microsoft YaHei', 'SimHei'],
                                }
                            }),
                            '*'
                        );
                        artifactEvents.addLog('log', 'Draw.io 配置已发送');
                    }

                    // init 事件 - draw.io 准备就绪，发送 XML 数据
                    else if (msg.event === 'init' && !loaded) {
                        loaded = true;
                        console.log('[Draw.io] 发送 XML 数据...');
                        postDrawioLoad(drawioXmlContent);
                        artifactEvents.addLog('success', 'Draw.io 图表已加载');
                    }

                    // load 事件 - 图表加载完成
                    else if (msg.event === 'load') {
                        console.log('[Draw.io] 图表加载完成，尺寸:', msg.bounds);
                    }

                    else if (msg.event === 'autosave' && typeof msg.xml === 'string') {
                        setDrawioXmlContent(msg.xml);
                        setOriginalCode(msg.xml);
                    }

                    // save 事件 - 用户保存时
                    else if (msg.event === 'save' && typeof msg.xml === 'string') {
                        console.log('[Draw.io] 用户保存，XML 长度:', msg.xml.length);
                        setDrawioXmlContent(msg.xml);
                        setOriginalCode(msg.xml);
                        void handleDrawioExport(msg.xml);
                    }

                    // exit 事件 - 用户退出
                    else if (msg.event === 'exit') {
                        console.log('[Draw.io] 用户退出，是否修改:', msg.modified);
                    }
                } catch (e) {
                    // 忽略无法解析的消息（可能来自其他来源）
                    console.debug('[Draw.io] 忽略非 JSON 消息:', evt.data.substring(0, 50));
                }
            };

            window.addEventListener('message', handleMessage);
            return () => {
                console.log('[Draw.io] 移除消息监听');
                window.removeEventListener('message', handleMessage);
            };
        }
    }, [previewType, drawioXmlContent, artifactEvents, handleDrawioExport, postDrawioLoad]);

    const codeContent = originalCode || htmlContent || mermaidContent || markdownContent || '';
    const codeLanguage = previewType === 'react'
        ? 'tsx'
        : previewType === 'vue'
            ? 'vue'
            : previewType === 'mermaid'
                ? 'mermaid'
                : previewType === 'html'
                    ? 'html'
                    : previewType === 'svg' || previewType === 'xml' || previewType === 'drawio'
                        ? 'xml'
                        : previewType === 'markdown' || previewType === 'md'
                            ? 'markdown'
                            : 'text';

    return (
        <div className="flex h-screen bg-background overflow-hidden" data-aipp-window="artifact_preview" data-aipp-slot="window-root">
            <div
                className="flex flex-col flex-1 bg-background rounded-xl m-2 shadow-lg border border-border min-h-0 overflow-hidden"
                data-aipp-slot="artifact-preview-main"
            >
                {/* 顶部工具栏 */}
                {isPreviewReady && (previewUrl || previewType === 'mermaid' || previewType === 'html' || previewType === 'svg' || previewType === 'xml' || previewType === 'markdown' || previewType === 'md' || previewType === 'drawio') && (
                    <div className="flex-shrink-0 p-4 border-b border-border flex items-center justify-between" data-aipp-slot="artifact-preview-toolbar">
                        <div className="text-sm text-muted-foreground">
                            {currentView === 'logs' ? '日志视图' : currentView === 'code' ? '代码视图' :
                                previewType === 'mermaid' ? 'Mermaid 图表预览' :
                                    previewType === 'html' ? 'HTML 预览' :
                                        previewType === 'svg' ? 'SVG 预览' :
                                            previewType === 'xml' ? 'XML 预览' :
                                                previewType === 'markdown' || previewType === 'md' ? 'Markdown 预览' :
                                                    previewType === 'drawio' ? 'Draw.io 图表预览' :
                                                        `预览地址: ${previewUrl}`}
                        </div>
                        <div className="flex items-center gap-2">
                            <Popover>
                                <PopoverTrigger asChild>
                                    <Button variant="outline" size="sm" title="临时配置">
                                        <Settings2 className="h-4 w-4" />
                                    </Button>
                                </PopoverTrigger>
                                <PopoverContent align="end" className="w-80">
                                    <div className="space-y-4">
                                        <div className="text-sm font-medium text-foreground">临时配置（未保存也可用）</div>
                                        <div className="space-y-2">
                                            <Label className="flex items-center gap-2 text-sm">
                                                <Database className="h-4 w-4" />
                                                数据库标识
                                            </Label>
                                            <div className="flex gap-2">
                                                <Input
                                                    value={runtimeConfig.db_id || ''}
                                                    onChange={(event) => handleRuntimeDbChange(event.target.value)}
                                                    placeholder="artifact-xxxx"
                                                />
                                                <Button variant="outline" size="sm" onClick={handleGenerateDbId}>
                                                    随机
                                                </Button>
                                            </div>
                                            <p className="text-xs text-muted-foreground">仅支持字母、数字、下划线和连字符</p>
                                        </div>
                                        <div className="space-y-2">
                                            <Label className="flex items-center gap-2 text-sm">
                                                <Bot className="h-4 w-4" />
                                                关联助手
                                            </Label>
                                            <Select
                                                value={runtimeConfig.assistant_id ? String(runtimeConfig.assistant_id) : 'none'}
                                                onValueChange={handleAssistantChange}
                                            >
                                                <SelectTrigger className="w-full">
                                                    <SelectValue placeholder="选择一个助手（可选）" />
                                                </SelectTrigger>
                                                <SelectContent>
                                                    <SelectItem value="none">不关联助手</SelectItem>
                                                    {assistants.map((assistant) => (
                                                        <SelectItem key={assistant.id} value={String(assistant.id)}>
                                                            <span className="flex items-center gap-2">
                                                                <span>{assistant.icon}</span>
                                                                <span>{assistant.name}</span>
                                                            </span>
                                                        </SelectItem>
                                                    ))}
                                                </SelectContent>
                                            </Select>
                                        </div>
                                    </div>
                                </PopoverContent>
                            </Popover>
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
                            {currentView === 'preview' && previewType === 'drawio' && (
                                <>
                                    <Button
                                        onClick={() => {
                                            void handleDrawioExport();
                                        }}
                                        variant="outline"
                                        size="sm"
                                        title="保存 Draw.io 文件"
                                    >
                                        保存文件
                                    </Button>
                                </>
                            )}
                            {previewType !== 'mermaid' && previewType !== 'html' && previewType !== 'svg' && previewType !== 'xml' && previewType !== 'markdown' && previewType !== 'md' && previewType !== 'drawio' && (
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
                            <Tabs value={currentView} onValueChange={(value) => setCurrentView(value as 'logs' | 'preview' | 'code')}>
                                <TabsList>
                                    <TabsTrigger value="logs">日志</TabsTrigger>
                                    <TabsTrigger value="code">代码</TabsTrigger>
                                    <TabsTrigger value="preview">预览</TabsTrigger>
                                </TabsList>
                            </Tabs>
                        </div>
                    </div>
                )}

                {/* 主要内容区域 */}
                <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
                    {currentView === 'logs' ? (
                        /* 日志视图 - 全屏显示 */
                        <div className="flex-1 flex flex-col p-4">
                            <h2 className="text-lg font-semibold mb-2 text-foreground">Artifact Preview Logs</h2>
                            <div className="flex-1 overflow-y-auto rounded border border-border p-2 bg-muted text-sm font-mono">
                                {artifactEvents.logs.map((log, idx) => (
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
                    ) : currentView === 'code' ? (
                        <div className="flex-1 flex flex-col p-4 gap-3 overflow-hidden min-h-0">
                            <div className="text-sm text-muted-foreground flex-shrink-0">Artifact 代码</div>
                            <div className="flex-1 overflow-auto rounded border border-border bg-muted p-3 min-h-0">
                                <div className="w-full max-w-[1100px] [&_pre]:!overflow-visible [&_pre]:!m-0 [&_pre]:!p-0 [&_pre]:!bg-transparent">
                                    <ArtifactPreviewCodeBlock language={codeLanguage} className="bg-transparent">
                                        {codeContent}
                                    </ArtifactPreviewCodeBlock>
                                </div>
                            </div>
                        </div>
                    ) : (
                        /* 预览视图 - 根据类型显示不同内容 */
                        <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
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
                                                tipscomponent: TipsComponent,
                                                code({ className, children, node, ...props }: any) {
                                                    const match = /language-(\w+)/.exec(className || '');
                                                    const meta = resolveCodeBlockMeta(props as Record<string, unknown>, node);
                                                    const dataLanguage = typeof (props as Record<string, unknown>)['data-language'] === 'string'
                                                        ? (props as Record<string, unknown>)['data-language'] as string
                                                        : undefined;
                                                    const language = match?.[1] ?? dataLanguage ?? 'text';
                                                    const isInline = !match && !meta && !dataLanguage;
                                                    const metaLabel = meta
                                                        ? [meta.title || meta.filename, meta.line ? `line ${meta.line}` : null, meta.highlight ? `highlight ${meta.highlight}` : null]
                                                            .filter(Boolean)
                                                            .join(' · ')
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
                                                                {String(children).replace(/\n$/, '')}
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
                                                >
                                                    {markdownContent}
                                                </ReactMarkdown>
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
                            ) : previewType === 'drawio' ? (
                                /* Draw.io 图表预览 */
                                <iframe
                                    ref={drawioIframeRef}
                                    src="https://embed.diagrams.net/?embed=1&ui=min&spin=1&proto=json&noSaveBtn=1&noExitBtn=1"
                                    className="flex-1 w-full border-0 bg-background"
                                    sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox allow-downloads"
                                    style={{
                                        minHeight: '400px'
                                    }}
                                    onLoad={() => {
                                        console.log('[Draw.io] iframe 加载完成');
                                    }}
                                />
                            ) : (previewType === 'react' || previewType === 'vue') && !previewUrl ? (
                                /* 等待 React/Vue 预览服务器启动 */
                                <div className="flex-1 flex items-center justify-center text-muted-foreground">
                                    <div className="text-center">
                                        <Loader2 className="h-8 w-8 mx-auto mb-2 animate-spin opacity-60" />
                                        <p className="text-sm">正在启动预览服务器...</p>
                                    </div>
                                </div>
                            ) : previewUrl ? (
                                /* iframe 预览 - 用于 React 和 Vue */
                                <iframe
                                    ref={previewIframeRef}
                                    src={previewUrl}
                                    className="flex-1 w-full border-0"
                                    sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                                    onLoad={() => {
                                    }}
                                    onError={() => {
                                    }}
                                />
                            ) : (
                                /* 没有预览内容 */
                                <div className="flex-1 flex items-center justify-center text-muted-foreground">
                                    <div className="text-center">
                                        <p className="text-sm">等待预览数据...</p>
                                    </div>
                                </div>
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
                    initialDbId={runtimeConfig.db_id}
                    initialAssistantId={runtimeConfig.assistant_id}
                />
            )}
        </div>
    );
} 
