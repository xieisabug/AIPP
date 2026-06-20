import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ContextList from './ContextList';
import { ContextItem } from './types';

vi.mock('@tauri-apps/plugin-opener', () => ({
    openUrl: vi.fn(),
}));

afterEach(() => {
    vi.useRealTimers();
});

describe('ContextList skills', () => {
    it('groups active skills under the Skills section', () => {
        const items: ContextItem[] = [
            {
                id: 'skill-1',
                type: 'skill',
                name: 'skill-creator',
                details: 'agents:skill-creator',
                source: 'user',
            },
        ];

        render(<ContextList items={items} />);

        expect(screen.getByText('Skills')).toBeInTheDocument();
        expect(screen.getByText('skill-creator')).toBeInTheDocument();
        expect(screen.getByText('agents:skill-creator')).toBeInTheDocument();
        expect(screen.queryByText('用户文件')).not.toBeInTheDocument();
    });

    it('exposes full truncated values through title attributes', async () => {
        const items: ContextItem[] = [
            {
                id: 'read-1',
                type: 'read_file',
                name: '/very/long/path/to/a/file/that/gets/truncated/in/the/sidebar/App.tsx',
                details: 'read_file',
                source: 'mcp',
            },
            {
                id: 'search-1',
                type: 'search',
                name: 'A very long search query that is likely to be truncated in the sidebar',
                source: 'mcp',
                searchResults: [
                    {
                        title: 'A long search result title that may not fit in the available row width',
                        url: 'https://example.com',
                        snippet: 'A long snippet that should still be available in full when the user hovers the clamped text.',
                    },
                ],
            },
        ];

        render(<ContextList items={items} />);

        expect(screen.getByText('App.tsx')).toBeInTheDocument();
        expect(
            screen.queryByText('/very/long/path/to/a/file/that/gets/truncated/in/the/sidebar/App.tsx'),
        ).not.toBeInTheDocument();
        expect(
            screen.getByTitle('/very/long/path/to/a/file/that/gets/truncated/in/the/sidebar/App.tsx'),
        ).toBeInTheDocument();
        expect(screen.getByTitle('read_file')).toBeInTheDocument();
        expect(
            screen.queryByTitle('A long search result title that may not fit in the available row width'),
        ).not.toBeInTheDocument();

        await userEvent.click(screen.getByLabelText('展开搜索结果'));

        expect(
            screen.getByTitle('A long search result title that may not fit in the available row width'),
        ).toBeInTheDocument();
        expect(
            screen.getByTitle('A long snippet that should still be available in full when the user hovers the clamped text.'),
        ).toBeInTheDocument();
    });

    it('groups assistant generated images under the generated images section', () => {
        const items: ContextItem[] = [
            {
                id: 'generated-image-1',
                type: 'generated_image',
                name: '图片 1',
                details: 'generated-image-1.png',
                source: 'assistant',
                attachmentData: {
                    type: 'Image',
                    content: 'data:image/png;base64,Zm9v',
                    url: 'generated-image-1.png',
                },
                previewData: {
                    title: '图片 1',
                    contentType: 'image',
                    content: 'data:image/png;base64,Zm9v',
                },
            },
        ];

        render(<ContextList items={items} />);

        expect(screen.getByText('生成图片')).toBeInTheDocument();
        expect(screen.getByText('图片 1')).toBeInTheDocument();
        expect(screen.queryByText('用户文件')).not.toBeInTheDocument();
    });

    it('groups preview files and invokes the preview click callback', async () => {
        const user = userEvent.setup();
        const onPreviewFileClick = vi.fn();
        const item: ContextItem = {
            id: 'preview-1',
            type: 'preview_file',
            name: '/workspace/src',
            details: 'App.tsx',
            source: 'mcp',
            previewFileData: {
                callId: 12,
                conversationId: 1,
                messageId: 20,
                requestId: 'req-1',
            },
        };

        render(<ContextList items={[item]} onPreviewFileClick={onPreviewFileClick} />);

        expect(screen.getByText('预览文件')).toBeInTheDocument();
        await user.click(screen.getByText('/workspace/src'));

        expect(onPreviewFileClick).toHaveBeenCalledWith(item);
    });

    it('auto-expands fresh search results briefly and keeps manual opens expanded', async () => {
        vi.useFakeTimers();
        vi.setSystemTime(new Date('2026-06-12T00:00:00.000Z'));
        const searchItem: ContextItem = {
            id: 'search-fresh',
            type: 'search',
            name: 'fresh query',
            source: 'mcp',
            timestamp: new Date(),
            searchResults: [
                {
                    title: 'Fresh result',
                    url: 'https://example.com/fresh',
                    snippet: 'Fresh snippet',
                },
            ],
        };

        const { rerender } = render(<ContextList items={[]} />);

        rerender(<ContextList items={[searchItem]} />);

        expect(screen.getByTitle('Fresh result')).toBeInTheDocument();

        act(() => {
            vi.advanceTimersByTime(5000);
        });

        expect(screen.queryByTitle('Fresh result')).not.toBeInTheDocument();

        fireEvent.click(screen.getByLabelText('展开搜索结果'));

        expect(screen.getByTitle('Fresh result')).toBeInTheDocument();

        act(() => {
            vi.advanceTimersByTime(10000);
        });

        expect(screen.getByTitle('Fresh result')).toBeInTheDocument();
    });

    it('expands a focused search item 300ms after highlight starts', () => {
        vi.useFakeTimers();
        Element.prototype.scrollIntoView = vi.fn();
        const searchItem: ContextItem = {
            id: 'search-focused',
            type: 'search',
            name: 'focused query',
            source: 'mcp',
            searchResults: [
                {
                    title: 'Focused result',
                    url: 'https://example.com/focused',
                    snippet: 'Focused snippet',
                },
            ],
        };

        render(<ContextList items={[searchItem]} focusedItemId="search-focused" />);

        expect(screen.queryByTitle('Focused result')).not.toBeInTheDocument();

        act(() => {
            vi.advanceTimersByTime(299);
        });
        expect(screen.queryByTitle('Focused result')).not.toBeInTheDocument();

        act(() => {
            vi.advanceTimersByTime(1);
        });
        expect(screen.getByTitle('Focused result')).toBeInTheDocument();
    });

    it('keeps the selected context row visually selected', () => {
        const items: ContextItem[] = [
            {
                id: 'search-selected',
                type: 'search',
                name: 'selected query',
                source: 'mcp',
            },
        ];

        render(<ContextList items={items} selectedItemId="search-selected" />);

        expect(screen.getByText('selected query').closest('.cursor-pointer')).toHaveClass('bg-muted/50');
    });
});
