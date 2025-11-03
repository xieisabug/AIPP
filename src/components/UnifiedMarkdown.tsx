import React, { useMemo } from 'react';
import ReactMarkdown, { Components } from 'react-markdown';
import CodeBlock from './CodeBlock';
import { useMarkdownConfig } from '../hooks/useMarkdownConfig';

interface UnifiedMarkdownProps {
    children: string;
    onCodeRun?: (lang: string, code: string) => void;
    className?: string;
    disableMarkdownSyntax?: boolean;
    // 当为 true 时，不包裹默认的 prose 样式容器，便于父级自定义样式
    noProseWrapper?: boolean;
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
}) => {
    // 使用统一的markdown配置
    const markdownConfig = useMarkdownConfig({
        onCodeRun,
        disableMarkdownSyntax
    });

    // 自定义组件配置，包含antthinking组件
    const customComponents = useMemo((): CustomComponents => ({
        ...markdownConfig.markdownComponents,
        // 重写code组件以支持CodeBlock
        code({ className, children, ...props }) {
            const match = /language-(\w+)/.exec(className || '');

            // 如果禁用markdown语法，使用原始文本
            if (disableMarkdownSyntax) {
                return match ? (
                    <span>```{match[1]}{'\n'}{children}{'\n'}```</span>
                ) : (
                    <span>`{children}`</span>
                );
            }

            // 正常模式下使用CodeBlock
            return match ? (
                <CodeBlock
                    language={match[1]}
                    onCodeRun={onCodeRun || (() => { })}
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

    const markdownNode = (
        <ReactMarkdown
            children={children}
            // 交由 useMarkdownConfig 统一管理插件，避免重复添加
            remarkPlugins={[...markdownConfig.remarkPlugins] as any}
            rehypePlugins={[...markdownConfig.rehypePlugins] as any}
            components={customComponents}
        />
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
