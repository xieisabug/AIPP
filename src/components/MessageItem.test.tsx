import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import MessageItem from './MessageItem';
import type { Message } from '@/data/Conversation';

vi.mock('./UnifiedMarkdown', () => ({
    default: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

vi.mock('./ReasoningMessage', () => ({
    default: () => <div>reasoning</div>,
}));

vi.mock('./message-item/ErrorMessage', () => ({
    default: ({ content }: { content: string }) => <div>{content}</div>,
}));

vi.mock('./message-item/MessageActionButtons', () => ({
    default: () => null,
}));

vi.mock('./message-item/ImageAttachments', () => ({
    default: ({ attachments }: { attachments?: Array<{ attachment_url?: string }> }) => (
        <div data-testid="image-attachments">
            {(attachments ?? []).map((attachment) => attachment.attachment_url).join(',')}
        </div>
    ),
}));

vi.mock('./RawTextRenderer', () => ({
    default: ({ content }: { content: string }) => <div>{content}</div>,
}));

vi.mock('./magicui/shine-border', () => ({
    ShineBorder: () => null,
}));

vi.mock('@/hooks/useCopyHandler', () => ({
    useCopyHandler: () => ({
        copyIconState: 'copy',
        handleCopy: vi.fn(),
    }),
}));

vi.mock('@/hooks/useCustomTagParser', () => ({
    useCustomTagParser: () => ({
        parseCustomTags: (content: string) => content,
    }),
}));

vi.mock('@/hooks/useMarkdownConfig', () => ({
    useMarkdownConfig: () => ({}),
}));

vi.mock('@/hooks/useMcpToolCallProcessor', () => ({
    useMcpToolCallProcessor: () => ({
        processContent: (_content: string, element: React.ReactNode) => element,
    }),
}));

vi.mock('@/hooks/useDisplayConfig', () => ({
    useDisplayConfig: () => ({
        isUserMessageMarkdownEnabled: true,
        isShowThinking: true,
    }),
}));

vi.mock('@/hooks/useFeishuDebugResend', () => ({
    useFeishuDebugResend: () => ({
        pendingMessageId: null,
        resendMessageToFeishuDebug: vi.fn(),
    }),
}));

vi.mock('@/contexts/AntiLeakageContext', () => ({
    useAntiLeakage: () => ({
        enabled: false,
        isRevealed: true,
    }),
}));

function createMessage(overrides: Partial<Message> = {}): Message {
    return {
        id: 1,
        conversation_id: 1,
        message_type: 'response',
        content: '',
        llm_model_id: null,
        created_time: new Date('2026-04-24T00:00:00Z'),
        start_time: new Date('2026-04-24T00:00:00Z'),
        finish_time: new Date('2026-04-24T00:00:01Z'),
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        regenerate: null,
        attachment_list: [],
        ...overrides,
    };
}

describe('MessageItem attachment updates', () => {
    it('re-renders when the same message receives image attachments later', () => {
        const { rerender } = render(<MessageItem message={createMessage()} />);

        expect(screen.getByTestId('image-attachments')).toHaveTextContent('');

        rerender(
            <MessageItem
                message={createMessage({
                    attachment_list: [
                        {
                            id: 10,
                            attachment_type: 'Image',
                            attachment_url: 'generated-image-1.png',
                            attachment_content: 'data:image/png;base64,Zm9v',
                        },
                    ],
                })}
            />
        );

        expect(screen.getByTestId('image-attachments')).toHaveTextContent(
            'generated-image-1.png'
        );
    });
});