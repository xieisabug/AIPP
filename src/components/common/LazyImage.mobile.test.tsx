import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { PropsWithChildren } from 'react';
import { toast } from 'sonner';
import { isAndroid } from '@/services/mobileFileService';
import LazyImage from './LazyImage';

vi.mock('sonner', () => ({ toast: { error: vi.fn(), info: vi.fn(), success: vi.fn() } }));
vi.mock('@/services/mobileFileService', () => ({ isAndroid: vi.fn(), mobileFileAction: vi.fn(), encodeMobileExport: vi.fn(), outputMobileFile: vi.fn() }));
vi.mock('@/components/ui/context-menu', () => {
    const Wrapper = ({ children }: PropsWithChildren) => <>{children}</>;
    return { ContextMenu: Wrapper, ContextMenuTrigger: Wrapper, ContextMenuContent: Wrapper,
        ContextMenuSeparator: () => null,
        ContextMenuItem: ({ children, onClick }: PropsWithChildren<{ onClick: () => void }>) => <button onClick={onClick}>{children}</button> };
});

beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(isAndroid).mockResolvedValue(true);
    vi.stubGlobal('createImageBitmap', vi.fn());
    vi.stubGlobal('IntersectionObserver', class {
        callback: IntersectionObserverCallback;
        constructor(callback: IntersectionObserverCallback) { this.callback = callback; }
        observe() { this.callback([{ isIntersecting: true } as IntersectionObserverEntry], this as unknown as IntersectionObserver); }
        disconnect() {}
    });
});
afterEach(() => vi.unstubAllGlobals());

it('should reject a large image before bitmap allocation when copying on Android', async () => {
    render(<LazyImage src="https://example.test/image.png" alt="fixture" />);
    const image = await screen.findByAltText('fixture');
    Object.defineProperties(image, { complete: { value: true }, naturalWidth: { value: 5000 }, naturalHeight: { value: 5000 } });
    fireEvent.click(screen.getByText('复制图片'));
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('1600 万像素')));
    expect(createImageBitmap).not.toHaveBeenCalled();
});

it('should display a platform lookup failure when saving an image', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, blob: async () => new Blob(['png'], { type: 'image/png' }) }));
    vi.mocked(isAndroid).mockRejectedValue(new Error('get_platform: IPC unavailable'));
    render(<LazyImage src="https://example.test/image.png" alt="fixture" />);
    fireEvent.click(await screen.findByText('保存图片'));
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('get_platform: IPC unavailable')));
});
