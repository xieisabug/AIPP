import { renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import { clearAllMockHandlers, mockInvokeHandler } from '@/__tests__/mocks/tauri';
import { MCPToolCallUpdateEvent, Message } from '@/data/Conversation';
import { useContextList } from './useContextList';

describe('useContextList skill attachments', () => {
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

        // Wait for the loaded-tool effect to settle so React doesn't warn about async state updates.
        await waitFor(() => {
            expect(result.current.contextItems).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: 'skill',
                        name: 'skill-creator',
                        details: 'agents:skill-creator',
                        source: 'user',
                    }),
                ]),
            );
        });
    });

    it('deduplicates repeated file, directory, and search context items', async () => {
        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [
                1,
                {
                    call_id: 1,
                    conversation_id: 1,
                    status: 'success',
                    tool_name: 'read_file',
                    parameters: JSON.stringify({ path: '/workspace/notes/todo.md' }),
                },
            ],
            [
                2,
                {
                    call_id: 2,
                    conversation_id: 1,
                    status: 'success',
                    tool_name: 'get_file_contents',
                    parameters: JSON.stringify({ file_path: '\\workspace\\notes\\todo.md' }),
                },
            ],
            [
                3,
                {
                    call_id: 3,
                    conversation_id: 1,
                    status: 'success',
                    tool_name: 'list_directory',
                    parameters: JSON.stringify({ directory: '/workspace/project/' }),
                },
            ],
            [
                4,
                {
                    call_id: 4,
                    conversation_id: 1,
                    status: 'success',
                    tool_name: 'search',
                    parameters: JSON.stringify({ query: 'fix duplicate context' }),
                },
            ],
            [
                5,
                {
                    call_id: 5,
                    conversation_id: 1,
                    status: 'success',
                    tool_name: 'search_files',
                    parameters: JSON.stringify({ query: 'fix duplicate context' }),
                },
            ],
        ]);

        const { result } = renderHook(() =>
            useContextList({
                conversationId: 1,
                userFiles: null,
                mcpToolCallStates,
                messages: [],
                acpWorkingDirectory: '\\workspace\\project\\',
            }),
        );

        await waitFor(() => {
            const readFiles = result.current.contextItems.filter(
                (item) => item.type === 'read_file',
            );
            const directories = result.current.contextItems.filter(
                (item) => item.type === 'list_directory',
            );
            const searches = result.current.contextItems.filter(
                (item) => item.type === 'search',
            );

            expect(readFiles).toHaveLength(1);
            expect(readFiles[0]).toEqual(
                expect.objectContaining({
                    name: '/workspace/notes/todo.md',
                }),
            );

            expect(directories).toHaveLength(1);
            // The UI keeps the original display value from the first item; only the dedupe key is normalized.
            expect(directories[0]).toEqual(
                expect.objectContaining({
                    name: '\\workspace\\project\\',
                    details: 'ACP 工作目录',
                }),
            );

            expect(searches).toHaveLength(1);
            expect(searches[0]).toEqual(
                expect.objectContaining({
                    name: 'fix duplicate context',
                }),
            );
        });
    });
});
