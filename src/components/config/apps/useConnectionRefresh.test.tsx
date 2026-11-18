import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { useConnectionRefresh } from './useConnectionRefresh';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
function Harness() {
    const { refreshConnection, refreshDialog } = useConnectionRefresh();
    return <><button onClick={() => void refreshConnection(42)}>刷新</button>{refreshDialog}</>;
}
describe('connection refresh dialog', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        vi.mocked(invoke).mockImplementation(async command => command === 'preview_connection_refresh' ? {
            token: 'token', added: ['new_tool'], removed: ['old_tool'], changed: ['generate_image'],
            details: { generate_image: [{field: '输入参数.properties.prompt.description', before: '旧说明', after: '新的参数说明'}] },
        } : undefined);
    });
    it('should show precise changes and apply only after confirmation', async () => {
        const user = userEvent.setup();
        render(<Harness />);
        await user.click(screen.getByText('刷新'));
        expect(await screen.findByRole('dialog')).toHaveTextContent('确认工具更新');
        await user.click(screen.getByText('输入参数 · 参数 · prompt · 说明'));
        expect(screen.getByText('旧说明')).toBeVisible();
        expect(screen.getByText('新的参数说明')).toBeVisible();
        expect(invoke).not.toHaveBeenCalledWith('apply_connection_refresh', expect.anything());
        await user.click(screen.getByRole('button', {name: '应用更新'}));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith('apply_connection_refresh', {serverId: 42, token: 'token'}));
    });
    it('should cancel without applying when dismissed or unmounted', async () => {
        const user = userEvent.setup();
        const view = render(<Harness />);
        await user.click(screen.getByText('刷新'));
        await screen.findByRole('dialog');
        await user.keyboard('{Escape}');
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
        await user.click(screen.getByText('刷新'));
        await screen.findByRole('dialog');
        await act(async () => view.unmount());
        expect(invoke).not.toHaveBeenCalledWith('apply_connection_refresh', expect.anything());
    });
});
