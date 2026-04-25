import type { Message } from "@/data/Conversation";

const PREVIEW_CODE_JSON_PATTERN = /"tool_name"\s*:\s*"preview_code"/;
const PREVIEW_CODE_XML_PATTERN = /<tool_name>\s*preview_code\s*<\/tool_name>/i;

export function messageContainsPreviewCode(content: string | null | undefined): boolean {
    if (!content) {
        return false;
    }
    return PREVIEW_CODE_JSON_PATTERN.test(content) || PREVIEW_CODE_XML_PATTERN.test(content);
}

export function messagesContainPreviewCode(messages: readonly Pick<Message, "content">[]): boolean {
    return messages.some((message) => messageContainsPreviewCode(message.content));
}
