import React, { useMemo } from "react";
import ReactMarkdown from "react-markdown";
import {
    MARKDOWN_COMPONENTS_BASE,
    REMARK_PLUGINS,
    REHYPE_PLUGINS,
    customUrlTransform,
} from "@/constants/markdown";

interface RawTextRendererProps {
    content: string;
}

// Render raw text while still allowing custom tags to hydrate as React components.
const CUSTOM_TAG_PATTERN =
    /<(fileattachment|bangwebtomarkdown|bangweb|tipscomponent)\b[^>]*>[\s\S]*?<\/\1>|<(fileattachment|bangwebtomarkdown|bangweb|tipscomponent)\b[^>]*\/>/gi;

const RawTextRenderer: React.FC<RawTextRendererProps> = ({ content }) => {
    const processedContent = useMemo(() => {
        const segments: React.ReactNode[] = [];
        let cursor = 0;

        for (const match of content.matchAll(CUSTOM_TAG_PATTERN)) {
            const start = match.index ?? 0;
            const tagText = match[0];

            if (start > cursor) {
                const plainText = content.slice(cursor, start);
                segments.push(
                    <span
                        key={`text-${cursor}`}
                        className="whitespace-pre-wrap break-all"
                    >
                        {plainText}
                    </span>,
                );
            }

            segments.push(
                <ReactMarkdown
                    key={`tag-${start}`}
                    children={tagText}
                    remarkPlugins={[
                        REMARK_PLUGINS.find(
                            (plugin) => plugin.name === "remarkCustomCompenent",
                        ) || REMARK_PLUGINS[3],
                    ].filter(Boolean) as any}
                    rehypePlugins={[REHYPE_PLUGINS[0], REHYPE_PLUGINS[1]] as any}
                    components={MARKDOWN_COMPONENTS_BASE as any}
                    urlTransform={customUrlTransform}
                />,
            );

            cursor = start + tagText.length;
        }

        if (cursor < content.length) {
            const trailingText = content.slice(cursor);
            segments.push(
                <span
                    key={`text-${cursor}`}
                    className="whitespace-pre-wrap break-all"
                >
                    {trailingText}
                </span>,
            );
        }

        if (segments.length === 0) {
            return (
                <span className="whitespace-pre-wrap break-all">
                    {content}
                </span>
            );
        }

        return segments;
    }, [content]);

    return (
        <div className="text-sm text-neutral-900 dark:text-neutral-100">
            {processedContent}
        </div>
    );
};

export default RawTextRenderer;
