import { useMemo } from 'react';
import { FileInfo, MCPToolCallUpdateEvent } from '../data/Conversation';
import { ContextItem, CONTEXT_TOOL_NAMES, getContextTypeFromToolName } from '../components/chat-sidebar/types';

interface UseContextListOptions {
    // User provided files from input area
    userFiles: FileInfo[] | null;
    // MCP tool call states
    mcpToolCallStates: Map<number, MCPToolCallUpdateEvent>;
}

interface UseContextListReturn {
    contextItems: ContextItem[];
}

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

            items.push({
                id: `mcp-${callId}`,
                type: contextType,
                name: displayName.length > 50 ? displayName.slice(0, 47) + '...' : displayName,
                details: toolCall.tool_name,
                source: 'mcp',
                timestamp: new Date(),
            });
        });

        return items;
    }, [userFiles, mcpToolCallStates]);

    return { contextItems };
}
