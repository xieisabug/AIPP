import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import AppConnectForm from './AppConnectForm';
import { AppDefinition, visibleAppFields } from '@/data/ConnectedApp';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
const app: AppDefinition = {
    id: 'test', name: 'Test', version: '1', description: 'Test app', publisher: 'Test', category: '效率', notice: '', platforms: ['windows'],
    connection: { transport: 'http', auth: 'fields', url: 'https://example.com/mcp' },
    fields: [{ key: 'token', label: 'API Key', kind: 'password', required: true, secret: true }],
};

describe('AppConnectForm', () => {
    beforeEach(() => vi.clearAllMocks());
    it('should open the published key settings when the user requests a key', () => {
        vi.mocked(openUrl).mockResolvedValueOnce();
        const keyUrl = 'https://accounts.example.com/settings/keys';
        render(<AppConnectForm app={{ ...app, connection: { ...app.connection, transport: 'http_api', http_api: { key_url: keyUrl } } }} onConnected={vi.fn()} />);
        fireEvent.click(screen.getByRole('button', { name: '创建 API Key' }));
        expect(openUrl).toHaveBeenCalledWith(keyUrl);
        expect(invoke).not.toHaveBeenCalled();
    });
    it('should keep failure visible when connection fails', async () => {
        vi.mocked(invoke).mockRejectedValue('安全存储不可用');
        const completed = vi.fn();
        render(<AppConnectForm app={app} onConnected={completed} />);
        fireEvent.change(screen.getByLabelText('API Key *'), { target: { value: 'fake-token' } });
        fireEvent.click(screen.getByRole('button', { name: '保存并验证' }));
        expect(await screen.findByRole('alert')).toHaveTextContent('安全存储不可用');
        expect(completed).not.toHaveBeenCalled();
    });
    it('should clear secret input when a connection is created', async () => {
        vi.mocked(invoke).mockResolvedValue({ server_id: 12, authorization_url: null });
        const completed = vi.fn();
        render(<AppConnectForm app={app} onConnected={completed} />);
        fireEvent.change(screen.getByLabelText('API Key *'), { target: { value: 'fake-token' } });
        fireEvent.click(screen.getByRole('button', { name: '保存并验证' }));
        await waitFor(() => expect(completed).toHaveBeenCalledWith(12));
        expect(screen.getByLabelText('API Key *')).toHaveValue('');
        expect(screen.queryByText('已连接')).not.toBeInTheDocument();
    });
    it('should use declared defaults when evaluating conditional fields', () => {
        const fields = [{ key: 'service', label: 'Service', kind: 'select' as const, default: 'local' },
            { key: 'token', label: 'Token', kind: 'password' as const, visible_when: { field: 'service', equals: 'remote' } }];
        expect(visibleAppFields(fields, {}).map(f => f.key)).toEqual(['service']);
        expect(visibleAppFields(fields, { service: 'remote' })).toHaveLength(2);
    });
    it('should retain the authorization retry when the browser cannot open', async () => {
        vi.mocked(invoke).mockResolvedValue({ server_id: 12, authorization_url: 'https://example.com/authorize' });
        vi.mocked(openUrl).mockRejectedValueOnce(new Error('browser unavailable')).mockResolvedValueOnce();
        const completed = vi.fn();
        render(<AppConnectForm app={{ ...app, connection: { ...app.connection, auth: 'oauth' }, fields: [] }} onConnected={completed} />);
        fireEvent.click(screen.getByRole('button', { name: '登录并连接' }));
        expect(await screen.findByRole('alert')).toHaveTextContent('无法打开系统浏览器');
        expect(completed).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole('button', { name: '重新打开授权页面' }));
        await waitFor(() => expect(completed).toHaveBeenCalledWith(12));
        expect(invoke).toHaveBeenCalledTimes(1);
    });
});
