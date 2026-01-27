import { useMemo } from 'react';
import { FileInfo, MCPToolCallUpdateEvent, Message } from '../data/Conversation';
import { ContextItem, CONTEXT_TOOL_NAMES, SearchResultItem, getContextTypeFromToolName } from '../components/chat-sidebar/types';

interface Attachment {
    attachment_url: string;
    attachment_content: string;
    attachment_type: string;
}

interface UseContextListOptions {
    // User provided files from input area (pending to be sent)
    userFiles: FileInfo[] | null;
    // MCP tool call states
    mcpToolCallStates: Map<number, MCPToolCallUpdateEvent>;
    // Messages in conversation (to extract sent attachments)
    messages?: Message[];
}

interface UseContextListReturn {
    contextItems: ContextItem[];
}

const parseSearchResultItems = (rawResult: string): SearchResultItem[] | undefined => {
    if (!rawResult) return undefined;
    try {
        const parsed = JSON.parse(rawResult);
        const parts = Array.isArray(parsed) ? parsed : parsed?.content;
        if (!Array.isArray(parts)) return undefined;
        const jsonPart = parts.find((part) => part?.type === 'json');
        const payload = jsonPart?.json;
        const items = Array.isArray(payload) ? payload : payload?.items;
        if (!Array.isArray(items)) return undefined;
        return items
            .map((item: any) => ({
                title: typeof item?.title === 'string' ? item.title : '',
                url: typeof item?.url === 'string' ? item.url : '',
                snippet: typeof item?.snippet === 'string' ? item.snippet : '',
                displayUrl: typeof item?.display_url === 'string' ? item.display_url : undefined,
                rank: typeof item?.rank === 'number' ? item.rank : undefined,
            }))
            .filter((item: SearchResultItem) => item.title && item.url);
    } catch {
        return undefined;
    }
};

const getAttachmentTypeName = (type: string): string => {
    switch (type) {
        case 'Image':
            return '图片';
        case 'Text':
            return '文本';
        case 'PDF':
            return 'PDF';
        case 'Word':
            return 'Word';
        case 'PowerPoint':
            return 'PPT';
        case 'Excel':
            return 'Excel';
        default:
            return '文件';
    }
};

export function useContextList({ userFiles, mcpToolCallStates, messages }: UseContextListOptions): UseContextListReturn {
    const contextItems = useMemo(() => {
        const items: ContextItem[] = [];

        // Track attachment counts by type for naming
        const attachmentCounts: Record<string, number> = {};

        // Add attachments from messages (sent files)
        if (messages && messages.length > 0) {
            for (const message of messages) {
                if (message.message_type !== 'user') continue;
                if (!message.attachment_list || message.attachment_list.length === 0) continue;

                for (const attachment of message.attachment_list as Attachment[]) {
                    const attType = attachment.attachment_type || 'File';
                    attachmentCounts[attType] = (attachmentCounts[attType] || 0) + 1;
                    const displayName = `${getAttachmentTypeName(attType)} ${attachmentCounts[attType]}`;
                    
                    items.push({
                        id: `msg-attachment-${message.id}-${attachmentCounts[attType]}`,
                        type: 'user_file',
                        name: displayName,
                        details: getAttachmentTypeName(attType),
                        source: 'user',
                        attachmentData: {
                            type: attType,
                            content: attachment.attachment_content,
                            url: attachment.attachment_url,
                        },
                    });
                }
            }
        }

        // Add user provided files (pending to be sent)
        if (userFiles && userFiles.length > 0) {
            for (const file of userFiles) {
                items.push({
                    id: `user-file-${file.id}`,
                    type: 'user_file',
                    name: file.name,
                    details: file.path,
                    source: 'user',
                });
            }
        }

        // Add MCP tool calls that represent context operations
        mcpToolCallStates.forEach((toolCall, callId) => {
            // Skip failed or still executing calls
            if (toolCall.status !== 'success') return;

            const toolName = toolCall.tool_name?.toLowerCase() || '';
            
            // Check if this is a context-related tool
            const isContextTool = CONTEXT_TOOL_NAMES.some(name => 
                toolName.includes(name.toLowerCase())
            );

            if (!isContextTool) return;

            // Parse parameters to get meaningful details
            let details = '';
            try {
                if (toolCall.parameters) {
                    const params = JSON.parse(toolCall.parameters);
                    // Common parameter names for file paths and search queries
                    details = params.path || params.file_path || params.query || 
                              params.pattern || params.directory || params.uri || '';
                }
            } catch {
                // Ignore parse errors
            }

            const contextType = getContextTypeFromToolName(toolName);
            const displayName = details || toolCall.tool_name || 'Unknown';
            const searchResults =
                contextType === 'search' && toolCall.result ? parseSearchResultItems(toolCall.result) : undefined;

            items.push({
                id: `mcp-${callId}`,
                type: contextType,
                name: displayName.length > 50 ? displayName.slice(0, 47) + '...' : displayName,
                details: toolCall.tool_name,
                searchResults,
                source: 'mcp',
                timestamp: new Date(),
            });
        });

        return items;
    }, [userFiles, mcpToolCallStates, messages]);

    return { contextItems };
}
