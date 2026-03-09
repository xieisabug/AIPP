import React, { useEffect, useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import { useMarkdownConfig } from '../hooks/useMarkdownConfig';
import { customUrlTransform } from '@/constants/markdown';
import { transformInlineImages, releaseInlineImages } from '@/lib/inlineImageStore';

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
            components={markdownConfig.markdownComponents}
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
        prev.isStreaming === next.isStreaming &&
        prev.onCodeRun === next.onCodeRun
    );
});
