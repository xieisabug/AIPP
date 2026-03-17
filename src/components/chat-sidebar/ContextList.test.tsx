import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import ContextList from './ContextList';
import { ContextItem } from './types';

vi.mock('@tauri-apps/plugin-opener', () => ({
    openUrl: vi.fn(),
}));

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

    it('exposes full truncated values through title attributes', () => {
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

        expect(
            screen.getByTitle('/very/long/path/to/a/file/that/gets/truncated/in/the/sidebar/App.tsx'),
        ).toBeInTheDocument();
        expect(screen.getByTitle('read_file')).toBeInTheDocument();
        expect(
            screen.getByTitle('A long search result title that may not fit in the available row width'),
        ).toBeInTheDocument();
        expect(
            screen.getByTitle('A long snippet that should still be available in full when the user hovers the clamped text.'),
        ).toBeInTheDocument();
    });
});
