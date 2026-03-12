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
});
