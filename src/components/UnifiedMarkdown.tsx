import React, { useMemo, useEffect, useRef } from 'react';
import { Streamdown } from 'streamdown';
import type { Options } from 'react-markdown';
import { useMarkdownConfig } from '../hooks/useMarkdownConfig';
import { useCodeTheme } from '../hooks/useCodeTheme';
import type { BundledTheme } from 'shiki';

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

    // 用于访问 DOM 并添加自定义按钮
    const containerRef = useRef<HTMLDivElement>(null);

    // 在 Streamdown 渲染后，找到所有代码块并添加自定义运行按钮
    useEffect(() => {
        if (!onCodeRun || !containerRef.current) return;

        const codeBlocks = containerRef.current.querySelectorAll('[data-streamdown="code-block"]');
        
        codeBlocks.forEach((block) => {
            // 检查是否已经添加过自定义按钮
            if (block.querySelector('.custom-run-button')) return;

            // 获取语言信息
            const preElement = block.querySelector('pre');
            const codeElement = preElement?.querySelector('code');
            if (!codeElement) return;

            // 从 class 中提取语言
            const languageMatch = codeElement.className.match(/language-(\w+)/);
            const language = languageMatch ? languageMatch[1] : 'text';

            // 创建运行按钮容器
            const buttonContainer = document.createElement('div');
            buttonContainer.className = 'custom-run-button absolute right-2 top-2 z-10';
            
            // 创建运行按钮
            const runButton = document.createElement('button');
            runButton.className = 'p-1.5 rounded hover:bg-muted transition-colors';
            runButton.innerHTML = `<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>`;
            runButton.title = 'Run code';
            
            runButton.onclick = () => {
                const code = codeElement.textContent || '';
                onCodeRun(language, code);
            };

            buttonContainer.appendChild(runButton);

            // 将按钮添加到代码块的 header 区域
            const header = block.querySelector('[data-code-block-header]');
            if (header) {
                // 设置 header 为相对定位，以便按钮绝对定位
                (header as HTMLElement).style.position = 'relative';
                header.appendChild(buttonContainer);
            }
        });
    }, [children, onCodeRun]);

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

    const markdownNode = (
        <div ref={containerRef}>
            <Streamdown
                parseIncompleteMarkdown={!disableMarkdownSyntax}
                // 使用统一的 Remark 插件；Rehype 使用 Streamdown 默认（包含 harden/raw/katex），避免 sanitize 误删 Shiki 类
                remarkPlugins={[...markdownConfig.remarkPlugins] as any}
                components={customComponents}
                shikiTheme={[lightTheme as BundledTheme, darkTheme as BundledTheme]}
                controls={{
                    code: true, // 启用默认复制按钮，我们会添加额外的运行按钮
                    table: true,
                    mermaid: true,
                }}
            >
                {children}
            </Streamdown>
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