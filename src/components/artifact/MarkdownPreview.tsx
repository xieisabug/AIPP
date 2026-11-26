import { memo } from "react";
import ReactMarkdown from "react-markdown";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import remarkMath from "remark-math";
import remarkBreaks from "remark-breaks";
import rehypeKatex from "rehype-katex";
import rehypeRaw from "rehype-raw";
import remarkCustomCompenent from "@/react-markdown/remarkCustomComponent";
import remarkCodeBlockMeta, { resolveCodeBlockMeta } from "@/react-markdown/remarkCodeBlockMeta";
import TipsComponent from "@/react-markdown/components/TipsComponent";
import { customUrlTransform } from "@/constants/markdown";
import "katex/dist/katex.min.css";

/* eslint-disable @typescript-eslint/no-explicit-any */
const markdownComponents: any = {
    tipscomponent: TipsComponent,
    code({ className, children, node, ...props }: any) {
        const match = /language-(\w+)/.exec(className || "");
        const meta = resolveCodeBlockMeta(props as Record<string, unknown>, node);
        const dataLanguage =
            typeof (props as Record<string, unknown>)["data-language"] === "string"
                ? ((props as Record<string, unknown>)["data-language"] as string)
                : undefined;
        const language = match?.[1] ?? dataLanguage ?? "text";
        const isInline = !match && !meta && !dataLanguage;
        if (isInline) {
            return (
                <code className={className} {...props}>
                    {children}
                </code>
            );
        }
        const metaLabel = meta
            ? [meta.title || meta.filename, meta.line ? `line ${meta.line}` : null, meta.highlight ? `highlight ${meta.highlight}` : null]
                  .filter(Boolean)
                  .join(" · ")
            : null;
        return (
            <div>
                {metaLabel && (
                    <div className="mb-2 text-xs text-muted-foreground font-mono truncate" title={metaLabel}>
                        {metaLabel}
                    </div>
                )}
                <SyntaxHighlighter style={oneDark as any} language={language} PreTag="div" {...props}>
                    {String(children).replace(/\n$/, "")}
                </SyntaxHighlighter>
            </div>
        );
    },
};
/* eslint-enable @typescript-eslint/no-explicit-any */

/** Markdown 文档预览（与聊天气泡同一套插件）。 */
function MarkdownPreview({ code }: { code: string }) {
    return (
        <div className="flex-1 overflow-auto bg-background p-6">
            <div className="prose prose-lg max-w-none dark:prose-invert">
                <ReactMarkdown
                    remarkPlugins={[remarkMath, remarkBreaks, remarkCodeBlockMeta, remarkCustomCompenent]}
                    rehypePlugins={[rehypeKatex, rehypeRaw]}
                    components={markdownComponents}
                    urlTransform={customUrlTransform}
                >
                    {code}
                </ReactMarkdown>
            </div>
        </div>
    );
}

export default memo(MarkdownPreview);
