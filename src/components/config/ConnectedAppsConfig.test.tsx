import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import ConnectedAppsConfig from './ConnectedAppsConfig';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('@/hooks/use-platform', () => ({ useIsMobilePlatform: () => false }));
vi.mock('./MCPConfig', () => ({ default: ({ initialServerId }: { initialServerId: number }) => <div>管理实例 {initialServerId}</div> }));
vi.mock('./MCPServerDialog', () => ({ default: ({ isOpen, initialConfig }: { isOpen: boolean; initialConfig?: { transport_type?: string } }) => isOpen ? <div>编辑 MCP {initialConfig?.transport_type}</div> : null }));
vi.mock('./JSONImportDialog', () => ({ default: () => null }));
vi.mock('./BuiltinToolDialog', () => ({ default: () => null }));

const builtin = { id: 7, name: '搜索工具', description: '搜索网页', command: 'aipp:search', is_builtin: true, is_enabled: true, transport_type: 'stdio' };
const legacy = { id: 8, name: '公司知识库', description: '已有配置', command: null, is_builtin: false, is_enabled: true, transport_type: 'http' };
const definition = { id: 'notes', name: '笔记服务', version: '1', publisher: 'Example', category: '效率', description: '笔记', notice: '', platforms: ['windows'], fields: [], connection: { transport: 'http', url: 'https://example.com/mcp', auth: 'none' } };

describe('ConnectedAppsConfig', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === 'get_mcp_servers') return [builtin, legacy];
            if (command === 'list_app_connections') return [];
            if (command === 'list_aipp_builtin_templates') return [{ id: 'search', name: '搜索工具', description: '搜索网页', command: 'aipp:search' }];
            if (command === 'get_mcp_server_tools') return [];
            if (command === 'fetch_app_catalog') return { catalog: null, warning: '服务端目录暂不可用', fetched_at: null };
            throw new Error(`Unexpected command: ${command}`);
        });
    });
    it('should retain builtins and existing MCP connections when the remote catalog is unavailable', async () => {
        const user = userEvent.setup();
        render(<ConnectedAppsConfig />);
        expect(await screen.findByText('搜索工具')).toBeInTheDocument();
        expect(await screen.findByRole('status')).toHaveTextContent('服务端目录暂不可用');
        await user.click(screen.getByRole('tab', { name: /我的应用/ }));
        expect(await screen.findByText('公司知识库')).toBeInTheDocument();
        await user.click(screen.getByRole('button', { name: /公司知识库/ }));
        await user.click(screen.getByRole('tab', { name: '工具与管理' }));
        expect(await screen.findByText('管理实例 8')).toBeInTheDocument();
    });
    it('should open the same MCP instance when managing an application from the catalog', async () => {
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === 'get_mcp_servers') return [{ ...legacy, id: 21, name: '笔记 · 1234' }];
            if (command === 'list_app_connections') return [{ server_id: 21, app_id: 'notes', definition, label: '工作笔记', config: {}, status: 'available', error: null, credential_status: '连接凭据仅在本机加密保存' }];
            if (command === 'fetch_app_catalog') return { catalog: { schema_version: 1, version: '1', apps: [definition] }, warning: null };
            return [];
        });
        const user = userEvent.setup();
        render(<ConnectedAppsConfig />);
        await user.click(await screen.findByRole('button', { name: /笔记服务/ }));
        await user.click(await screen.findByRole('button', { name: /工作笔记/ }));
        await user.click(screen.getByRole('tab', { name: '工具与管理' }));
        expect(await screen.findByText('管理实例 21')).toBeInTheDocument();
        expect(screen.getByText('连接凭据仅在本机加密保存')).toBeInTheDocument();
        expect(vi.mocked(invoke).mock.calls.some(([cmd]) => cmd === 'add_mcp_server')).toBe(false);
    });
    it('should distinguish undiscovered tools from zero tools when showing existing connections', async () => {
        const user = userEvent.setup();
        render(<ConnectedAppsConfig />);
        await screen.findByText('搜索工具');
        await user.click(screen.getByRole('tab', { name: /我的应用/ }));
        expect(await screen.findByRole('button', { name: /公司知识库/ })).toHaveTextContent('工具未检测');
        expect(screen.getByRole('button', { name: /搜索工具/ })).toHaveTextContent('0 / 0 个工具');
    });
    it('should reopen the pending authorization without creating another connection when login is in progress', async () => {
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === 'get_mcp_servers') return [{ ...legacy, id: 21, is_enabled: false }];
            if (command === 'list_app_connections') return [{ server_id: 21, app_id: 'notes', definition, label: '工作笔记', config: {}, status: 'connecting', error: null }];
            if (command === 'get_app_authorization_url') return 'https://example.com/authorize?state=test';
            return [];
        });
        const user = userEvent.setup();
        render(<ConnectedAppsConfig />);
        await user.click(screen.getByRole('tab', { name: /我的应用/ }));
        await user.click(await screen.findByRole('button', { name: /工作笔记/ }));
        await user.click(screen.getByRole('button', { name: '继续授权' }));
        expect(openUrl).toHaveBeenCalledWith('https://example.com/authorize?state=test');
        expect(vi.mocked(invoke).mock.calls.some(([cmd]) => cmd === 'connect_catalog_app')).toBe(false);
    });
    it('should retain quick templates and JSON import when adding a connection', async () => {
        const user = userEvent.setup();
        render(<ConnectedAppsConfig />);
        await user.click(screen.getByRole('button', { name: '添加' }));
        expect(screen.getByRole('menuitem', { name: '从 JSON 导入' })).toBeInTheDocument();
        expect(screen.getByRole('menuitem', { name: 'Stdio MCP 模板' })).toBeInTheDocument();
        await user.click(screen.getByRole('menuitem', { name: 'HTTP MCP 模板' }));
        expect(screen.getByText('编辑 MCP http')).toBeInTheDocument();
    });
    it('should require an explicit choice to skip revocation when remote revocation fails', async () => {
        let status = 'available';
        let error: string | null = null;
        vi.mocked(invoke).mockImplementation(async (command, args) => {
            if (command === 'get_mcp_servers') return [{ ...legacy, id: 21, is_enabled: status === 'available' }];
            if (command === 'list_app_connections') return [{ server_id: 21, app_id: 'notes', definition, label: '工作笔记', config: {}, status, error }];
            if (command === 'disconnect_app_connection') {
                if ((args as { revokeRemote: boolean }).revokeRemote) {
                    status = 'disconnected'; error = '服务商撤销未完成'; throw new Error(error);
                }
                error = null; return '已删除本机凭据';
            }
            return [];
        });
        const user = userEvent.setup();
        render(<ConnectedAppsConfig />);
        await user.click(screen.getByRole('tab', { name: /我的应用/ }));
        await user.click(await screen.findByRole('button', { name: /工作笔记/ }));
        await user.click(screen.getByRole('tab', { name: '连接配置' }));
        expect(screen.queryByRole('button', { name: '删除凭据，不撤销授权' })).not.toBeInTheDocument();
        await user.click(screen.getByRole('button', { name: '断开连接' }));
        await user.click(await screen.findByRole('button', { name: '删除凭据，不撤销授权' }));
        expect(invoke).toHaveBeenCalledWith('disconnect_app_connection', { serverId: 21, revokeRemote: false });
    });
    it('should remove an existing connection only after confirmation from its overview', async () => {
        let removed = false;
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === 'get_mcp_servers') return removed ? [] : [{ ...legacy, is_deletable: true }];
            if (command === 'delete_mcp_server') { removed = true; return; }
            return [];
        });
        const user = userEvent.setup();
        render(<ConnectedAppsConfig />);
        await user.click(screen.getByRole('tab', { name: /我的应用/ }));
        await user.click(await screen.findByRole('button', { name: /公司知识库/ }));
        await user.click(screen.getByRole('button', { name: '删除连接' }));
        expect(invoke).not.toHaveBeenCalledWith('delete_mcp_server', expect.anything());
        await user.click(screen.getByRole('button', { name: '确认' }));
        expect(invoke).toHaveBeenCalledWith('delete_mcp_server', { id: 8 });
        await waitFor(() => expect(screen.queryByRole('button', { name: /公司知识库/ })).not.toBeInTheDocument());
        expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    });

});
