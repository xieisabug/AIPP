import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useMarkdownConfig } from './useMarkdownConfig';
import { markdownRegistry } from '@/services/markdownRegistry';

vi.mock('@/hooks/use-platform', () => ({ useIsMobilePlatform: () => true }));
vi.mock('@/components/RustCodeBlock', () => ({ default: () => null }));
vi.mock('@/components/common/LazyImage', () => ({ default: () => null }));

describe('useMarkdownConfig shared configuration', () => {
    afterEach(() => markdownRegistry.clearTagsForPlugin('perf-test'));

    it('should preserve configuration identity when inputs stay unchanged', () => {
        const { result, rerender } = renderHook(() => useMarkdownConfig());
        const original = result.current;
        rerender();
        expect(result.current).toBe(original);
        expect(result.current.markdownComponents.table).toBeTypeOf('function');
    });

    it('should refresh the shared configuration when a plugin registers or unregisters', () => {
        const { result } = renderHook(() => useMarkdownConfig());
        const original = result.current;
        act(() => markdownRegistry.registerTag('perf-test', { tagName: 'perf-tag', attributes: ['value'], render: () => null }));
        expect(result.current).not.toBe(original);
        expect(result.current.markdownComponents).toHaveProperty('perf-tag');
        expect(result.current.rehypePlugins).not.toBe(original.rehypePlugins);
        act(() => markdownRegistry.clearTagsForPlugin('perf-test'));
        expect(result.current.markdownComponents).not.toHaveProperty('perf-tag');
    });
});
