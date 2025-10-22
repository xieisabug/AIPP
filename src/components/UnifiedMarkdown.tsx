import React, { useMemo } from 'react';
import type { Options } from 'react-markdown';
import { useMarkdownConfig } from '../hooks/useMarkdownConfig';
import { useCodeTheme } from '../hooks/useCodeTheme';
import type { BundledTheme } from 'shiki';
import { Response } from '@/components/ai-elements/response';

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

    const markdownNode = (
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