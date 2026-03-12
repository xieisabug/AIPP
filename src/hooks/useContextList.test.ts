import { renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import { clearAllMockHandlers, mockInvokeHandler } from '@/__tests__/mocks/tauri';
import { Message } from '@/data/Conversation';
import { useContextList } from './useContextList';

describe('useContextList skill attachments', () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler('get_conversation_loaded_mcp_tools', () => []);
    });

    it('maps skill attachments into sidebar skill context items', () => {
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
