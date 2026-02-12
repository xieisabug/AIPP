import React, { useMemo, useEffect } from 'react';
import ReactMarkdown, { Components } from 'react-markdown';
import CodeBlock from './RustCodeBlock';
import { useMarkdownConfig } from '../hooks/useMarkdownConfig';
import { customUrlTransform } from '@/constants/markdown';
import { transformInlineImages, releaseInlineImages } from '@/lib/inlineImageStore';
import { resolveCodeBlockMeta } from '@/react-markdown/remarkCodeBlockMeta';

interface UnifiedMarkdownProps {
    children: string;
    onCodeRun?: (lang: string, code: string) => void;
    className?: string;
    disableMarkdownSyntax?: boolean;
    // 当为 true 时，不包裹默认的 prose 样式容器，便于父级自定义样式
    noProseWrapper?: boolean;
    // 是否处于流式输出中，用于代码块自动折叠的首次触发
    isStreaming?: boolean;
}

interface CustomComponents extends Components {
    antthinking: React.ElementType;
}

/**
 * 统一的ReactMarkdown组件，确保在AskWindow和ConversationUI中的展示逻辑一致
 * 同时正确处理暗色模式下的文字颜色
 */
const UnifiedMarkdown: React.FC<UnifiedMarkdownProps> = ({
    children,
    onCodeRun,
    className = '',
    disableMarkdownSyntax = false,
    noProseWrapper = false,
    isStreaming = false,
}) => {
    // 使用统一的markdown配置
    const markdownConfig = useMarkdownConfig({
        onCodeRun,
        disableMarkdownSyntax,
        isStreaming,
    });

    // 自定义组件配置，包含antthinking组件
    const customComponents = useMemo((): CustomComponents => ({
        ...markdownConfig.markdownComponents,
        // 重写code组件以支持CodeBlock
        code({ className, children, node, ...props }) {
            const match = /language-(\w+)/.exec(className || '');
            const meta = resolveCodeBlockMeta(props as Record<string, unknown>, node);
            const dataLanguage = typeof (props as Record<string, unknown>)["data-language"] === "string"
                ? (props as Record<string, unknown>)["data-language"] as string
                : undefined;
            const language = match?.[1] ?? dataLanguage ?? "text";
            const isBlock = Boolean(match || meta || dataLanguage);

            // 如果禁用markdown语法，使用原始文本
            if (disableMarkdownSyntax) {
                return isBlock ? (
                    <span>```{language}{'\n'}{children}{'\n'}```</span>
                ) : (
                    <span>`{children}`</span>
                );
            }

            // 正常模式下使用CodeBlock
            return isBlock ? (
                <CodeBlock
                    language={language}
                    meta={meta}
                    onCodeRun={onCodeRun || (() => { })}
                    isStreaming={isStreaming}
                >
                    {String(children).replace(/\n$/, '')}
                </CodeBlock>
            ) : (
                <code
                    {...props}
                    className={className}
                >
                    {children}
                </code>
            );
        },
        // antthinking自定义组件
        antthinking({ children }) {
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
    }), [markdownConfig.markdownComponents, onCodeRun, disableMarkdownSyntax]);

    const { content: optimizedContent, inlineImageIds } = useMemo(() => transformInlineImages(children), [children]);

    useEffect(() => {
        return () => {
            if (inlineImageIds.length) {
                releaseInlineImages(inlineImageIds);
            }
        };
    }, [inlineImageIds]);

    const markdownNode = (
        <ReactMarkdown
            children={optimizedContent}
            // 交由 useMarkdownConfig 统一管理插件，避免重复添加
            remarkPlugins={[...markdownConfig.remarkPlugins] as any}
            rehypePlugins={[...markdownConfig.rehypePlugins] as any}
            components={customComponents}
            urlTransform={customUrlTransform}
        />
    );

    if (noProseWrapper) {
        return markdownNode;
    }

    return (
        <div className={`prose prose-sm max-w-none prose-neutral dark:prose-invert text-foreground break-all ${className}`}>
            {markdownNode}
        </div>
    );
};

// 避免不必要的重渲染：仅在核心 props/children 变更时更新
export default React.memo(UnifiedMarkdown, (prev, next) => {
    return (
        prev.children === next.children &&
        prev.className === next.className &&
        prev.disableMarkdownSyntax === next.disableMarkdownSyntax &&
        prev.noProseWrapper === next.noProseWrapper &&
        prev.onCodeRun === next.onCodeRun
    );
});
