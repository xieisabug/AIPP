import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { invoke } from '@tauri-apps/api/core';
import { MCPServer } from '@/data/MCP';
import AppTransportSettings from './AppTransportSettings';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

describe('AppTransportSettings', () => {
    it('should preserve connection identity and credential reference when updating network settings', async () => {
        const server = { id: 17, name: '工作笔记', transport_type: 'http', url: 'https://example.com/mcp', headers: '{"x-aipp-credential-ref":"reference"}', timeout: 30000, proxy_enabled: false, is_enabled: true } as MCPServer;
        const onSaved = vi.fn();
        vi.mocked(invoke).mockResolvedValue(undefined);
        const user = userEvent.setup();
        render(<AppTransportSettings server={server} onSaved={onSaved} />);
        await user.clear(screen.getByLabelText('调用超时（毫秒）'));
        await user.type(screen.getByLabelText('调用超时（毫秒）'), '45000');
        await user.click(screen.getByRole('switch'));
        await user.click(screen.getByRole('button', { name: '保存网络设置' }));
        expect(invoke).toHaveBeenCalledWith('update_mcp_server', { id: 17, request: { ...server, timeout: 45000, proxy_enabled: true } });
        expect(onSaved).toHaveBeenCalledOnce();
    });
});
