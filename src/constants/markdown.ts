import remarkMath from "remark-math";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import rehypeRaw from "rehype-raw";
import rehypeKatex from "rehype-katex";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import { defaultUrlTransform } from "react-markdown";
import remarkCustomCompenent from "@/react-markdown/remarkCustomComponent";
import remarkCodeBlockMeta from "@/react-markdown/remarkCodeBlockMeta";
import TipsComponent from "@/react-markdown/components/TipsComponent";
import MessageFileAttachment from "@/components/MessageFileAttachment";
import MessageWebContent from "@/components/conversation/MessageWebContent";
import LazyImage from "@/components/common/LazyImage";
// highlight is disabled to avoid missing deps in this revert

// ReactMarkdown 插件配置
export const REMARK_PLUGINS = [
    remarkMath,
    remarkBreaks,
    remarkGfm,
    remarkCodeBlockMeta,
    remarkCustomCompenent,
] as const;

// 自定义 URL 转换函数，允许 data: 协议（用于 base64 图片）
export const customUrlTransform = (url: string): string => {
    // 允许 data: URI（base64 图片等）
    if (url.startsWith("data:")) {
        return url;
    }
    // 其他 URL 使用默认转换
    return defaultUrlTransform(url);
};

// 简化的清理配置 - 移除无用的 mcp_tool_call 相关配置
export const SANITIZE_SCHEMA = {
    ...defaultSchema,
    tagNames: [
        ...(defaultSchema.tagNames || []),
        "fileattachment",
        "bangwebtomarkdown",
        "bangweb",
        // 允许自定义 Tips 组件标签
        "tipscomponent",
        "inlineimage",
    ],
    attributes: {
        ...(defaultSchema.attributes || {}),
        fileattachment: [
            ...(defaultSchema.attributes?.fileattachment || []),
            "attachment_id",
            "attachment_url",
            "attachment_type",
            "attachment_content",
        ],
        bangwebtomarkdown: [
            ...(defaultSchema.attributes?.bangwebtomarkdown || []),
            "url",
        ],
        bangweb: [...(defaultSchema.attributes?.bangweb || []), "url"],
        // 允许 tipscomponent 透传 text 属性
        tipscomponent: [
            ...(defaultSchema.attributes?.tipscomponent || []),
            "text",
        ],
        inlineimage: [
            ...(defaultSchema.attributes?.inlineimage || []),
            "data-inline-id",
            "data-inlineid",
            "dataInlineId",
            "data-alt",
            "dataalt",
            "dataAlt",
            "className",
        ],
        pre: [
            ...(defaultSchema.attributes?.pre || []),
            "data-language",
            "data-title",
            "data-filename",
            "data-line",
            "data-highlight",
            "data-meta",
        ],
        code: [
            ...(defaultSchema.attributes?.code || []),
            "data-language",
            "data-title",
            "data-filename",
            "data-line",
            "data-highlight",
            "data-meta",
        ],
    },
    // 允许 data: URI 协议用于内联图片 (base64 图片)
    protocols: {
        ...defaultSchema.protocols,
        src: ["http", "https", "data"],
    },
};

export const REHYPE_PLUGINS = [
    rehypeRaw,
    [rehypeSanitize, SANITIZE_SCHEMA] as const,
    rehypeKatex,
] as const;

// ReactMarkdown 组件配置的基础部分 - 移除无用的 mcp_tool_call
export const MARKDOWN_COMPONENTS_BASE = {
    fileattachment: MessageFileAttachment,
    bangwebtomarkdown: MessageWebContent,
    bangweb: MessageWebContent,
    tipscomponent: TipsComponent,
    inlineimage: LazyImage,
} as const;
