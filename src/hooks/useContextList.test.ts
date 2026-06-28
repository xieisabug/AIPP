import { renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import { clearAllMockHandlers, mockInvokeHandler } from '@/__tests__/mocks/tauri';
import { MCPToolCallUpdateEvent, Message } from '@/data/Conversation';
import { useContextList } from './useContextList';

describe('useContextList', () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler('get_conversation_loaded_mcp_tools', () => []);
    });

    it('maps skill attachments into sidebar skill context items', async () => {
        const messages: Message[] = [
            {
                id: 1,
                conversation_id: 1,
                message_type: 'user',
                content: '我想创建一个帮我炒股的skill',
                llm_model_id: null,
                created_time: new Date(),
                start_time: null,
                finish_time: null,
                token_count: 0,
                input_token_count: 0,
                output_token_count: 0,
                regenerate: null,
                attachment_list: [
                    {
                        attachment_type: 'Skill',
                        attachment_url: 'skill-creator',
                        attachment_content: JSON.stringify({
                            displayName: 'skill-creator',
                            identifier: 'agents:skill-creator',
                            content: '# Skill Creator',
                        }),
                    },
                ],
            },
        ];

        const { result } = renderHook(() =>
            useContextList({
                conversationId: 1,
                userFiles: null,
                mcpToolCallStates: new Map(),
                messages,
                acpWorkingDirectory: null,
            }),
        );

        await waitFor(() => {
            expect(result.current.contextItems).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: 'skill',
                        name: 'skill-creator',
                        details: 'agents:skill-creator',
                        source: 'user',
                        previewStatus: 'needs_load',
                        previewData: expect.objectContaining({
                            rawValue: 'agents:skill-creator',
                            subtitle: 'agents:skill-creator',
                        }),
                    }),
                ]),
            );
        });
    });

    it('includes response image attachments in sidebar context items', async () => {
        const messages: Message[] = [
            {
                id: 2,
                conversation_id: 1,
                message_type: 'response',
                content: '这里是生成的图片',
                llm_model_id: null,
                created_time: new Date(),
                start_time: null,
                finish_time: null,
                token_count: 0,
                input_token_count: 0,
                output_token_count: 0,
                regenerate: null,
                attachment_list: [
                    {
                        attachment_type: 'Image',
                        attachment_url: 'generated-image-1.png',
                        attachment_content: 'data:image/png;base64,Zm9v',
                    },
                ],
            },
        ];

        const { result } = renderHook(() =>
            useContextList({
                conversationId: 1,
                userFiles: null,
                mcpToolCallStates: new Map(),
                messages,
                acpWorkingDirectory: null,
            }),
        );

        await waitFor(() => {
            expect(result.current.contextItems).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: 'generated_image',
                        source: 'assistant',
                        details: 'generated-image-1.png',
                        attachmentData: expect.objectContaining({
                            type: 'Image',
                            content: 'data:image/png;base64,Zm9v',
                            url: 'generated-image-1.png',
                        }),
                        previewData: expect.objectContaining({
                            contentType: 'image',
                            content: 'data:image/png;base64,Zm9v',
                            url: 'generated-image-1.png',
                            metadata: expect.objectContaining({
                                来源: '回复附件',
                                类型: '图片',
                            }),
                        }),
                    }),
                ]),
            );
        });
    });

    it('deduplicates identical file and directory context items while keeping repeated searches', async () => {
        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [1, {
                call_id: 1,
                conversation_id: 1,
                status: 'success',
                tool_name: 'read_file',
                parameters: JSON.stringify({ path: '/workspace/src/App.tsx' }),
            }],
            [2, {
                call_id: 2,
                conversation_id: 1,
                status: 'success',
                tool_name: 'read_file',
                parameters: JSON.stringify({ path: '/workspace/src/App.tsx' }),
            }],
            [3, {
                call_id: 3,
                conversation_id: 1,
                status: 'success',
                tool_name: 'list_directory',
                parameters: JSON.stringify({ path: '/workspace/src/components' }),
            }],
            [4, {
                call_id: 4,
                conversation_id: 1,
                status: 'success',
                tool_name: 'list_directory',
                parameters: JSON.stringify({ path: '/workspace/src/components' }),
            }],
            [5, {
                call_id: 5,
                conversation_id: 1,
                status: 'success',
                tool_name: 'search_files',
                parameters: JSON.stringify({ query: 'Button' }),
            }],
            [6, {
                call_id: 6,
                conversation_id: 1,
                status: 'success',
                tool_name: 'search_files',
                parameters: JSON.stringify({ query: 'Button' }),
            }],
        ]);

        const { result } = renderHook(() =>
            useContextList({
                conversationId: 1,
                userFiles: null,
                mcpToolCallStates,
                messages: [],
                acpWorkingDirectory: null,
            }),
        );

        await waitFor(() => {
            expect(
                result.current.contextItems.filter(
                    (item) => item.type === 'read_file' && item.name === '/workspace/src/App.tsx',
                ),
            ).toHaveLength(1);
            expect(
                result.current.contextItems.filter(
                    (item) => item.type === 'list_directory' && item.name === '/workspace/src/components',
                ),
            ).toHaveLength(1);
            expect(
                result.current.contextItems.filter(
                    (item) => item.type === 'search' && item.name === 'Button',
                ),
            ).toHaveLength(2);
        });
    });

    it('uses Chinese labels for built-in web search and fetch sidebar items', async () => {
        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [7, {
                call_id: 7,
                conversation_id: 1,
                status: 'success',
                tool_name: 'search_web',
                parameters: JSON.stringify({ query: 'Rust async', result_type: 'items' }),
                result: JSON.stringify([
                    {
                        type: 'json',
                        json: [
                            {
                                title: 'Rust Async',
                                url: 'https://example.com/rust',
                                snippet: 'Rust async result',
                            },
                        ],
                    },
                ]),
            }],
            [8, {
                call_id: 8,
                conversation_id: 1,
                status: 'success',
                tool_name: 'fetch_url',
                parameters: JSON.stringify({ url: 'https://example.com/article' }),
                result: JSON.stringify([{ type: 'text', text: 'Hello world' }]),
            }],
        ]);

        const { result } = renderHook(() =>
            useContextList({
                conversationId: 1,
                userFiles: null,
                mcpToolCallStates,
                messages: [],
                acpWorkingDirectory: null,
            }),
        );

        await waitFor(() => {
            expect(result.current.contextItems).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        id: 'mcp-7',
                        type: 'search',
                        name: 'Rust async',
                        details: '网络搜索',
                        previewData: expect.objectContaining({
                            subtitle: '网络搜索',
                            metadata: expect.objectContaining({
                                工具: '网络搜索',
                            }),
                        }),
                    }),
                    expect.objectContaining({
                        id: 'mcp-8',
                        type: 'search',
                        name: 'https://example.com/article',
                        details: '抓取网页',
                        previewData: expect.objectContaining({
                            subtitle: '抓取网页',
                            metadata: expect.objectContaining({
                                工具: '抓取网页',
                            }),
                        }),
                    }),
                ]),
            );
        });
    });

    it('keeps full raw values in context item names for hover and dedupe', async () => {
        const longPath = '/workspace/src/components/chat-sidebar/some/really/long/path/that/should/not/be/truncated/in/context-item-data/ContextList.tsx';
        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [1, {
                call_id: 1,
                conversation_id: 1,
                status: 'success',
                tool_name: 'read_file',
                parameters: JSON.stringify({ path: longPath }),
            }],
        ]);

        const { result } = renderHook(() =>
            useContextList({
                conversationId: 1,
                userFiles: null,
                mcpToolCallStates,
                messages: [],
                acpWorkingDirectory: null,
            }),
        );

        await waitFor(() => {
            expect(
                result.current.contextItems.find((item) => item.type === 'read_file')?.name,
            ).toBe(longPath);
            expect(
                result.current.contextItems.find((item) => item.type === 'read_file')?.previewData,
            ).toEqual(
                expect.objectContaining({
                    rawValue: longPath,
                    path: longPath,
                }),
            );
        });
    });

    it('builds preview data for read_file tool results', async () => {
        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [1, {
                call_id: 1,
                conversation_id: 1,
                status: 'success',
                tool_name: 'read_file',
                parameters: JSON.stringify({ path: '/workspace/src/App.tsx' }),
                result: 'export const App = () => null;',
            }],
        ]);

        const { result } = renderHook(() =>
            useContextList({
                conversationId: 1,
                userFiles: null,
                mcpToolCallStates,
                messages: [],
                acpWorkingDirectory: null,
            }),
        );

        await waitFor(() => {
            expect(
                result.current.contextItems.find((item) => item.type === 'read_file')?.previewData,
            ).toEqual(
                expect.objectContaining({
                    contentType: 'code',
                    content: 'export const App = () => null;',
                    language: 'typescript',
                    path: '/workspace/src/App.tsx',
                }),
            );
        });
    });

    it('builds preview_file context item from file URLs using the directory label', async () => {
        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [12, {
                call_id: 12,
                conversation_id: 1,
                message_id: 20,
                status: 'success',
                tool_name: 'preview_file',
                parameters: JSON.stringify({
                    files: [
                        {
                            title: 'App.tsx',
                            type: 'text',
                            url: '/workspace/src/App.tsx',
                            language: 'typescript',
                        },
                    ],
                    viewMode: 'tabs',
                }),
                result: JSON.stringify({
                    content: [{ type: 'json', json: { status: 'preview_shown', request_id: 'req-1' } }],
                }),
            }],
        ]);

        const { result } = renderHook(() =>
            useContextList({
                conversationId: 1,
                userFiles: null,
                mcpToolCallStates,
                messages: [],
                acpWorkingDirectory: null,
            }),
        );

        await waitFor(() => {
            expect(
                result.current.contextItems.find((item) => item.type === 'preview_file'),
            ).toEqual(
                expect.objectContaining({
                    name: '/workspace/src',
                    details: 'App.tsx',
                    previewFileData: {
                        callId: 12,
                        conversationId: 1,
                        messageId: 20,
                        requestId: 'req-1',
                    },
                    previewData: expect.objectContaining({
                        path: '/workspace/src',
                        metadata: expect.objectContaining({
                            目录: '/workspace/src',
                            文件: 'App.tsx',
                        }),
                    }),
                }),
            );
        });
    });

    it('builds preview_file context item from inline content using the title label', async () => {
        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [13, {
                call_id: 13,
                conversation_id: 1,
                status: 'success',
                tool_name: 'preview_file',
                parameters: JSON.stringify({
                    files: [
                        {
                            title: '分析结果',
                            type: 'markdown',
                            content: '# 分析结果',
                        },
                    ],
                }),
            }],
        ]);

        const { result } = renderHook(() =>
            useContextList({
                conversationId: 1,
                userFiles: null,
                mcpToolCallStates,
                messages: [],
                acpWorkingDirectory: null,
            }),
        );

        await waitFor(() => {
            expect(
                result.current.contextItems.find((item) => item.type === 'preview_file'),
            ).toEqual(
                expect.objectContaining({
                    name: '分析结果',
                    details: '分析结果',
                    previewData: expect.objectContaining({
                        items: [
                            expect.objectContaining({
                                label: '分析结果',
                                value: '6 字符内容',
                            }),
                        ],
                    }),
                }),
            );
        });
    });

    it('keeps recursive directory hierarchy in list_directory preview content', async () => {
        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [1, {
                call_id: 1,
                conversation_id: 1,
                status: 'success',
                tool_name: 'list_directory',
                parameters: JSON.stringify({ path: '/workspace/src', recursive: true }),
                result: JSON.stringify({
                    content: [{ type: 'text', text: 'a/\na/b/\na/b/c/\na/b/c/d.md' }],
                    isError: false,
                    metadata: {
                        path: '/workspace/src',
                        total_count: 4,
                    },
                }),
            }],
        ]);

        const { result } = renderHook(() =>
            useContextList({
                conversationId: 1,
                userFiles: null,
                mcpToolCallStates,
                messages: [],
                acpWorkingDirectory: null,
            }),
        );

        await waitFor(() => {
            expect(
                result.current.contextItems.find((item) => item.type === 'list_directory')?.previewData,
            ).toEqual(
                expect.objectContaining({
                    contentType: 'directory',
                    content: 'a/\na/b/\na/b/c/\na/b/c/d.md',
                    rawValue: '/workspace/src',
                }),
            );
        });
    });
});
