import { act, render, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import useFileDropHandler from './useFileDropHandler';

const { listen, unlisten } = vi.hoisted(() => ({ listen: vi.fn(), unlisten: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true }));
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: listen }) }));

describe('native attachment drops', () => {
    it('uses the original paths only for drops inside the active conversation', async () => {
        listen.mockResolvedValue(unlisten);
        const paths = vi.fn();
        const files = vi.fn();
        function DropZone() {
            const { dropRef } = useFileDropHandler(files, paths);
            return <div ref={dropRef} data-testid="zone" />;
        }
        const view = render(<DropZone />);
        const zone = view.getByTestId('zone');
        vi.spyOn(zone, 'getBoundingClientRect').mockReturnValue(new DOMRect(10, 10, 100, 100));
        const emit = listen.mock.calls[0][0];
        const point = (n: number) => ({ x: n * window.devicePixelRatio, y: n * window.devicePixelRatio });
        act(() => {
            emit({ payload: { type: 'drop', position: point(5), paths: ['/outside.zip'] } });
            emit({ payload: { type: 'drop', position: point(25), paths: ['/data/report.xlsx', '/data/report.pdf'] } });
        });
        expect(paths.mock.calls).toEqual([[['/data/report.xlsx', '/data/report.pdf']]]);
        expect(files).not.toHaveBeenCalled();
        view.unmount();
        await waitFor(() => expect(unlisten).toHaveBeenCalledOnce());
    });
});
