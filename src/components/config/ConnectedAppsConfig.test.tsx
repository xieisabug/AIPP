import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { invoke } from '@tauri-apps/api/core';
import ConnectedAppsConfig from './ConnectedAppsConfig';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('@/hooks/use-platform', () => ({ useIsMobilePlatform: () => false }));
vi.mock('./MCPConfig', () => ({ default: ({ initialServerId }: { initialServerId: number }) => <div>管理实例 {initialServerId}</div> }));
vi.mock('./MCPServerDialog', () => ({ default: () => null }));
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
            if (command === 'list_app_connections') return [{ server_id: 21, app_id: 'notes', definition, label: '工作笔记', config: {}, status: 'available', error: null }];
            if (command === 'fetch_app_catalog') return { catalog: { schema_version: 1, version: '1', apps: [definition] }, warning: null };
            return [];
        });
        const user = userEvent.setup();
        render(<ConnectedAppsConfig />);
        await user.click(await screen.findByRole('button', { name: /笔记服务/ }));
        await user.click(await screen.findByRole('button', { name: /工作笔记/ }));
        await user.click(screen.getByRole('tab', { name: '工具与管理' }));
        expect(await screen.findByText('管理实例 21')).toBeInTheDocument();
        expect(vi.mocked(invoke).mock.calls.some(([cmd]) => cmd === 'add_mcp_server')).toBe(false);
    });
});
