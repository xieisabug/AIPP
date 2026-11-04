import React, { useEffect, useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import { MarkdownStreamingContext, useMarkdownConfig } from '../hooks/useMarkdownConfig';
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
    /** Reuse the caller's configuration when it also renders MCP segments. */
    markdownConfig?: ReturnType<typeof useMarkdownConfig>;
}

/**
 * 统一的ReactMarkdown组件，确保在AskWindow和ConversationUI中的展示逻辑一致
 * 同时正确处理暗色模式下的文字颜色
 */
const ConfiguredMarkdown: React.FC<UnifiedMarkdownProps & {
    markdownConfig: ReturnType<typeof useMarkdownConfig>;
}> = ({
    children,
    className = '',
    noProseWrapper = false,
    isStreaming = false,
    markdownConfig,
}) => {
    const { content: optimizedContent, inlineImageIds } = useMemo(() => transformInlineImages(children), [children]);

    useEffect(() => {
        return () => {
            if (inlineImageIds.length) {
                releaseInlineImages(inlineImageIds);
            }
        };
    }, [inlineImageIds]);

    const markdownNode = (
        <MarkdownStreamingContext.Provider value={isStreaming}>
            <ReactMarkdown
                children={optimizedContent}
                // 交由 useMarkdownConfig 统一管理插件，避免重复添加
                remarkPlugins={[...markdownConfig.remarkPlugins] as any}
                rehypePlugins={[...markdownConfig.rehypePlugins] as any}
                components={markdownConfig.markdownComponents}
                urlTransform={customUrlTransform}
            />
        </MarkdownStreamingContext.Provider>
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

const StandaloneMarkdown: React.FC<UnifiedMarkdownProps> = (props) => {
    const markdownConfig = useMarkdownConfig({
        onCodeRun: props.onCodeRun,
        disableMarkdownSyntax: props.disableMarkdownSyntax,
    });
    return <ConfiguredMarkdown {...props} markdownConfig={markdownConfig} />;
};

const UnifiedMarkdown: React.FC<UnifiedMarkdownProps> = (props) => props.markdownConfig
    ? <ConfiguredMarkdown {...props} markdownConfig={props.markdownConfig} />
    : <StandaloneMarkdown {...props} />;

// 避免不必要的重渲染：仅在核心 props/children 变更时更新
export default React.memo(UnifiedMarkdown, (prev, next) => {
    return (
        prev.children === next.children &&
        prev.className === next.className &&
        prev.disableMarkdownSyntax === next.disableMarkdownSyntax &&
        prev.noProseWrapper === next.noProseWrapper &&
        prev.isStreaming === next.isStreaming &&
        prev.markdownConfig === next.markdownConfig &&
        prev.onCodeRun === next.onCodeRun
    );
});
