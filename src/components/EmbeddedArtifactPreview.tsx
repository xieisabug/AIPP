/**
 * 嵌入式 Artifact 预览组件
 * 可在任意窗口内嵌入，用于预览代码 artifacts
 */
import { useEffect, useRef, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
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
import { useArtifactEvents, ArtifactData, EnvironmentCheckData } from '@/hooks/useArtifactEvents';
import { Button } from '@/components/ui/button';
import { Loader2 } from 'lucide-react';
import EnvironmentInstallDialog from '@/components/EnvironmentInstallDialog';
import 'katex/dist/katex.min.css';

interface EmbeddedArtifactPreviewProps {
    className?: string;
}

type PreviewType = 'react' | 'vue' | 'mermaid' | 'html' | 'svg' | 'xml' | 'markdown' | 'md' | 'drawio' | null;

export default function EmbeddedArtifactPreview({ className }: EmbeddedArtifactPreviewProps) {
    const [previewUrl, setPreviewUrl] = useState<string | null>(null);
    const [isPreviewReady, setIsPreviewReady] = useState(false);
    const [currentView, setCurrentView] = useState<'logs' | 'preview'>('logs');
    const [previewType, setPreviewType] = useState<PreviewType>(null);
    const previewTypeRef = useRef<PreviewType>(null);
    const mermaidContainerRef = useRef<HTMLDivElement | null>(null);
    const [mermaidContent, setMermaidContent] = useState<string>('');
    const [htmlContent, setHtmlContent] = useState<string>('');
    const [markdownContent, setMarkdownContent] = useState<string>('');
    const [drawioXmlContent, setDrawioXmlContent] = useState<string>('');
    const drawioIframeRef = useRef<HTMLIFrameElement>(null);
    const logsEndRef = useRef<HTMLDivElement | null>(null);
    const isInstalling = useRef<boolean>(false);

    // 环境安装相关状态
    const [showEnvironmentDialog, setShowEnvironmentDialog] = useState<boolean>(false);
    const [environmentTool, setEnvironmentTool] = useState<string>('');
    const [environmentMessage, setEnvironmentMessage] = useState<string>('');
    const [currentLang, setCurrentLang] = useState<string>('');
    const [currentInputStr, setCurrentInputStr] = useState<string>('');

    const currentLangRef = useRef<string>('');
    const currentInputStrRef = useRef<string>('');

    useEffect(() => {
        previewTypeRef.current = previewType;
    }, [previewType]);

    useEffect(() => {
        currentLangRef.current = currentLang;
        currentInputStrRef.current = currentInputStr;
    }, [currentLang, currentInputStr]);

    // 重置预览状态
    const resetPreviewState = useCallback(async () => {
        const currentType = previewTypeRef.current;
        if (currentType === 'vue') {
            try {
                await invoke('close_vue_preview', { previewId: 'vue' });
            } catch (e) {
                console.warn('关闭 Vue 预览失败:', e);
            }
        } else if (currentType === 'react') {
            try {
                await invoke('close_react_preview', { previewId: 'react' });
            } catch (e) {
                console.warn('关闭 React 预览失败:', e);
            }
        }

        setPreviewUrl(null);
        setPreviewType(null);
        setMermaidContent('');
        setHtmlContent('');
        setMarkdownContent('');
        setDrawioXmlContent('');
        setIsPreviewReady(false);
        setCurrentView('logs');
    }, []);

    // 处理 artifact 数据
    const handleArtifactData = useCallback((data: ArtifactData) => {
        if (data.original_code && data.type) {
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
        onArtifactData: handleArtifactData,
        onRedirect: handleRedirect,
        onEnvironmentCheck: handleEnvironmentCheck,
        onEnvironmentInstallStarted: handleEnvironmentInstallStarted,
        onBunInstallFinished: handleBunInstallFinished,
        onUvInstallFinished: handleUvInstallFinished,
        onReset: resetPreviewState,
    });

    // 初始化 mermaid
    useEffect(() => {
        const isDark = document.documentElement.classList.contains('dark');
        mermaid.initialize({
            startOnLoad: false,
            theme: isDark ? 'dark' : 'default',
            securityLevel: 'loose',
            fontFamily: 'monospace',
            themeVariables: { darkMode: isDark },
            flowchart: { useMaxWidth: false },
        });
    }, []);

    // 自动滚动到底部
    useEffect(() => {
        logsEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [artifactEvents.logs]);

    // 渲染 mermaid 图表
    useEffect(() => {
        if (previewType === 'mermaid' && currentView === 'preview' && mermaidContent && mermaidContainerRef.current) {
            const renderMermaid = async () => {
                try {
                    const container = mermaidContainerRef.current;
                    if (!container) return;
                    container.innerHTML = '';
                    const { svg } = await mermaid.render('mermaid-' + Date.now(), mermaidContent);
                    container.innerHTML = svg;
                } catch (error) {
                    console.error('Mermaid 渲染失败:', error);
                    if (mermaidContainerRef.current) {
                        mermaidContainerRef.current.innerHTML = `<pre class="text-destructive p-4">${error}</pre>`;
                    }
                }
            };
            renderMermaid();
        }
    }, [previewType, mermaidContent, currentView]);

    // 当预览准备好时，切换到预览视图
    useEffect(() => {
        if (isPreviewReady && (previewUrl || previewType === 'mermaid' || previewType === 'html' || previewType === 'svg' || previewType === 'xml' || previewType === 'markdown' || previewType === 'md' || previewType === 'drawio')) {
            setCurrentView('preview');
        }
    }, [isPreviewReady, previewUrl, previewType]);

    // draw.io postMessage 通信
    useEffect(() => {
        if (previewType === 'drawio' && drawioXmlContent) {
            let loaded = false;

            const handleMessage = (evt: MessageEvent) => {
                if (typeof evt.data !== 'string' || evt.data.length === 0) return;
                try {
                    const msg = JSON.parse(evt.data);
                    if (msg.event === 'configure') {
                        drawioIframeRef.current?.contentWindow?.postMessage(
                            JSON.stringify({
                                action: 'configure',
                                config: { defaultFonts: ['Humor Sans', 'Microsoft YaHei', 'SimHei'] }
                            }),
                            '*'
                        );
                    } else if (msg.event === 'init' && !loaded) {
                        loaded = true;
                        drawioIframeRef.current?.contentWindow?.postMessage(
                            JSON.stringify({ action: 'load', xml: drawioXmlContent, autosave: 0 }),
                            '*'
                        );
                    }
                } catch {
                    // ignore
                }
            };

            window.addEventListener('message', handleMessage);
            return () => window.removeEventListener('message', handleMessage);
        }
    }, [previewType, drawioXmlContent]);

    const handleToggleView = () => {
        setCurrentView(current => current === 'logs' ? 'preview' : 'logs');
    };

    const handleEnvironmentInstallConfirm = async () => {
        try {
            if (environmentTool === 'bun') {
                await invoke('install_bun');
            } else if (environmentTool === 'uv') {
                await invoke('install_uv');
            }
            setShowEnvironmentDialog(false);
        } catch (error) {
            artifactEvents.addLog('error', `安装失败: ${error}`);
        }
    };

    const handleEnvironmentInstallCancel = async () => {
        try {
            if (environmentTool === 'bun') {
                await invoke('cancel_bun_install');
            } else if (environmentTool === 'uv') {
                await invoke('cancel_uv_install');
            }
            setShowEnvironmentDialog(false);
        } catch (error) {
            artifactEvents.addLog('error', `取消安装失败: ${error}`);
        }
    };

    // 如果没有数据，显示空状态
    if (!artifactEvents.hasReceivedData && artifactEvents.logs.length === 0) {
        return (
            <div className={`flex items-center justify-center h-full text-muted-foreground ${className}`}>
                <div className="text-center">
                    <Loader2 className="h-8 w-8 mx-auto mb-3 animate-spin opacity-50" />
                    <p className="text-sm">等待预览数据...</p>
                </div>
            </div>
        );
    }

    const mdComponents: any = {
        tipscomponent: TipsComponent,
        code({ className: codeClassName, children, node, ...props }: any) {
            const match = /language-(\w+)/.exec(codeClassName || '');
            const meta = resolveCodeBlockMeta(props as Record<string, unknown>, node);
            const dataLanguage = typeof (props as Record<string, unknown>)['data-language'] === 'string'
                ? (props as Record<string, unknown>)['data-language'] as string
                : undefined;
            const language = match?.[1] ?? dataLanguage ?? 'text';
            const isInline = !match && !meta && !dataLanguage;
            return !isInline ? (
                <SyntaxHighlighter style={oneDark as any} language={language} PreTag="div" {...props}>
                    {String(children).replace(/\n$/, '')}
                </SyntaxHighlighter>
            ) : (
                <code className={codeClassName} {...props}>{children}</code>
            );
        },
    };

    return (
        <div className={`flex flex-col h-full ${className}`}>
            {/* 顶部工具栏 */}
            {isPreviewReady && (
                <div className="flex-shrink-0 p-2 border-b border-border flex items-center justify-between">
                    <div className="text-xs text-muted-foreground">
                        {currentView === 'logs' ? '日志' :
                            previewType === 'mermaid' ? 'Mermaid' :
                                previewType === 'html' ? 'HTML' :
                                    previewType === 'svg' ? 'SVG' :
                                        previewType === 'xml' ? 'XML' :
                                            previewType === 'markdown' || previewType === 'md' ? 'Markdown' :
                                                previewType === 'drawio' ? 'Draw.io' :
                                                    previewType === 'react' ? 'React' :
                                                        previewType === 'vue' ? 'Vue' : '预览'}
                    </div>
                    <Button onClick={handleToggleView} variant="ghost" size="sm" className="h-6 text-xs">
                        {currentView === 'logs' ? '查看预览' : '查看日志'}
                    </Button>
                </div>
            )}

            {/* 主要内容区域 */}
            <div className="flex-1 min-h-0 overflow-hidden">
                {currentView === 'logs' ? (
                    /* 日志视图 */
                    <div className="h-full overflow-y-auto p-3">
                        <div className="text-xs font-mono space-y-1">
                            {artifactEvents.logs.map((log, idx) => (
                                <div
                                    key={idx}
                                    className={
                                        log.type === 'error' ? 'text-destructive' :
                                            log.type === 'success' ? 'text-green-600 dark:text-green-400' :
                                                'text-foreground'
                                    }
                                >
                                    {log.message}
                                </div>
                            ))}
                            <div ref={logsEndRef} />
                        </div>
                    </div>
                ) : (
                    /* 预览视图 */
                    <div className="h-full">
                        {previewType === 'mermaid' ? (
                            <div ref={mermaidContainerRef} className="h-full overflow-auto p-4 flex items-center justify-center" />
                        ) : previewType === 'markdown' || previewType === 'md' ? (
                            <div className="h-full overflow-auto p-4">
                                <div className="prose prose-sm max-w-none dark:prose-invert">
                                    <ReactMarkdown
                                        remarkPlugins={[remarkMath, remarkBreaks, remarkCodeBlockMeta, remarkCustomCompenent]}
                                        rehypePlugins={[rehypeKatex, rehypeRaw]}
                                        components={mdComponents}
                                    >
                                        {markdownContent}
                                    </ReactMarkdown>
                                </div>
                            </div>
                        ) : previewType === 'html' || previewType === 'svg' || previewType === 'xml' ? (
                            <iframe
                                srcDoc={htmlContent}
                                className="w-full h-full border-0 bg-background"
                                sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                            />
                        ) : previewType === 'drawio' ? (
                            <iframe
                                ref={drawioIframeRef}
                                src="https://embed.diagrams.net/?embed=1&ui=min&spin=1&proto=json&noSaveBtn=1&noExitBtn=1"
                                className="w-full h-full border-0 bg-background"
                                sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                            />
                        ) : (
                            /* React/Vue iframe 预览 */
                            <iframe
                                src={previewUrl || ''}
                                className="w-full h-full border-0"
                                sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                            />
                        )}
                    </div>
                )}
            </div>

            {/* 环境安装对话框 */}
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
