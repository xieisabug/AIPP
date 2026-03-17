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
});
