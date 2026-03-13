import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { FileInfo, MCPToolCallUpdateEvent, Message } from '../data/Conversation';
import {
    ContextItem,
    ContextPreviewData,
    ContextPreviewListItem,
    CONTEXT_TOOL_NAMES,
    SearchResultItem,
    getContextTypeFromToolName,
} from '../components/chat-sidebar/types';

interface Attachment {
    attachment_url?: string;
    attachment_content?: string;
    attachment_type?: string;
}

interface SkillAttachmentPayload {
    displayName?: string;
    identifier?: string;
}

const DEDUPED_CONTEXT_TYPES = new Set<ContextItem['type']>(['read_file', 'list_directory']);

interface UseContextListOptions {
    conversationId?: string | number;
    // User provided files from input area (pending to be sent)
    userFiles: FileInfo[] | null;
    // MCP tool call states
    mcpToolCallStates: Map<number, MCPToolCallUpdateEvent>;
    // Messages in conversation (to extract sent attachments)
    messages?: Message[];
    // ACP assistant working directory (resolved by backend)
    acpWorkingDirectory?: string | null;
}

interface UseContextListReturn {
    contextItems: ContextItem[];
}

interface LoadedMcpToolItem {
    id: number;
    conversation_id: number;
    tool_id: number;
    loaded_server_name: string;
    loaded_tool_name: string;
    status: string;
    invalid_reason?: string | null;
    tool_description?: string | null;
    parameters?: string | null;
}

interface ParsedToolResult {
    text?: string;
    json?: unknown;
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

const parseSearchResultText = (rawResult: string): string | undefined => {
    if (!rawResult) return undefined;
    try {
        const parsed = JSON.parse(rawResult);
        const parts = Array.isArray(parsed) ? parsed : parsed?.content;
        if (!Array.isArray(parts)) return undefined;
        const textParts = parts
            .map((part: any) => (part?.type === 'text' && typeof part?.text === 'string' ? part.text : ''))
            .filter((text: string) => text);
        if (textParts.length === 0) return undefined;
        return textParts.join('\n');
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
        case 'Skill':
            return '技能';
        default:
            return '文件';
    }
};

const parseSkillAttachmentPayload = (
    attachmentContent?: string,
): SkillAttachmentPayload | null => {
    if (!attachmentContent) {
        return null;
    }

    try {
        const parsed = JSON.parse(attachmentContent);
        if (!parsed || typeof parsed !== 'object') {
            return null;
        }

        return {
            displayName:
                typeof parsed.displayName === 'string' ? parsed.displayName : undefined,
            identifier:
                typeof parsed.identifier === 'string' ? parsed.identifier : undefined,
        };
    } catch {
        return null;
    }
};

const normalizeContextRawValue = (value?: string): string | undefined => {
    const normalizedValue = value?.trim();
    return normalizedValue ? normalizedValue : undefined;
};

const buildMetadata = (entries: Record<string, string | undefined>): Record<string, string> | undefined => {
    const metadataEntries = Object.entries(entries).filter(
        (entry): entry is [string, string] => typeof entry[1] === 'string' && entry[1].trim().length > 0,
    );
    return metadataEntries.length > 0 ? Object.fromEntries(metadataEntries) : undefined;
};

const stringifyJsonValue = (value: unknown): string | undefined => {
    if (value === undefined) return undefined;
    if (typeof value === 'string') return value;
    try {
        return JSON.stringify(value, null, 2);
    } catch {
        return String(value);
    }
};

const parseToolResultPayload = (rawResult?: string): ParsedToolResult => {
    if (!rawResult) return {};

    try {
        const parsed = JSON.parse(rawResult);
        const parts = Array.isArray(parsed) ? parsed : parsed?.content;

        if (Array.isArray(parts)) {
            const textParts = parts
                .map((part: any) => (part?.type === 'text' && typeof part?.text === 'string' ? part.text : ''))
                .filter((text: string) => text.length > 0);
            const jsonPart = parts.find((part: any) => part?.type === 'json' && part?.json !== undefined);

            return {
                text: textParts.length > 0 ? textParts.join('\n') : undefined,
                json: jsonPart?.json,
            };
        }

        if (typeof parsed === 'string') {
            return { text: parsed };
        }

        return {
            text: typeof parsed?.text === 'string' ? parsed.text : undefined,
            json: parsed,
        };
    } catch {
        return { text: rawResult };
    }
};

const buildPreviewItems = (value: unknown): ContextPreviewListItem[] | undefined => {
    const itemsSource = Array.isArray(value)
        ? value
        : Array.isArray((value as { items?: unknown[] } | undefined)?.items)
            ? (value as { items: unknown[] }).items
            : undefined;

    if (!itemsSource || itemsSource.length === 0) {
        return undefined;
    }

    const items = itemsSource
        .map((item): ContextPreviewListItem | null => {
            if (typeof item === 'string' || typeof item === 'number' || typeof item === 'boolean') {
                return { label: String(item) };
            }

            if (!item || typeof item !== 'object') {
                return null;
            }

            const candidate = item as Record<string, unknown>;
            const label =
                (typeof candidate.name === 'string' && candidate.name) ||
                (typeof candidate.title === 'string' && candidate.title) ||
                (typeof candidate.label === 'string' && candidate.label) ||
                (typeof candidate.path === 'string' && candidate.path) ||
                (typeof candidate.url === 'string' && candidate.url) ||
                stringifyJsonValue(candidate);

            if (!label) {
                return null;
            }

            return {
                label,
                value:
                    (typeof candidate.value === 'string' && candidate.value) ||
                    (typeof candidate.path === 'string' && candidate.path !== label ? candidate.path : undefined),
                description:
                    (typeof candidate.type === 'string' && candidate.type) ||
                    (typeof candidate.kind === 'string' && candidate.kind) ||
                    (typeof candidate.snippet === 'string' && candidate.snippet) ||
                    (typeof candidate.display_url === 'string' && candidate.display_url) ||
                    undefined,
                url: typeof candidate.url === 'string' ? candidate.url : undefined,
            };
        })
        .filter((item): item is ContextPreviewListItem => item !== null);

    return items.length > 0 ? items : undefined;
};

const inferLanguageFromPath = (path?: string): string | undefined => {
    const ext = path?.split('.').pop()?.toLowerCase();
    switch (ext) {
        case 'ts':
        case 'tsx':
            return 'typescript';
        case 'js':
        case 'jsx':
            return 'javascript';
        case 'rs':
            return 'rust';
        case 'py':
            return 'python';
        case 'go':
            return 'go';
        case 'json':
            return 'json';
        case 'md':
        case 'markdown':
            return 'markdown';
        case 'html':
            return 'html';
        case 'css':
            return 'css';
        case 'sql':
            return 'sql';
        case 'yaml':
        case 'yml':
            return 'yaml';
        default:
            return undefined;
    }
};

const inferContentType = (
    pathOrQuery: string | undefined,
    resultType: string | undefined,
    parsedResult: ParsedToolResult,
): Pick<ContextPreviewData, 'contentType' | 'language' | 'content'> => {
    const explicitType = resultType?.toLowerCase();
    const language = inferLanguageFromPath(pathOrQuery);
    const content = parsedResult.text ?? stringifyJsonValue(parsedResult.json);

    if (!content) {
        return { contentType: undefined, language };
    }

    if (explicitType === 'markdown' || language === 'markdown') {
        return { contentType: 'markdown', content, language: 'markdown' };
    }

    if (explicitType === 'json' || language === 'json') {
        return { contentType: 'json', content, language: 'json' };
    }

    if (language && language !== 'markdown' && language !== 'json') {
        return { contentType: 'code', content, language };
    }

    return { contentType: 'text', content, language };
};

export function useContextList({
    conversationId,
    userFiles,
    mcpToolCallStates,
    messages,
    acpWorkingDirectory,
}: UseContextListOptions): UseContextListReturn {
    const [loadedMcpTools, setLoadedMcpTools] = useState<LoadedMcpToolItem[]>([]);

    useEffect(() => {
        const cid = Number(conversationId);
        if (!conversationId || Number.isNaN(cid)) {
            setLoadedMcpTools([]);
            return;
        }
        invoke<LoadedMcpToolItem[]>("get_conversation_loaded_mcp_tools", {
            conversationId: cid,
        })
            .then((tools) => setLoadedMcpTools(tools))
            .catch((error) => {
                console.warn("Failed to fetch loaded MCP tools", error);
                setLoadedMcpTools([]);
            });
    }, [conversationId, mcpToolCallStates]);

    const contextItems = useMemo(() => {
        const items: ContextItem[] = [];
        const dedupeKeys = new Set<string>();

        const pushContextItem = (item: ContextItem, rawValue?: string) => {
            if (!DEDUPED_CONTEXT_TYPES.has(item.type)) {
                items.push(item);
                return;
            }

            const normalizedRawValue = normalizeContextRawValue(rawValue ?? item.details ?? item.name);
            if (!normalizedRawValue) {
                items.push(item);
                return;
            }

            const dedupeKey = `${item.type}:${normalizedRawValue}`;
            if (dedupeKeys.has(dedupeKey)) {
                return;
            }

            dedupeKeys.add(dedupeKey);
            items.push(item);
        };

        if (acpWorkingDirectory) {
            pushContextItem({
                id: `acp-working-directory-${acpWorkingDirectory}`,
                type: 'list_directory',
                name: acpWorkingDirectory,
                details: 'ACP 工作目录',
                source: 'mcp',
            }, acpWorkingDirectory);
        }

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

                    if (attType === 'Skill') {
                        const skillPayload = parseSkillAttachmentPayload(
                            attachment.attachment_content,
                        );
                        const skillName =
                            skillPayload?.displayName?.trim() ||
                            attachment.attachment_url ||
                            `技能 ${attachmentCounts[attType]}`;
                        const skillIdentifier = skillPayload?.identifier?.trim();

                        items.push({
                            id: `msg-skill-${message.id}-${attachmentCounts[attType]}`,
                            type: 'skill',
                            name: skillName,
                            details: skillIdentifier,
                            source: 'user',
                            previewStatus: 'needs_load',
                            previewData: {
                                title: skillName,
                                subtitle: skillIdentifier,
                                rawValue: skillIdentifier || attachment.attachment_url || skillName,
                                contentType: 'file-meta',
                                metadata: buildMetadata({
                                    来源: '用户附件',
                                    标识符: skillIdentifier || attachment.attachment_url,
                                }),
                            },
                        });
                        continue;
                    }

                    const displayName = `${getAttachmentTypeName(attType)} ${attachmentCounts[attType]}`;
                    const fileTypeName = getAttachmentTypeName(attType);

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
                        previewStatus: 'ready',
                        previewData: {
                            title: displayName,
                            subtitle: attachment.attachment_url || fileTypeName,
                            rawValue: attachment.attachment_url || displayName,
                            contentType:
                                attType === 'Image'
                                    ? 'image'
                                    : attType === 'Text'
                                        ? 'text'
                                        : 'file-meta',
                            content: attachment.attachment_content,
                            path: attachment.attachment_url,
                            url: attachment.attachment_url,
                            metadata: buildMetadata({
                                来源: '用户附件',
                                类型: fileTypeName,
                                路径: attachment.attachment_url,
                            }),
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
                    previewStatus: 'ready',
                    previewData: {
                        title: file.name,
                        subtitle: file.path,
                        rawValue: file.path || file.name,
                        contentType: 'file-meta',
                        path: file.path,
                        metadata: buildMetadata({
                            来源: '待发送文件',
                            路径: file.path,
                        }),
                    },
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
            let rawValue = '';
            let resultType: string | undefined;
            try {
                if (toolCall.parameters) {
                    const params = JSON.parse(toolCall.parameters);
                    // Common parameter names for file paths and search queries
                    rawValue = params.path || params.file_path || params.query ||
                        params.pattern || params.directory || params.uri || params.url || '';
                    resultType = typeof params.result_type === 'string' ? params.result_type : undefined;
                }
            } catch {
                // Ignore parse errors
            }

            const contextType = getContextTypeFromToolName(toolName);
            const displayName = rawValue || toolCall.tool_name || 'Unknown';
            const parsedResult = parseToolResultPayload(toolCall.result);
            const searchResults =
                contextType === 'search' && toolCall.result ? parseSearchResultItems(toolCall.result) : undefined;
            const searchMarkdown =
                contextType === 'search' &&
                toolCall.result &&
                (!resultType || resultType === 'markdown') &&
                !searchResults
                    ? parseSearchResultText(toolCall.result)
                    : undefined;
            const inferredContent = inferContentType(rawValue, resultType, parsedResult);
            const previewItems = contextType === 'list_directory' ? buildPreviewItems(parsedResult.json) : undefined;
            const previewData: ContextPreviewData =
                contextType === 'search'
                    ? {
                        title: displayName,
                        subtitle: toolCall.tool_name,
                        rawValue: rawValue || displayName,
                        contentType: searchMarkdown ? 'markdown' : 'file-meta',
                        content: searchMarkdown,
                        items: searchResults?.map((result) => ({
                            label: result.title,
                            description: result.snippet || result.displayUrl,
                            url: result.url,
                        })),
                        metadata: buildMetadata({
                            来源: 'MCP 搜索',
                            工具: toolCall.tool_name,
                            查询: rawValue,
                        }),
                    }
                    : contextType === 'list_directory'
                        ? {
                            title: displayName,
                            subtitle: toolCall.tool_name,
                            rawValue: rawValue || displayName,
                            contentType: 'directory',
                            content: !previewItems ? inferredContent.content : undefined,
                            path: rawValue || undefined,
                            items: previewItems,
                            metadata: buildMetadata({
                                来源: 'MCP 目录',
                                工具: toolCall.tool_name,
                                路径: rawValue,
                                状态: !previewItems && !inferredContent.content ? '未缓存目录内容' : undefined,
                            }),
                        }
                        : {
                            title: displayName,
                            subtitle: toolCall.tool_name,
                            rawValue: rawValue || displayName,
                            contentType: inferredContent.contentType || 'file-meta',
                            content: inferredContent.content,
                            language: inferredContent.language,
                            path: rawValue || undefined,
                            metadata: buildMetadata({
                                来源: contextType === 'read_file' ? 'MCP 文件读取' : 'MCP',
                                工具: toolCall.tool_name,
                                路径: rawValue,
                                状态:
                                    contextType === 'read_file' && !inferredContent.content
                                        ? '未缓存文件内容'
                                        : undefined,
                            }),
                        };

            pushContextItem({
                id: `mcp-${callId}`,
                type: contextType,
                name: displayName,
                details: toolCall.tool_name,
                searchResults,
                searchMarkdown,
                source: 'mcp',
                timestamp: new Date(),
                previewStatus: 'ready',
                previewData,
            }, rawValue);
        });

        loadedMcpTools.forEach((tool) => {
            const statusLabel = tool.status === "valid" ? "已加载" : `失效: ${tool.status}`;
            const reason = tool.invalid_reason ? ` (${tool.invalid_reason})` : "";
            const parsedParameters = (() => {
                if (!tool.parameters) return undefined;
                try {
                    return JSON.parse(tool.parameters);
                } catch {
                    return tool.parameters;
                }
            })();
            items.push({
                id: `loaded-mcp-tool-${tool.id}`,
                type: "loaded_mcp_tool",
                name: `${tool.loaded_server_name}::${tool.loaded_tool_name}`,
                details: `${statusLabel}${reason}`,
                source: "mcp",
                timestamp: new Date(),
                previewStatus: 'ready',
                previewData: {
                    title: `${tool.loaded_server_name}::${tool.loaded_tool_name}`,
                    subtitle: `${statusLabel}${reason}`,
                    rawValue: `${tool.loaded_server_name}::${tool.loaded_tool_name}`,
                    contentType: 'json',
                    content: stringifyJsonValue(parsedParameters),
                    items: tool.tool_description
                        ? [{ label: '介绍', value: tool.tool_description }]
                        : undefined,
                    metadata: buildMetadata({
                        来源: 'MCP 已加载工具',
                        服务: tool.loaded_server_name,
                        工具: tool.loaded_tool_name,
                        状态: tool.status,
                        失效原因: tool.invalid_reason ?? undefined,
                    }),
                },
                loadedToolData: {
                    loadedToolId: tool.id,
                    toolId: tool.tool_id,
                    serverName: tool.loaded_server_name,
                    toolName: tool.loaded_tool_name,
                    status: tool.status,
                    invalidReason: tool.invalid_reason ?? undefined,
                    description: tool.tool_description ?? undefined,
                    parameters: tool.parameters ?? undefined,
                },
            });
        });

        return items;
    }, [userFiles, mcpToolCallStates, messages, acpWorkingDirectory, loadedMcpTools]);

    return { contextItems };
}
