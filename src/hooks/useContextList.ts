import { useMemo } from 'react';
import { FileInfo, MCPToolCallUpdateEvent } from '../data/Conversation';
import { ContextItem, CONTEXT_TOOL_NAMES, SearchResultItem, getContextTypeFromToolName } from '../components/chat-sidebar/types';

interface UseContextListOptions {
    // User provided files from input area
    userFiles: FileInfo[] | null;
    // MCP tool call states
    mcpToolCallStates: Map<number, MCPToolCallUpdateEvent>;
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

export function useContextList({ userFiles, mcpToolCallStates }: UseContextListOptions): UseContextListReturn {
    const contextItems = useMemo(() => {
        const items: ContextItem[] = [];

        // Add user provided files
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
    }, [userFiles, mcpToolCallStates]);

    return { contextItems };
}
