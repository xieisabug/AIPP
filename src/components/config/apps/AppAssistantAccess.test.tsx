import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import AppAssistantAccess from './AppAssistantAccess';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

describe('AppAssistantAccess', () => {
    beforeEach(() => vi.clearAllMocks());

    it('should update the existing assistant binding when permission is changed', async () => {
        vi.mocked(invoke).mockResolvedValueOnce([{ id: 7, name: '工作助手', enabled: false }]).mockResolvedValueOnce(undefined);
        render(<AppAssistantAccess serverId={21} />);
        const permission = await screen.findByRole('switch', { name: '允许 工作助手 使用此连接' });
        expect(permission).not.toBeChecked();
        fireEvent.click(permission);
        await waitFor(() => expect(permission).toBeChecked());
        expect(invoke).toHaveBeenCalledWith('update_assistant_mcp_config', { assistantId: 7, mcpServerId: 21, isEnabled: true });
    });

    it('should retain the saved permission when an update fails', async () => {
        vi.mocked(invoke).mockResolvedValueOnce([{ id: 7, name: '工作助手', enabled: true }]).mockRejectedValueOnce('权限保存失败');
        render(<AppAssistantAccess serverId={21} />);
        const permission = await screen.findByRole('switch', { name: '允许 工作助手 使用此连接' });
        fireEvent.click(permission);
        expect(await screen.findByRole('alert')).toHaveTextContent('权限保存失败');
        expect(permission).toBeChecked();
    });
});
