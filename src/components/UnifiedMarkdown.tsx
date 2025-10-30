import React, { useEffect, useMemo, useRef } from 'react';
import type { Options } from 'react-markdown';
import { useMarkdownConfig } from '../hooks/useMarkdownConfig';
import { useCodeTheme } from '../hooks/useCodeTheme';
import type { BundledTheme } from 'shiki';
import { Response } from '@/components/ai-elements/response';
import { invoke } from '@tauri-apps/api/core';

interface UnifiedMarkdownProps {
    children: string;
    onCodeRun?: (lang: string, code: string) => void;
    className?: string;
    disableMarkdownSyntax?: boolean;
    // 当为 true 时，不包裹默认的 prose 样式容器，便于父级自定义样式
    noProseWrapper?: boolean;
}

type CustomComponents = NonNullable<Options['components']> & {
    antthinking: React.ElementType;
};

/**
 * 统一的Streamdown组件，确保在AskWindow和ConversationUI中的展示逻辑一致
 * 同时正确处理暗色模式下的文字颜色
 */
const UnifiedMarkdown: React.FC<UnifiedMarkdownProps> = ({
    children,
    onCodeRun,
    className = '',
    disableMarkdownSyntax = false,
    noProseWrapper = false,
}) => {
    // 使用统一的markdown配置
    const markdownConfig = useMarkdownConfig({
        onCodeRun,
        disableMarkdownSyntax
    });

    // 获取当前代码主题（提供明暗两套给 Shiki）
    const { lightTheme, darkTheme } = useCodeTheme();

    // 迁移到 ai-elements 的 Response 组件后，不再手动注入运行按钮。

    // 自定义组件配置，只包含需要的自定义组件，避免覆盖 Streamdown 的 code 渲染
    const customComponents = useMemo((): CustomComponents => ({
        // 注意：不要传入 markdownConfig.markdownComponents 中的 code 覆盖，
        // 以免破坏 Streamdown 内置的 Shiki 高亮与结构
        ...(Object.fromEntries(
            Object.entries(markdownConfig.markdownComponents).filter(([key]) => key !== 'code')
        ) as CustomComponents),
        // antthinking自定义组件
        antthinking({ children }: any) {
            return (
                <div>
                    <div
                        className="bg-primary/10 text-primary px-2 py-1 rounded text-sm font-medium inline-block"
                        title={children}
                        data-thinking={children}
                    >
                        思考...
                    </div>
                </div>
            );
        },
    }), [markdownConfig.markdownComponents]);

    const containerRef = useRef<HTMLDivElement | null>(null);

    // 注入“运行”按钮至每个代码块的工具栏（不破坏 Streamdown/Shiki 默认渲染）
    useEffect(() => {
        if (!onCodeRun) return; // 未提供回调则不注入
        const root = containerRef.current;
        if (!root) return;

        const markerAttr = 'data-run-code-button';

        const injectRunButtons = (scope: HTMLElement) => {
            const headers = scope.querySelectorAll<HTMLDivElement>('[data-code-block-header]');
            headers.forEach((header) => {
                // actions 容器为 header 内最后一个 flex 区域
                const actions = header.querySelector<HTMLDivElement>('div.flex.items-center.gap-2')
                    || header.querySelector<HTMLDivElement>('div:last-child');
                if (!actions) return;

                if (actions.querySelector(`[${markerAttr}]`)) return; // 已注入

                const parentContainer = header.closest('[data-code-block-container]') as HTMLElement | null;
                if (!parentContainer) return;

                const btn = document.createElement('button');
                btn.setAttribute(markerAttr, 'true');
                btn.type = 'button';
                btn.title = '运行代码';
                btn.setAttribute('aria-label', '运行代码');
                btn.className = 'cursor-pointer p-1 text-muted-foreground transition-all hover:text-foreground';
                // 使用与内置按钮一致的尺寸（14px），采用内联 SVG 的播放图标
                btn.innerHTML = `
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
                        <polygon points="5 3 19 12 5 21 5 3"></polygon>
                    </svg>
                `;

                btn.addEventListener('click', () => {
                    try {
                        const language = (parentContainer.getAttribute('data-language') || '').toString();
                        // 优先选择当前可见的 code 区域；若无法判断则取第一个
                        const blocks = parentContainer.querySelectorAll<HTMLElement>('[data-code-block]');
                        let codeEl: HTMLElement | null = null;
                        if (blocks.length === 1) {
                            codeEl = blocks[0];
                        } else if (blocks.length > 1) {
                            codeEl = Array.from(blocks).find(el => el.offsetParent !== null) || blocks[0];
                        }
                        const code = (codeEl?.textContent || '').toString();
                        onCodeRun?.(language, code);
                    } catch {
                        // 忽略点击错误，避免影响其他按钮
                    }
                });

                actions.appendChild(btn);
            });
        };

        // 初次注入
        injectRunButtons(root);

        // 监听流式/动态变更，增量注入
        const observer = new MutationObserver((mutations) => {
            for (const m of mutations) {
                if (m.type === 'childList') {
                    // 尝试在变更节点内注入
                    m.addedNodes.forEach((node) => {
                        if (node instanceof HTMLElement) {
                            injectRunButtons(node);
                        }
                    });
                }
            }
        });
        observer.observe(root, { childList: true, subtree: true });

        return () => observer.disconnect();
    }, [onCodeRun]);

    // 注入"Mermaid 全屏预览"按钮，并在新窗口中打开交互预览
    useEffect(() => {
        const root = containerRef.current;
        if (!root) return;

        const markerAttr = 'data-open-mermaid-button';
        
        // 从 children (Markdown 源码) 中提取所有 mermaid 代码块
        const extractMermaidBlocks = (markdown: string): Map<number, string> => {
            const blocks = new Map<number, string>();
            const regex = /```mermaid\s*\n([\s\S]*?)```/g;
            let match;
            let index = 0;
            while ((match = regex.exec(markdown)) !== null) {
                // 不使用 trim()，保留原始格式和换行
                const code = match[1];
                blocks.set(index++, code);
            }
            return blocks;
        };
        
        const mermaidBlocks = extractMermaidBlocks(children);

        const injectButtons = (scope: HTMLElement) => {
            const blocks = scope.querySelectorAll<HTMLElement>('[data-streamdown="mermaid-block"]');
            blocks.forEach((block, index) => {
                // 工具栏是 mermaid block 的第一个子元素（controls 启用时）
                const header = block.querySelector(':scope > div');
                if (!header) return;
                if (header.querySelector(`[${markerAttr}]`)) return; // 已注入

                const btn = document.createElement('button');
                btn.setAttribute(markerAttr, 'true');
                btn.type = 'button';
                btn.title = '在新窗口打开';
                btn.setAttribute('aria-label', '在新窗口打开');
                btn.className = 'cursor-pointer p-1 text-muted-foreground transition-all hover:text-foreground';
                // 使用与内置按钮一致的 14px 图标
                btn.innerHTML = `
                  <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
                    <path d="M15 3h6v6"/>
                    <path d="M10 14 21 3"/>
                    <path d="M21 14v7H3V3h7"/>
                  </svg>`;

                btn.addEventListener('click', async () => {
                    try {
                        const mermaidCode = mermaidBlocks.get(index);
                        
                        if (!mermaidCode) {
                            console.warn('[Mermaid Open] 无法提取 mermaid 代码，索引:', index);
                            return;
                        }
                        
                        await invoke("run_artifacts", { lang: 'mermaid', inputStr: mermaidCode });
                    } catch (e) {
                        console.error('[Mermaid Open] failed', e);
                    }
                });

                // 将按钮追加到工具栏（右侧 actions 区域）
                // const actions = header.querySelector('div.flex.items-center.gap-2') || header.lastElementChild;
                // (actions as HTMLElement | null)?.appendChild(btn);
                header.appendChild(btn);
            });
        };

        // 初次注入
        injectButtons(root);

        const observer = new MutationObserver((mutations) => {
            for (const m of mutations) {
                if (m.type === 'childList') {
                    m.addedNodes.forEach((node) => {
                        if (node instanceof HTMLElement) {
                            injectButtons(node);
                        }
                    });
                }
            }
        });
        observer.observe(root, { childList: true, subtree: true });

        return () => observer.disconnect();
    }, [children]);

    const markdownNode = (
        <div ref={containerRef} className="contents">
            <Response
                parseIncompleteMarkdown={!disableMarkdownSyntax}
                // 使用统一的 Remark 插件；Rehype 使用 Response/Streamdown 默认（包含 harden/raw/katex）
                remarkPlugins={[...markdownConfig.remarkPlugins] as any}
                components={customComponents}
                shikiTheme={[lightTheme as BundledTheme, darkTheme as BundledTheme]}
                controls={{
                    code: true,
                    table: true,
                    mermaid: true,
                }}
            >
                {children}
            </Response>
        </div>
    );

    if (noProseWrapper) {
        return markdownNode;
    }

    return (
        <div className={`prose prose-sm max-w-none prose-neutral dark:prose-invert text-foreground ${className}`}>
            {markdownNode}
        </div>
    );
};

export default UnifiedMarkdown;
