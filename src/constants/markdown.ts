import remarkMath from "remark-math";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import rehypeRaw from "rehype-raw";
import rehypeKatex from "rehype-katex";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import remarkCustomCompenent from "@/react-markdown/remarkCustomComponent";
import TipsComponent from "@/react-markdown/components/TipsComponent";
import MessageFileAttachment from "@/components/MessageFileAttachment";
import MessageWebContent from "@/components/conversation/MessageWebContent";

// ReactMarkdown 插件配置
export const REMARK_PLUGINS = [
    remarkMath,
    remarkBreaks,
    remarkGfm,
    remarkCustomCompenent,
] as const;

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
    },
};

export const REHYPE_PLUGINS = [
    rehypeRaw,
    [rehypeSanitize, SANITIZE_SCHEMA] as const,
    rehypeKatex,
    // Note: Code highlighting is handled by Streamdown's built-in Shiki support
] as const;

// ReactMarkdown 组件配置的基础部分 - 移除无用的 mcp_tool_call
export const MARKDOWN_COMPONENTS_BASE = {
    fileattachment: MessageFileAttachment,
    bangwebtomarkdown: MessageWebContent,
    bangweb: MessageWebContent,
    tipscomponent: TipsComponent,
} as const;
