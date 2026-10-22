import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import MessageItem from './MessageItem';
import type { Message } from '@/data/Conversation';
import {
    getMessageItemRenderCount,
    resetMessageItemRenderCounts,
} from '@/utils/messageItemRenderStats';

const messageActionButtonCalls = vi.hoisted(() => [] as Array<{ messageContent?: string }>);
const antiLeakageState = vi.hoisted(() => ({
    enabled: false,
    isRevealed: true,
}));

const markdownMountCounts = vi.hoisted(() => new Map<string, number>());

vi.mock('./UnifiedMarkdown', async () => {
    const { useEffect } = await import('react');
    return {
        default: ({ children }: { children: React.ReactNode }) => {
            useEffect(() => {
                const key = String(children);
                markdownMountCounts.set(key, (markdownMountCounts.get(key) ?? 0) + 1);
            }, []);
            return <div>{children}</div>;
        },
    };
});

vi.mock('./ReasoningMessage', () => ({
    default: () => <div>reasoning</div>,
}));

vi.mock('./message-item/ErrorMessage', () => ({
    default: ({ content }: { content: string }) => <div>{content}</div>,
}));

vi.mock('./message-item/MessageActionButtons', () => ({
    default: (props: { messageContent?: string }) => {
        messageActionButtonCalls.push(props);
        return null;
    },
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

vi.mock('@/hooks/useMarkdownConfig', async () => {
    const { createContext } = await import('react');
    return {
        MarkdownStreamingContext: createContext(false),
        useMarkdownConfig: () => ({}),
    };
});

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
        enabled: antiLeakageState.enabled,
        isRevealed: antiLeakageState.isRevealed,
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

describe('MessageItem large message preview', () => {
    beforeEach(() => {
        messageActionButtonCalls.length = 0;
        antiLeakageState.enabled = false;
        antiLeakageState.isRevealed = true;
    });

    it('keeps historical plain large response fully rendered', () => {
        const largeContent = Array.from(
            { length: 260 },
            (_, index) => `large response line ${index}`,
        ).join('\n');

        render(<MessageItem message={createMessage({ content: largeContent })} />);

        expect(screen.queryByText('展开完整内容')).not.toBeInTheDocument();
        expect(screen.queryByText('收起完整内容')).not.toBeInTheDocument();
        expect(screen.getByText(/large response line 259/)).toBeInTheDocument();
    });

    it('keeps historical tool result content rendered without an outer preview gate', () => {
        const content = ['tool result line 0', 'tool result line 1'].join('\n');

        render(
            <MessageItem
                message={createMessage({
                    content,
                    message_type: 'tool_result',
                })}
            />,
        );

        expect(screen.queryByText('展开完整内容')).not.toBeInTheDocument();
        expect(screen.queryByText('收起完整内容')).not.toBeInTheDocument();
        expect(screen.getByText(/tool result line 1/)).toBeInTheDocument();
        expect(messageActionButtonCalls.at(-1)?.messageContent).toBe(content);
    });

    it('does not add an outer preview gate for MCP payload messages', () => {
        const hiddenPayload = 'x'.repeat(5200);
        const mcpContent = [
            `<!-- MCP_TOOL_CALL:${JSON.stringify({
                call_id: 1751,
                tool_name: 'write_file',
                parameters: hiddenPayload,
            })} -->`,
            'visible assistant tail',
        ].join('\n');

        render(<MessageItem message={createMessage({ content: mcpContent })} />);

        expect(screen.queryByText('展开完整内容')).not.toBeInTheDocument();
        expect(screen.queryByText('收起完整内容')).not.toBeInTheDocument();
        expect(screen.getByText((content) => content.includes(hiddenPayload))).toBeInTheDocument();
        expect(screen.getByText(/visible assistant tail/)).toBeInTheDocument();
    });

    it('does not leak original preview metadata while anti-leakage masking is active', () => {
        antiLeakageState.enabled = true;
        antiLeakageState.isRevealed = false;
        const secretContent = `${'secret-value-line\n'.repeat(900)}hidden-final-line`;

        render(
            <MessageItem
                message={createMessage({
                    content: secretContent,
                    message_type: 'tool_result',
                    large_message_preview: {
                        lineCount: 261,
                        payloadCharCount: secretContent.length,
                        contentHash: 'sha256:secret',
                        reason: 'tool_result',
                        shouldPreview: true,
                        summary: '大型工具结果已折叠',
                        previewText: 'secret-value from backend metadata',
                    },
                })}
            />,
        );

        expect(screen.queryByText('展开完整内容')).not.toBeInTheDocument();
        expect(screen.queryByText('收起完整内容')).not.toBeInTheDocument();
        expect(screen.queryByText(/secret-value/)).not.toBeInTheDocument();
        expect(screen.queryByText(/hidden-final-line/)).not.toBeInTheDocument();
    });
});

describe('MessageItem user message folding', () => {
    beforeEach(() => {
        antiLeakageState.enabled = false;
        antiLeakageState.isRevealed = true;
    });

    it('keeps a 300-character user message fully visible', () => {
        const content = '用'.repeat(300);

        render(
            <MessageItem
                message={createMessage({ content, message_type: 'user' })}
            />,
        );

        expect(screen.getByText(content)).toBeInTheDocument();
        expect(screen.queryByRole('button', { name: '展开' })).not.toBeInTheDocument();
    });

    it('shows only the first 300 characters until the user expands the message', async () => {
        const user = userEvent.setup();
        const visibleContent = '前'.repeat(300);
        const hiddenContent = '这是折叠后的内容';
        const content = `${visibleContent}${hiddenContent}`;

        render(
            <MessageItem
                message={createMessage({ content, message_type: 'user' })}
            />,
        );

        expect(screen.getByText(`${visibleContent}…`)).toBeInTheDocument();
        expect(screen.queryByText(content)).not.toBeInTheDocument();

        await user.click(screen.getByRole('button', { name: '展开' }));

        expect(screen.getByText(content)).toBeInTheDocument();
        expect(screen.getByRole('button', { name: '收起' })).toHaveAttribute('aria-expanded', 'true');

        await user.click(screen.getByRole('button', { name: '收起' }));

        expect(screen.getByText(`${visibleContent}…`)).toBeInTheDocument();
    });
});

describe('MessageItem streaming markdown blocks', () => {
    it('should keep closed blocks mounted when the stream finishes', () => {
        markdownMountCounts.clear();
        const content = 'first block\n\nsecond block';
        const message = createMessage({ id: 31, content: '' });
        const { rerender } = render(
            <MessageItem
                message={message}
                streamEvent={{ message_id: 31, message_type: 'response', content, is_done: false }}
            />,
        );

        expect(screen.getByText('first block')).toBeInTheDocument();
        expect(screen.getByText('second block')).toBeInTheDocument();

        rerender(
            <MessageItem
                message={{ ...message, content }}
                streamEvent={{ message_id: 31, message_type: 'response', content, is_done: true }}
            />,
        );

        expect(markdownMountCounts.get('first block')).toBe(1);
        expect(markdownMountCounts.get('second block')).toBe(1);
    });

    it('should render historical messages as a single markdown document', () => {
        markdownMountCounts.clear();
        render(<MessageItem message={createMessage({ id: 32, content: 'one\n\ntwo' })} />);

        expect(markdownMountCounts.get('one\n\ntwo')).toBe(1);
    });
});

const stablePlanAction = () => undefined;

describe('MessageItem render stability', () => {
    it('does not render a historical message again when only the tail content changes', () => {
        resetMessageItemRenderCounts();
        const history = createMessage({ id: 11, content: 'earlier answer' });
        const { rerender } = render(
            <>
                <MessageItem
                    message={history}
                    onContinueAgentPlan={stablePlanAction}
                    onApproveAgentPlan={stablePlanAction}
                />
                <MessageItem
                    message={createMessage({ id: 12, content: 'partial' })}
                    onContinueAgentPlan={stablePlanAction}
                    onApproveAgentPlan={stablePlanAction}
                />
            </>,
        );
        const historyRenders = getMessageItemRenderCount(11);

        rerender(
            <>
                <MessageItem
                    message={history}
                    onContinueAgentPlan={stablePlanAction}
                    onApproveAgentPlan={stablePlanAction}
                />
                <MessageItem
                    message={createMessage({ id: 12, content: 'partial plus a token' })}
                    onContinueAgentPlan={stablePlanAction}
                    onApproveAgentPlan={stablePlanAction}
                />
            </>,
        );

        expect(getMessageItemRenderCount(11)).toBe(historyRenders);
        expect(getMessageItemRenderCount(12)).toBeGreaterThan(historyRenders);
    });

    it('renders again when the plan callback identity changes', () => {
        resetMessageItemRenderCounts();
        const history = createMessage({ id: 21, content: 'earlier answer' });
        const { rerender } = render(
            <MessageItem message={history} onContinueAgentPlan={stablePlanAction} />,
        );
        const renders = getMessageItemRenderCount(21);

        rerender(
            <MessageItem message={history} onContinueAgentPlan={() => undefined} />,
        );

        expect(getMessageItemRenderCount(21)).toBeGreaterThan(renders);
    });
});
