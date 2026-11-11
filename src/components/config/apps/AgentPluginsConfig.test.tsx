import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { invoke } from '@tauri-apps/api/core';
import AgentPluginsConfig from './AgentPluginsConfig';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => vi.fn()) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
const source = { repo: 'openai/plugins', git_ref: 'main', path: 'plugins/notion' };
const preview = { key: 'abc', source, revision: 'a'.repeat(40), digest: 'sha', name: 'Notion', description: 'Notion workflows', version: '1.0', publisher: 'Notion',
    skills: [{ id: 'agent_plugin:abc/capture', name: 'capture', description: '保存笔记', content: '# Capture', path: 'skills/capture/SKILL.md', files: [] }],
    servers: [{ name: 'notion', config: { url: 'https://mcp.notion.com/mcp' }, reason: null }], notices: [], files: ['skills/capture/SKILL.md'], locally_modified: false };

describe('AgentPluginsConfig', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === 'inspect_agent_plugin') return preview;
            if (command === 'list_agent_plugins' || command === 'get_mcp_servers' || command === 'list_app_connections') return [];
            throw new Error(`Unexpected ${command}`);
        });
    });
    it('should preview skills without connecting when a GitHub package is selected', async () => {
        render(<AgentPluginsConfig initialSource={source} onChanged={vi.fn()} />);
        expect(await screen.findByText('保存笔记')).toBeInTheDocument();
        expect(screen.getByRole('button', { name: '添加连接' })).toBeDisabled();
        expect(invoke).not.toHaveBeenCalledWith('connect_catalog_app', expect.anything());
        expect(screen.getByText(/没有实际工具清单/)).toBeInTheDocument();
        expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });
    it('should install exactly the inspected revision when installation is requested', async () => {
        const onChanged = vi.fn();
        vi.mocked(invoke).mockImplementation(async command => command === 'inspect_agent_plugin' ? preview : command === 'install_agent_plugin' ? { preview, installed_at: 'now' } : []);
        render(<AgentPluginsConfig initialSource={source} onChanged={onChanged} />);
        fireEvent.click(await screen.findByRole('button', { name: '安装插件与技能' }));
        await waitFor(() => expect(onChanged).toHaveBeenCalled());
        expect(invoke).toHaveBeenCalledWith('install_agent_plugin', { source, revision: preview.revision, digest: 'sha' });
    });
    it('should block replacement when local plugin files were modified', async () => {
        vi.mocked(invoke).mockImplementation(async command => command === 'inspect_agent_plugin' ? { ...preview, locally_modified: true } : []);
        render(<AgentPluginsConfig initialSource={source} onChanged={vi.fn()} />);
        expect(await screen.findByRole('button', { name: '安装插件与技能' })).toBeDisabled();
        expect(screen.getByRole('alert')).toHaveTextContent('本地修改');
    });
    it('should retain the error and avoid installation when GitHub inspection fails', async () => {
        vi.mocked(invoke).mockImplementation(async command => { if (command === 'inspect_agent_plugin') throw new Error('GitHub HTTP 403'); return []; });
        render(<AgentPluginsConfig initialSource={source} onChanged={vi.fn()} />);
        expect(await screen.findByRole('alert')).toHaveTextContent('GitHub HTTP 403');
        expect(screen.queryByRole('button', { name: '安装插件与技能' })).not.toBeInTheDocument();
    });
    it('should launch stdio with encrypted-input values only after the user starts it', async () => {
        const local = { ...preview, servers: [{name: 'local', config: {command: 'node', args: ['./server.mjs']}, reason: null}] };
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === 'inspect_agent_plugin') return local;
            if (command === 'list_agent_plugins') return [{preview:local,installed_at:'now'}];
            if (command === 'connect_agent_plugin_stdio') return 42;
            return [];
        });
        render(<AgentPluginsConfig initialSource={source} onChanged={vi.fn()} />);
        const add = await screen.findByRole('button',{name:'添加连接'});
        await waitFor(() => expect(add).toBeEnabled()); fireEvent.click(add);
        const input = await screen.findByRole('textbox',{name:'插件环境变量 JSON'});
        expect(invoke).not.toHaveBeenCalledWith('connect_agent_plugin_stdio',expect.anything());
        fireEvent.change(input,{target:{value:'{"TOKEN":"test-input"}'}});
        fireEvent.click(screen.getByRole('button',{name:'启动并检测工具'}));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith('verify_app_connection',{serverId:42}));
        expect(invoke).toHaveBeenCalledWith('connect_agent_plugin_stdio',{key:'abc',serverName:'local',values:{TOKEN:'test-input'},serverId:null});
        expect(input).toHaveValue('{}');
    });
    it('should expose public dependency installation separately from platform-only connectors', async () => {
        const deps = { ...preview, dependencies:[{name:'Notion dependency',connector_id:'public',source},{name:'Platform only',connector_id:'private',source:null}] };
        vi.mocked(invoke).mockImplementation(async command => command === 'inspect_agent_plugin' ? deps : []);
        render(<AgentPluginsConfig initialSource={source} onChanged={vi.fn()} />);
        expect(await screen.findByText(/Platform only.*没有公开接入地址/)).toBeInTheDocument();
        expect(screen.getAllByRole('button',{name:'查看并安装依赖'})).toHaveLength(1);
    });
    it('should uninstall an installed package offline and refresh the installed list', async () => {
        let installed = true;
        const onChanged = vi.fn();
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === 'list_agent_plugins') return installed ? [{ preview, installed_at: 'now' }] : [];
            if (command === 'inspect_agent_plugin') throw new Error('offline');
            if (command === 'uninstall_agent_plugin') { installed = false; return; }
            return [];
        });
        render(<AgentPluginsConfig initialSource={source} onChanged={onChanged} />);
        fireEvent.click(await screen.findByRole('button', { name: '卸载插件' }));
        expect(invoke).not.toHaveBeenCalledWith('uninstall_agent_plugin', expect.anything());
        fireEvent.click(screen.getByRole('button', { name: '确认卸载' }));
        await screen.findByRole('button', { name: '已安装 0' });
        expect(invoke).toHaveBeenCalledWith('uninstall_agent_plugin', { key: 'abc', revokeRemote: true });
        expect(invoke).not.toHaveBeenCalledWith('inspect_agent_plugin', expect.anything());
        expect(onChanged).toHaveBeenCalled();
    });
    it('should retain the installed package on failure and allow explicit removal without remote revocation', async () => {
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === 'list_agent_plugins') return [{ preview, installed_at: 'now' }];
            if (command === 'uninstall_agent_plugin') throw new Error('服务商撤销未完成');
            return [];
        });
        render(<AgentPluginsConfig initialSource={source} onChanged={vi.fn()} />);
        fireEvent.click(await screen.findByRole('button', { name: '卸载插件' }));
        fireEvent.click(screen.getByRole('button', { name: '确认卸载' }));
        expect(await screen.findByRole('alert')).toHaveTextContent('服务商撤销未完成');
        expect(screen.getByRole('button', { name: '卸载插件' })).toBeInTheDocument();
        fireEvent.click(screen.getByRole('checkbox', { name: /同时撤销服务商授权/ }));
        fireEvent.click(screen.getByRole('button', { name: '确认卸载' }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith('uninstall_agent_plugin', { key: 'abc', revokeRemote: false }));
    });

    it('should show skills and installation inline without repository fields in app presentation', async () => {
        const onConnect = vi.fn();
        let installed = false;
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === 'inspect_agent_plugin') return preview;
            if (command === 'list_agent_plugins') return installed ? [{preview,installed_at:'now'}] : [];
            if (command === 'install_agent_plugin') { installed = true; return; }
            if (command === 'uninstall_agent_plugin') { installed = false; return; }
            return [];
        });
        render(<AgentPluginsConfig embedded initialSource={source} onChanged={vi.fn()} onConnect={onConnect} />);
        expect(await screen.findByText('保存笔记')).toBeInTheDocument();
        expect(screen.queryByRole('textbox', {name:'GitHub 仓库'})).not.toBeInTheDocument();
        expect(screen.queryByRole('button', {name:'返回插件目录'})).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole('button', {name:'安装插件'}));
        await waitFor(() => expect(onConnect).toHaveBeenCalledOnce());
        expect(invoke).toHaveBeenCalledWith('install_agent_plugin',{source,revision:preview.revision,digest:preview.digest});
        await waitFor(() => expect(screen.getByRole('button',{name:'更多插件操作'})).toBeEnabled());
        await userEvent.click(screen.getByRole('button',{name:'更多插件操作'}));
        await userEvent.click(screen.getByRole('menuitem',{name:'卸载插件'}));
        fireEvent.click(await screen.findByRole('button',{name:'确认卸载'}));
        expect(await screen.findByRole('button',{name:'安装插件'})).toBeInTheDocument();
        expect(screen.queryByRole('textbox',{name:'GitHub 仓库'})).not.toBeInTheDocument();
    });

    it('should retain installation and prevent a second request when the sheet reopens', async () => {
        let finish!: () => void;
        let installed = false;
        const gate = new Promise<void>(resolve => { finish = resolve; });
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === 'inspect_agent_plugin') return preview;
            if (command === 'list_agent_plugins') return installed ? [{preview, installed_at:'now'}] : [];
            if (command === 'install_agent_plugin') { await gate; installed = true; return {preview, installed_at:'now'}; }
            return [];
        });
        const first = render(<AgentPluginsConfig embedded initialSource={source} onChanged={vi.fn()} onConnect={vi.fn()} />);
        await waitFor(() => expect(screen.getByRole('button', {name:'安装插件'})).toBeEnabled());
        expect(screen.queryByRole('button', {name:'检查更新'})).not.toBeInTheDocument();
        expect(screen.queryByText('高级管理与连接设置')).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole('button', {name:'安装插件'}));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith('install_agent_plugin', expect.anything()));
        first.unmount();
        render(<AgentPluginsConfig embedded initialSource={source} onChanged={vi.fn()} onConnect={vi.fn()} />);
        await screen.findByText('保存笔记');
        expect(screen.getByRole('button', {name:'安装插件'})).toBeDisabled();
        expect(screen.getByRole('status')).toHaveTextContent('正在准备安装');
        finish();
        await waitFor(() => expect(screen.getByRole('button', {name:'连接应用'})).toBeEnabled());
        expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'install_agent_plugin')).toHaveLength(1);
        expect(screen.getByRole('button', {name:'检查更新'})).toBeInTheDocument();
    });

});
