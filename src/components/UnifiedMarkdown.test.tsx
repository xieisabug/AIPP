import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import UnifiedMarkdown from './UnifiedMarkdown';
import { useMarkdownConfig } from '@/hooks/useMarkdownConfig';

vi.mock('@/hooks/useMarkdownConfig', async () => {
    const { createContext } = await import('react');
    return {
        MarkdownStreamingContext: createContext(false),
        useMarkdownConfig: vi.fn(() => ({ remarkPlugins: [], rehypePlugins: [], markdownComponents: {} })),
    };
});

describe('UnifiedMarkdown configuration reuse', () => {
    beforeEach(() => vi.clearAllMocks());

    it('should reuse supplied configuration when rendering a message', () => {
        const config = { remarkPlugins: [], rehypePlugins: [], markdownComponents: {} };
        render(<UnifiedMarkdown markdownConfig={config}># Message</UnifiedMarkdown>);
        expect(screen.getByRole('heading', { name: 'Message' })).toBeTruthy();
        expect(useMarkdownConfig).not.toHaveBeenCalled();
    });

    it('should update renderers when supplied configuration changes', () => {
        const config = { remarkPlugins: [], rehypePlugins: [], markdownComponents: {} };
        const { rerender } = render(<UnifiedMarkdown markdownConfig={config}># Message</UnifiedMarkdown>);
        const changedConfig = { ...config, markdownComponents: { h1: () => <h1>Updated renderer</h1> } };
        rerender(<UnifiedMarkdown markdownConfig={changedConfig}># Message</UnifiedMarkdown>);
        expect(screen.getByRole('heading', { name: 'Updated renderer' })).toBeTruthy();
        expect(useMarkdownConfig).not.toHaveBeenCalled();
    });

    it('should resolve its own configuration when used independently', () => {
        const onCodeRun = vi.fn();
        render(<UnifiedMarkdown onCodeRun={onCodeRun} disableMarkdownSyntax>Standalone</UnifiedMarkdown>);
        expect(screen.getByText('Standalone')).toBeTruthy();
        expect(useMarkdownConfig).toHaveBeenCalledWith({ onCodeRun, disableMarkdownSyntax: true });
    });
});
