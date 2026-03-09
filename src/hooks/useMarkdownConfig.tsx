import { useMemo, useCallback } from "react";
import React from "react";
import { Components } from "react-markdown";
import { openUrl } from "@tauri-apps/plugin-opener";
import rehypeRaw from "rehype-raw";
import rehypeKatex from "rehype-katex";
import rehypeSanitize from "rehype-sanitize";
import {
    REMARK_PLUGINS,
    MARKDOWN_COMPONENTS_BASE,
    buildSanitizeSchema,
} from "@/constants/markdown";
import CodeBlock from "@/components/RustCodeBlock";
import LazyImage from "@/components/common/LazyImage";
import { resolveCodeBlockMeta } from "@/react-markdown/remarkCodeBlockMeta";
import { useMarkdownRegistrySnapshot } from "@/services/markdownRegistry";

interface UseMarkdownConfigOptions {
    onCodeRun?: (lang: string, code: string) => void;
    disableMarkdownSyntax?: boolean;
    // 是否处于流式输出中，用于代码块自动折叠的首次触发
    isStreaming?: boolean;
}

function extractMarkdownAttributes(props: Record<string, unknown>): Record<string, string> {
    const attributes: Record<string, string> = {};
    Object.entries(props).forEach(([key, value]) => {
        if (value === undefined || value === null) {
            return;
        }
        if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
            attributes[key] = String(value);
        }
    });
    return attributes;
}

export const useMarkdownConfig = ({ onCodeRun, disableMarkdownSyntax = false, isStreaming = false }: UseMarkdownConfigOptions = {}) => {
    const registeredMarkdownTags = useMarkdownRegistrySnapshot();

    // 换行处理函数 - 完全按原样展示文本，保留所有换行和空行
    const renderTextWithBreaks = useCallback((children: React.ReactNode): React.ReactNode => {
        if (typeof children === "string") {
            return <span style={{ whiteSpace: "pre-wrap" }}>{children}</span>;
        }
        return children;
    }, []);

    const pluginMarkdownComponents = useMemo(() => {
        const components = {} as Record<string, React.ElementType>;
        registeredMarkdownTags.forEach((tag) => {
            components[tag.tagName] = ({ node, children, ...props }: any) =>
                tag.render({
                    node,
                    children,
                    attributes: extractMarkdownAttributes(props as Record<string, unknown>),
                    props,
                });
        });
        return components;
    }, [registeredMarkdownTags]);

    const markdownComponents = useMemo(
        (): Components => ({
            ...MARKDOWN_COMPONENTS_BASE,
            ...pluginMarkdownComponents,
            ...(disableMarkdownSyntax
                ? {
                    h1: ({ children }: any) => <span># {renderTextWithBreaks(children)}</span>,
                    h2: ({ children }: any) => <span>## {renderTextWithBreaks(children)}</span>,
                    h3: ({ children }: any) => <span>### {renderTextWithBreaks(children)}</span>,
                    h4: ({ children }: any) => <span>#### {renderTextWithBreaks(children)}</span>,
                    h5: ({ children }: any) => <span>##### {renderTextWithBreaks(children)}</span>,
                    h6: ({ children }: any) => <span>###### {renderTextWithBreaks(children)}</span>,
                    strong: ({ children }: any) => <span>**{children}**</span>,
                    em: ({ children }: any) => <span>*{children}*</span>,
                    blockquote: ({ children }: any) => <span>{"> "}{renderTextWithBreaks(children)}</span>,
                    ul: ({ children }: any) => <div>{children}</div>,
                    ol: ({ children }: any) => <div>{children}</div>,
                    li: ({ children }: any) => <div>- {renderTextWithBreaks(children)}</div>,
                    p: ({ children }: any) => <div>{renderTextWithBreaks(children)}</div>,
                    br: () => <br />,
                }
                : {}),
            code: ({ className, children, node, ...props }) => {
                const match = /language-(\w+)/.exec(className || "");
                const meta = resolveCodeBlockMeta(props as Record<string, unknown>, node);
                const dataLanguage = typeof (props as Record<string, unknown>)["data-language"] === "string"
                    ? ((props as Record<string, unknown>)["data-language"] as string)
                    : undefined;
                const language = match?.[1] ?? dataLanguage ?? "text";
                const isBlock = Boolean(match || meta || dataLanguage);

                if (disableMarkdownSyntax) {
                    return isBlock ? (
                        <span>```{language}{"\n"}{children}{"\n"}```</span>
                    ) : (
                        <span>`{children}`</span>
                    );
                }

                return isBlock ? (
                    <CodeBlock
                        language={language}
                        meta={meta}
                        onCodeRun={onCodeRun || (() => { })}
                        isStreaming={isStreaming}
                    >
                        {String(children).replace(/\n$/, "")}
                    </CodeBlock>
                ) : (
                    <code className={className} style={{ overflow: "auto" }}>
                        {children}
                    </code>
                );
            },
            a: ({ href, children, ...props }: any) => {
                const handleClick = useCallback(
                    (e: React.MouseEvent) => {
                        e.preventDefault();
                        if (href) {
                            openUrl(href).catch(console.error);
                        }
                    },
                    [href],
                );

                return (
                    <a
                        href={href}
                        onClick={handleClick}
                        className="text-primary hover:text-primary/80 underline cursor-pointer"
                        {...props}
                    >
                        {children}
                    </a>
                );
            },
            img: ({ src, alt, ...props }: any) => <LazyImage src={src} alt={alt} {...props} />,
        }),
        [
            disableMarkdownSyntax,
            isStreaming,
            onCodeRun,
            pluginMarkdownComponents,
            renderTextWithBreaks,
        ],
    );

    const remarkPlugins = useMemo(() => {
        if (disableMarkdownSyntax) {
            return [
                REMARK_PLUGINS.find((plugin) => plugin.name === "remarkCustomCompenent") || REMARK_PLUGINS[3],
            ].filter(Boolean);
        }
        return REMARK_PLUGINS;
    }, [disableMarkdownSyntax]);

    const rehypePlugins = useMemo(() => {
        const sanitizeSchema = buildSanitizeSchema(
            registeredMarkdownTags.map((tag) => ({
                tagName: tag.tagName,
                attributes: tag.attributes,
            })),
        );
        if (disableMarkdownSyntax) {
            return [
                rehypeRaw,
                [rehypeSanitize, sanitizeSchema],
            ];
        }
        return [
            rehypeRaw,
            [rehypeSanitize, sanitizeSchema],
            rehypeKatex,
        ];
    }, [disableMarkdownSyntax, registeredMarkdownTags]);

    return {
        remarkPlugins,
        rehypePlugins,
        markdownComponents,
    };
};
