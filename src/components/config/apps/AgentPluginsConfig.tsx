import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Download, RefreshCw, ArrowLeft, Package, ExternalLink, Plus, ChevronDown, Blocks, Plug, MoreHorizontal } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { AppConnection, AppDefinition, connectionStatus } from '@/data/ConnectedApp';
import { useDataRefresh } from '@/hooks/useDataRefresh';
import { MCPServer, MCPServerTool } from '@/data/MCP';
import { installPlugin, usePluginInstall } from './pluginInstallState';
import AppConnectForm from './AppConnectForm';
import AppAssistantAccess from './AppAssistantAccess';

export interface PluginSource { repo: string; git_ref: string; path: string }
interface PluginSkill { id: string; name: string; description: string; content: string; path: string; files: string[]; kind?: string }
interface PluginServer { name: string; config: { url?: string; type?: string; oauth_resource?: string; command?: string; args?: string[]; env?: Record<string, string>; headers?: Record<string, string> }; reason?: string | null }
interface PluginAgent { path: string; name: string; content: string }
interface Preview { dependencies?: {name: string; connector_id: string; source?: PluginSource | null}[]; agents?: PluginAgent[]; key: string; source: PluginSource; revision: string; digest: string; name: string; description: string; version: string; publisher: string; skills: PluginSkill[]; servers: PluginServer[]; notices: string[]; files: string[]; installed_revision?: string | null; locally_modified: boolean; file_hashes?: Record<string, string>; icon?: string | null }
interface Receipt { preview: Preview; installed_at: string }
export interface PluginCatalog { revision: string; entries: { name: string; description: string; category: string; source: PluginSource; icon?: string | null; icon_dark?: string | null; icon_small?: string | null }[] }
const notion: PluginSource = { repo: 'openai/plugins', git_ref: 'main', path: 'plugins/notion' };

export default function AgentPluginsConfig({ initialSource, onChanged, embedded = false, app, onConnect, hasConnection = false }: { initialSource?: PluginSource; onChanged: () => void; embedded?: boolean; app?: AppDefinition; onConnect?: () => void; hasConnection?: boolean }) {
    const [expandedSkills, setExpandedSkills] = useState(false);
    const [managing, setManaging] = useState(false);
    const [catalog, setCatalog] = useState<PluginCatalog>();
    const [installed, setInstalled] = useState<Receipt[]>([]);
    const [preview, setPreview] = useState<Preview>();
    const [source, setSource] = useState(initialSource ?? notion);
    const [query, setQuery] = useState('');
    const [localBusy, setBusy] = useState('');
    const installation = usePluginInstall(preview?.key ?? '');
    const busy = installation?.pending ? installation.message : localBusy;
    const mounted = useRef(true);
    useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
    const [localError, setError] = useState('');
    const error = localError || installation?.error || '';
    useEffect(() => {
        if (installation && !installation.pending) void reload().catch(e => setError(String(e)));
    }, [installation]);
    const [definition, setDefinition] = useState<AppDefinition>();
    const [formGeneration, setFormGeneration] = useState(0);
    const [servers, setServers] = useState<MCPServer[]>([]);
    const [connections, setConnections] = useState<AppConnection[]>([]);
    const [editingConnection, setEditingConnection] = useState<AppConnection>();
    const [serverId, setServerId] = useState<number>();
    const [tools, setTools] = useState<MCPServerTool[]>([]);
    const [auth, setAuth] = useState('oauth');
    const [clientId, setClientId] = useState('');
    const [oauthMethod, setOauthMethod] = useState('none');
    const [redirectUri, setRedirectUri] = useState('');
    const [stdio, setStdio] = useState<PluginServer>();
    const [stdioReconnect, setStdioReconnect] = useState<number>();
    const [environment, setEnvironment] = useState('{}');
    const [templates, setTemplates] = useState<{id: number; name: string}[]>([]);
    const [templateId, setTemplateId] = useState('');
    const [agentResult, setAgentResult] = useState('');
    const [showInstalled, setShowInstalled] = useState(false);
    const [uninstallRequested, setUninstallRequested] = useState(false);
    const [revokeOnUninstall, setRevokeOnUninstall] = useState(true);
    const current = preview && installed.find(r => r.preview.key === preview.key);
    const connection = connections.find(c => c.server_id === serverId);

    const reload = async () => {
        const [receipts, nextServers, nextConnections] = await Promise.all([invoke<Receipt[]>('list_agent_plugins'), invoke<MCPServer[]>('get_mcp_servers'), invoke<AppConnection[]>('list_app_connections')]);
        setInstalled(receipts); setServers(nextServers); setConnections(nextConnections);
    };
    useDataRefresh({ events: ['mcp_state_changed', 'skills-registry-changed'], initial: false, refresh: async isCurrent => {
        const [s, c, r] = await Promise.all([invoke<MCPServer[]>('get_mcp_servers'), invoke<AppConnection[]>('list_app_connections'), invoke<Receipt[]>('list_agent_plugins')]);
        const t = serverId ? await invoke<MCPServerTool[]>('get_mcp_server_tools', { serverId }) : [];
        if (isCurrent()) { setServers(s); setConnections(c); setInstalled(r); setTools(t); }
    }, onError: e => setError(String(e)) });
    const run = async (label: string, action: () => Promise<void>) => {
        setBusy(label); setError('');
        try { await action(); } catch (e) { setError(String(e)); } finally { setBusy(''); }
    };
    const inspect = async (value: PluginSource) => {
        setUninstallRequested(false); setStdio(undefined); setClientId(''); setOauthMethod('none'); setRedirectUri(''); setAgentResult('');
        setDefinition(undefined); setEditingConnection(undefined); setServerId(undefined); setTools([]); setSource(value);
        await run('正在读取插件文件与技能…', async () => { setPreview(await invoke<Preview>('inspect_agent_plugin', { source: value })); });
    };
    useEffect(() => {
        let alive = true;
        void Promise.all([invoke<Receipt[]>('list_agent_plugins'), invoke<MCPServer[]>('get_mcp_servers'), invoke<AppConnection[]>('list_app_connections')]).then(([r, s, c]) => { if (alive) { setInstalled(r); setServers(s); setConnections(c); } }).catch(e => { if (alive) setError(String(e)); });
        return () => { alive = false; };
    }, []);
    useEffect(() => {
        let alive = true;
        if (initialSource) void run('正在读取插件…', async () => {
            const receipts = await invoke<Receipt[]>('list_agent_plugins');
            const local = receipts.find(r => r.preview.source.repo === initialSource.repo && r.preview.source.path === initialSource.path && r.preview.source.git_ref === initialSource.git_ref);
            if (!alive) return;
            setInstalled(receipts);
            setSource(initialSource);
            const next = local?.preview ?? await invoke<Preview>('inspect_agent_plugin', { source: initialSource });
            if (alive) setPreview(next);
        });
        return () => { alive = false; };
    }, [initialSource]);
    useEffect(() => {
        let alive = true; setTools([]);
        if (serverId) void invoke<MCPServerTool[]>('get_mcp_server_tools', { serverId }).then(t => { if (alive) setTools(t); }).catch(e => { if (alive) setError(String(e)); });
        return () => { alive = false; };
    }, [serverId]);
    const connect = async (server: PluginServer) => run('正在准备连接…', async () => {
        if (!preview) return;
        if (server.config.command) { setStdio(server); setStdioReconnect(undefined); setEnvironment('{}'); setDefinition(undefined); return; }
        setStdio(undefined);
        setEditingConnection(undefined); setFormGeneration(g => g + 1);
        setDefinition(await invoke<AppDefinition>('get_agent_plugin_connection', { key: preview.key, serverName: server.name, auth, clientId: clientId || null, oauthClientAuth: oauthMethod, oauthRedirectUri: redirectUri || null }));
    });
    const matching = current ? servers.filter(s => current.preview.servers.some(p => p.config.url && p.config.url === s.url) || connections.some(c => c.server_id === s.id && c.app_id.startsWith(`agent-plugin:${current.preview.key}:`))) : [];
    useEffect(() => {
        let alive = true;
        if (current && current.preview.agents?.length) void invoke<{id: number; name: string}[]>('get_agent_plugin_assistants', {key: current.preview.key, serverId: null}).then(rows => { if (alive) setTemplates(rows); }).catch(e => { if (alive) setError(String(e)); });
        return () => { alive = false; };
    }, [current?.preview.key]);

    const uninstallConfirmation = current && uninstallRequested && <section role="region" aria-label="确认卸载插件" className="rounded-lg border border-destructive/30 p-3"><p className="font-medium">卸载 {preview.name}？</p><p className="my-2 text-sm text-muted-foreground">删除插件文件、配套技能以及由插件创建的连接和凭据。复用的已有连接会保留。有本地修改时不会删除。</p><label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={revokeOnUninstall} onChange={e => setRevokeOnUninstall(e.target.checked)} />同时撤销服务商授权（网络或服务商不可用时可取消；仍会删除本地凭据）</label><div className="mt-3 flex gap-2"><Button variant="destructive" disabled={!!busy} onClick={() => void run('正在卸载插件…', async () => { await invoke('uninstall_agent_plugin', { key: preview.key, revokeRemote: revokeOnUninstall }); if (!embedded) setPreview(undefined); setManaging(false); setDefinition(undefined); setStdio(undefined); setEnvironment('{}'); setServerId(undefined); setTools([]); setUninstallRequested(false); setShowInstalled(true); onChanged(); await reload(); })}>确认卸载</Button><Button variant="outline" disabled={!!busy} onClick={() => setUninstallRequested(false)}>取消</Button></div></section>;

    const management = <div className="space-y-5 py-4">
        <p className="text-sm text-muted-foreground">从 GitHub 安装配套工具和 Skills。技能、参考资料与脚本按原目录保存；连接与助手授权分别管理。</p>
        {error && <p role="alert" className="whitespace-pre-wrap break-words rounded-lg border border-destructive/30 p-3 text-sm text-destructive">{error}</p>}
        {busy && <p role="status" className="text-sm text-muted-foreground">{busy}</p>}
        {preview ? <>
            {!embedded && <Button variant="ghost" disabled={!!busy} onClick={() => { setPreview(undefined); setDefinition(undefined); setServerId(undefined); }}><ArrowLeft className="mr-2 h-4 w-4" />返回插件目录</Button>}
            <div className="space-y-2"><div className="flex items-center gap-3">{preview.icon && <img src={preview.icon} alt="" className="h-12 w-12 rounded-xl object-contain" />}<h3 className="text-xl font-semibold">{preview.name}</h3></div><p className="text-sm text-muted-foreground">{preview.description}</p>
                <div className="flex flex-wrap gap-2"><Badge variant="outline">{preview.publisher || preview.source.repo}</Badge><Badge variant="outline">版本 {preview.version}</Badge><Badge variant="outline">{current ? '已安装' : '未安装'}</Badge></div>
                <p className="break-all text-xs text-muted-foreground">来源 {preview.source.repo}/{preview.source.path} · 提交 {preview.revision.slice(0, 12)}</p>
                <Button variant="link" className="px-0" onClick={() => void openUrl(`https://github.com/${preview.source.repo}/tree/${preview.revision}/${preview.source.path}`).catch(e => setError(String(e)))}>查看源文件<ExternalLink className="ml-2 h-4 w-4" /></Button>
            </div>
            <div className="flex flex-wrap gap-2">
                <Button disabled={!!busy || preview.locally_modified || current?.preview.digest === preview.digest} onClick={() => void run(current ? '正在更新插件…' : '正在安装插件…', async () => {
                    await installPlugin(preview);
                    await reload(); onChanged(); setPreview({ ...preview, installed_revision: preview.revision });
                })}><Download className="mr-2 h-4 w-4" />{current ? current.preview.digest === preview.digest ? '已是此版本' : '更新到预览版本' : '安装插件与技能'}</Button>
                {current && <Button variant="outline" disabled={!!busy} onClick={() => void inspect(preview.source)}><RefreshCw className="mr-2 h-4 w-4" />检查更新</Button>}
                {current && <Button variant="destructive" disabled={!!busy} onClick={() => { setUninstallRequested(true); setRevokeOnUninstall(true); }}>卸载插件</Button>}
                {current && <Button variant="outline" disabled={!!busy} onClick={() => void run('正在回退…', async () => { await invoke('rollback_agent_plugin', { key: preview.key }); await reload(); onChanged(); await inspect(preview.source); })}>回退上一版本</Button>}
            </div>
            {current && current.preview.digest !== preview.digest && <div className="rounded-lg border p-3 text-sm">已安装 {current.preview.version}（{current.preview.revision.slice(0, 12)}）→ {preview.version}（{preview.revision.slice(0, 12)}）。
                <p>新增技能：{preview.skills.filter(s => !current.preview.skills.some(i => i.id === s.id)).map(s => s.name).join('、') || '无'}；移除技能：{current.preview.skills.filter(s => !preview.skills.some(i => i.id === s.id)).map(s => s.name).join('、') || '无'}。</p><p>更新保留现有连接和授权；新增技能需要授权后使用。连接定义变化不会自动迁移凭据。</p></div>}
            {uninstallConfirmation}

            {preview.locally_modified && <p role="alert" className="text-sm text-destructive">检测到插件文件有本地修改，已阻止覆盖更新。请先备份并恢复原文件。</p>}
            {current && current.preview.digest !== preview.digest && <details className="rounded-lg border p-3"><summary className="cursor-pointer text-sm">查看更新文件差异</summary><ul className="mt-2 space-y-1 break-all text-xs">{preview.files.filter(p => !current.preview.files.includes(p) || preview.file_hashes?.[p] !== current.preview.file_hashes?.[p]).map(p => <li key={p}>{current.preview.files.includes(p) ? '修改' : '新增'} · {p}</li>)}{current.preview.files.filter(p => !preview.files.includes(p)).map(p => <li key={p}>删除 · {p}</li>)}</ul></details>}
            {!!preview.dependencies?.length && <section className="space-y-2 rounded-lg border p-3"><h4 className="font-medium">外部连接器依赖</h4>{preview.dependencies.map(d => <div key={d.connector_id} className="text-sm"><span>{d.name} · {d.source ? '仓库中有公开 MCP 接入包' : '仅提供平台连接器 ID，仓库没有公开接入地址'}</span>{d.source && <Button size="sm" variant="link" disabled={!!busy} onClick={() => void inspect(d.source!)}>查看并安装依赖</Button>}</div>)}</section>}
            {preview.notices.length > 0 && <section className="space-y-2 rounded-lg border p-3"><h4 className="text-sm font-medium">兼容性与运行条件</h4>{preview.notices.map(n => <p key={n} className="text-sm text-muted-foreground">{n}</p>)}</section>}
            <section className="space-y-3"><h4 className="font-medium">Skills 与命令 · {preview.skills.length}</h4>
                {!preview.skills.length && <p className="text-sm text-muted-foreground">这个插件没有声明可发现的 Skills。</p>}
                {preview.skills.map(skill => <details key={skill.id} className="rounded-lg border p-3"><summary className="cursor-pointer text-sm font-medium">{skill.kind === 'command' ? '/ ' : ''}{skill.name}<p className="mt-1 font-normal text-muted-foreground">{skill.description}</p></summary><pre className="mt-3 max-h-96 overflow-auto whitespace-pre-wrap break-words text-xs">{skill.content}</pre><p className="mt-2 text-xs text-muted-foreground">附属文件 {skill.files.length} 个</p></details>)}
            </section>
            <section className="space-y-3"><h4 className="font-medium">连接器 · {preview.servers.length}</h4>
                <p className="text-sm text-muted-foreground">仓库通常只声明连接器，没有实际工具清单。连接后检测工具，未检测不表示没有工具。</p>
                {preview.servers.map(server => <div key={server.name} className="space-y-2 rounded-lg border p-3"><p className="text-sm font-medium">{server.name}</p><p className="break-all text-xs text-muted-foreground">{server.config.url ?? [server.config.command, ...(server.config.args ?? [])].join(' ') ?? server.config.type}</p>{server.reason && <p className="text-sm text-destructive">{server.reason}</p>}{(!server.reason || server.reason.includes('OAuth')) && <Button size="sm" variant="outline" disabled={!current || current.preview.digest !== preview.digest || !!busy} onClick={() => void connect(server)}>添加连接</Button>}</div>)}
                {current && <label className="flex items-center gap-2 text-sm">新连接认证方式<select aria-label="新连接认证方式" className="rounded border bg-background p-2" value={auth} onChange={e => setAuth(e.target.value)}><option value="oauth">浏览器 OAuth</option><option value="none">服务不要求认证</option><option value="bearer">自行提供 Bearer Token</option><option value="headers">配置清单中的请求头</option></select></label>}
                {current && auth === 'oauth' && <><Input aria-label="OAuth 客户端 ID" placeholder="自己的 OAuth 客户端 ID（可选；不会复用私密客户端）" value={clientId} onChange={e => setClientId(e.target.value)} /><select aria-label="OAuth 客户端认证" className="w-full rounded border bg-background p-2" value={oauthMethod} onChange={e => setOauthMethod(e.target.value)}><option value="none">公共客户端（PKCE）</option><option value="client_secret_post">自有机密客户端 · Secret POST</option><option value="client_secret_basic">自有机密客户端 · Secret Basic</option></select><Input aria-label="OAuth 回调地址" placeholder="自有客户端注册的回调地址（可选，例如 http://127.0.0.1:3000/oauth/callback）" value={redirectUri} onChange={e => setRedirectUri(e.target.value)} /></>}
                {stdio && current && <div className="space-y-3 rounded-lg border p-3"><h4 className="font-medium">本地服务 · {stdio.name}</h4><p className="text-sm text-muted-foreground">启动时使用插件安装目录，参数直接传入进程。需要本机安装声明的运行时。环境变量保存在加密凭据库；重新连接需要重新填写。声明的变量：{Object.keys(stdio.config.env ?? {}).join('、') || '无'}。</p>
                    <textarea aria-label="插件环境变量 JSON" className="min-h-24 w-full rounded border bg-background p-2 font-mono text-sm" value={environment} onChange={e => setEnvironment(e.target.value)} spellCheck={false} />
                    <Button disabled={!!busy} onClick={() => void run('正在启动并检测本地插件…', async () => {
                        const values: unknown = JSON.parse(environment);
                        if (!values || Array.isArray(values) || typeof values !== 'object' || Object.values(values).some(v => typeof v !== 'string')) throw new Error('环境变量必须为字符串值的 JSON 对象');
                        const id = await invoke<number>('connect_agent_plugin_stdio', {key: current.preview.key, serverName: stdio.name, values, serverId: stdioReconnect ?? null});
                        setEnvironment('{}'); setServerId(id); await reload(); onChanged();
                        try { await invoke('verify_app_connection', {serverId: id}); setTools(await invoke<MCPServerTool[]>('get_mcp_server_tools', {serverId: id})); }
                        finally { await reload(); }
                    })}>启动并检测工具</Button></div>}
                {matching.map(server => <Button key={server.id} variant={serverId === server.id ? 'secondary' : 'outline'} className="mr-2" onClick={() => { setServerId(server.id); setStdio(undefined); setEnvironment('{}'); setDefinition(undefined); }}>{server.name} · 使用已有连接</Button>)}
                {definition && <AppConnectForm key={`${definition.id}:${editingConnection?.server_id ?? 'new'}:${formGeneration}`} app={definition} connection={editingConnection} onConnected={id => { setServerId(id); void reload().catch(e => setError(String(e))); onChanged(); }} />}
                {connection && <div className="space-y-2 rounded-lg border p-3"><p className="text-sm">{connection.label} · {connectionStatus(connection.status)}</p>{connection.error && <p className="text-sm text-destructive">{connection.error}</p>}<div className="flex flex-wrap gap-2">
                    {connection.status === 'connecting' && <Button size="sm" variant="outline" onClick={() => void run('正在打开授权页面…', async () => { const url = await invoke<string | null>('get_app_authorization_url', { serverId }); if (!url) throw new Error('授权已结束，请重新登录'); await openUrl(url); })}>继续授权</Button>}
                    <Button size="sm" variant="outline" disabled={!!busy} onClick={() => {
                        if (connection.definition.connection.transport === 'stdio') {
                            setStdio(current?.preview.servers.find(s => connection.app_id === `agent-plugin:${current.preview.key}:${s.name}`)); setStdioReconnect(connection.server_id); setEnvironment('{}'); setDefinition(undefined);
                        } else { setEditingConnection(connection); setDefinition(connection.definition); setFormGeneration(g => g + 1); }
                    }}>{connection.definition.connection.transport === 'stdio' ? '重新配置并连接' : '重新登录'}</Button>
                    <Button size="sm" variant="outline" disabled={!!busy || connection.status === 'disconnected'} onClick={() => void run('正在断开连接…', async () => { await invoke('disconnect_app_connection', { serverId, revokeRemote: true }); await reload(); onChanged(); })}>断开连接</Button>
                </div></div>}
                {serverId && <><div className="rounded-lg border p-3"><div className="flex items-center justify-between"><h4 className="font-medium">{tools.length || connection?.status === 'available' ? `检测到的工具 · ${tools.length}` : '工具尚未检测'}</h4><Button size="sm" variant="outline" disabled={!!busy || (connection && ['connecting', 'disconnected', 'auth_required'].includes(connection.status))} onClick={() => void run('正在刷新工具…', async () => { if (connection) await invoke('verify_app_connection', { serverId }); else await invoke('refresh_mcp_server_capabilities', { serverId }); setTools(await invoke<MCPServerTool[]>('get_mcp_server_tools', { serverId })); await reload(); onChanged(); })}>刷新工具</Button></div>{tools.map(t => <details key={t.id} className="border-b py-2"><summary className="cursor-pointer text-sm">{t.tool_name}</summary><p className="py-2 text-xs text-muted-foreground">{t.tool_description}</p></details>)}</div>
                    <AppAssistantAccess key={`${preview.key}:${serverId}`} serverId={serverId} pluginKey={current?.preview.key} />
                </>}
            </section>
            {!!preview.agents?.length && <section className="space-y-3"><h4 className="font-medium">子助手定义 · {preview.agents.length}</h4><p className="text-sm text-muted-foreground">选择一个已有助手，继承它的模型、执行通道和工具授权；使用插件的角色定义创建独立助手。创建后可在联系人中使用或参与协作。</p><select aria-label="子助手模板" className="w-full rounded border bg-background p-2" value={templateId} onChange={e => setTemplateId(e.target.value)}><option value="">选择模板助手</option>{templates.map(t => <option key={t.id} value={t.id}>{t.name}</option>)}</select>
                {preview.agents.map(agent => <details key={agent.path} className="rounded border p-3"><summary>{agent.name}</summary><pre className="my-2 max-h-64 overflow-auto whitespace-pre-wrap text-xs">{agent.content}</pre><Button disabled={!current || !templateId || !!busy} onClick={() => void run('正在创建插件助手…',async () => { const id = await invoke<number>('create_agent_plugin_assistant',{key: preview.key,agentPath: agent.path,templateId: Number(templateId)}); setAgentResult(`已创建 ${agent.name}（助手 ${id}），可在助手设置中调整并使用。`); onChanged(); })}>创建此助手</Button></details>)}{agentResult && <p role="status" className="text-sm">{agentResult}</p>}</section>}
            {current && !preview.servers.length && <AppAssistantAccess pluginKey={current.preview.key} />}
            <details className="rounded-lg border p-3"><summary className="cursor-pointer text-sm">完整包文件 · {preview.files.length}</summary><pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all text-xs">{preview.files.join('\n')}</pre></details>
        </> : <>
            <div className="flex flex-wrap gap-2"><Button variant={!showInstalled ? 'secondary' : 'outline'} onClick={() => setShowInstalled(false)}>GitHub 目录</Button><Button variant={showInstalled ? 'secondary' : 'outline'} onClick={() => setShowInstalled(true)}>已安装 {installed.length}</Button></div>
            <div className="space-y-3 rounded-lg border p-4"><h4 className="text-sm font-medium">GitHub 插件来源</h4>
                <Input aria-label="GitHub 仓库" value={source.repo} onChange={e => setSource({ ...source, repo: e.target.value })} placeholder="owner/repo" />
                <div className="flex flex-wrap gap-2"><Input className="min-w-32 flex-1" aria-label="Git 分支或标签" value={source.git_ref} onChange={e => setSource({ ...source, git_ref: e.target.value })} placeholder="main" /><Input className="min-w-48 flex-1" aria-label="插件目录" value={source.path} onChange={e => setSource({ ...source, path: e.target.value })} placeholder="plugins/notion" /></div>
                <div className="flex flex-wrap gap-2"><Button disabled={!!busy} onClick={() => void inspect(source)}>预览指定插件</Button><Button variant="outline" disabled={!!busy} onClick={() => void run('正在读取 GitHub 插件目录…', async () => { setCatalog(await invoke<PluginCatalog>('fetch_agent_plugin_catalog', { repo: source.repo, gitRef: source.git_ref })); })}>读取仓库插件目录</Button></div>
            </div>
            <Input aria-label="搜索插件" placeholder="搜索插件…" value={query} onChange={e => setQuery(e.target.value)} />
            {showInstalled ? installed.filter(r => `${r.preview.name} ${r.preview.description}`.toLowerCase().includes(query.toLowerCase())).map(r => <Button key={r.preview.key} variant="outline" className="h-auto w-full justify-start whitespace-normal p-4 text-left" onClick={() => { setUninstallRequested(false); setPreview(r.preview); setStdio(undefined); setEnvironment('{}'); setDefinition(undefined); setEditingConnection(undefined); setServerId(undefined); }}><Package className="mr-3 h-5 w-5 shrink-0" /><span>{r.preview.name}<span className="block text-xs text-muted-foreground">{r.preview.version} · {r.preview.skills.length} 个技能</span></span></Button>) : (catalog?.entries ?? [{ name: 'Notion', description: '预览 OpenAI GitHub 仓库中的 Notion 插件', category: '效率', source: notion }]).filter(e => `${e.name} ${e.description}`.toLowerCase().includes(query.toLowerCase())).map(entry => <Button key={`${entry.source.repo}/${entry.source.path}`} disabled={!!busy} variant="outline" className="h-auto w-full justify-start whitespace-normal p-4 text-left" onClick={() => void inspect(entry.source)}><Package className="mr-3 h-5 w-5 shrink-0" /><span>{entry.name}<span className="block text-xs text-muted-foreground">{entry.description || entry.category}</span></span></Button>)}
        </>}
    </div>;
    if (!embedded) return management;
    return <div className="space-y-9 py-2" data-testid="app-plugin-overview">
        <header className="flex flex-wrap items-center justify-end gap-4">
            <div className="flex items-center gap-2">
                {current && <DropdownMenu><DropdownMenuTrigger asChild><Button variant="ghost" size="icon" aria-label="更多插件操作" disabled={!!busy}><MoreHorizontal className="h-4 w-4" /></Button></DropdownMenuTrigger><DropdownMenuContent align="end"><DropdownMenuItem className="text-destructive" onSelect={() => { setUninstallRequested(true); setRevokeOnUninstall(true); }}>卸载插件</DropdownMenuItem></DropdownMenuContent></DropdownMenu>}
                <Button disabled={!!busy || !preview || (!current && preview.locally_modified)} onClick={() => {
                    if (current) { if (onConnect) onConnect(); else setManaging(true); return; }
                    void run('正在安装插件…', async () => {
                        if (!preview) return;
                        await installPlugin(preview);
                        await reload(); onChanged();
                        if (mounted.current) { if (onConnect) onConnect(); else setManaging(true); }
                    });
                }}>{!current && <Plus className="mr-2 h-4 w-4" />}{current ? hasConnection ? '查看连接' : '连接应用' : '安装插件'}</Button>
            </div>
        </header>
        {error && <div role="alert" className="space-y-2 rounded-lg border border-destructive/30 p-3 text-sm text-destructive"><p>{error}</p><Button variant="outline" size="sm" disabled={!!busy} onClick={() => initialSource && void inspect(initialSource)}>重试加载详情</Button></div>}
        {busy && <p role="status" className="text-sm text-muted-foreground">{busy}</p>}
        <p className="text-sm leading-7">{preview?.description || app?.description}</p>
        {!preview && !error && <div aria-label="正在加载应用详情" className="space-y-6 animate-pulse"><div className="h-20 rounded-lg bg-muted" /><div className="h-48 rounded-lg bg-muted" /></div>}
        {preview && <>
            <section><h3 className="border-b pb-3 text-sm font-medium">应用 <span className="text-muted-foreground">{preview.servers.length}</span></h3><div className="divide-y">{preview.servers.map(server => <div key={server.name} className="flex items-start gap-3 py-5"><Plug className="mt-1 h-5 w-5 shrink-0 text-muted-foreground" /><div className="min-w-0 flex-1"><p className="text-sm font-medium">{preview.servers.length === 1 ? app?.name ?? preview.name : server.name}</p><p className="mt-1 text-xs leading-5 text-muted-foreground">{server.reason || (server.config.command ? '提供本地运行的工具' : app?.description || '连接服务后使用配套工具')}</p></div></div>)}</div></section>
            <section><h3 className="border-b pb-3 text-sm font-medium">Skills <span className="text-muted-foreground">{preview.skills.length}</span></h3>
                {!preview.skills.length && <p className="py-5 text-sm text-muted-foreground">此插件未提供配套技能。</p>}
                {(expandedSkills ? preview.skills : preview.skills.slice(0, 5)).map(skill => <details key={skill.id} className="group py-4"><summary className="flex cursor-pointer list-none items-start gap-3"><Blocks className="mt-1 h-5 w-5 shrink-0 text-muted-foreground" /><div className="min-w-0"><p className="text-sm font-medium">{skill.name}</p><p className="mt-1 text-xs leading-5 text-muted-foreground">{skill.description}</p></div></summary><pre className="mt-4 max-h-80 overflow-auto whitespace-pre-wrap rounded-lg bg-muted/40 p-4 text-xs">{skill.content}</pre></details>)}
                {preview.skills.length > 5 && <Button variant="ghost" size="sm" className="text-muted-foreground" onClick={() => setExpandedSkills(!expandedSkills)}><ChevronDown className="mr-2 h-4 w-4" />{expandedSkills ? '收起' : `查看另外 ${preview.skills.length - 5} 个技能`}</Button>}
            </section>
            <section><h3 className="border-b pb-3 text-sm font-medium">信息</h3><dl className="grid grid-cols-[6rem_1fr] gap-x-5 gap-y-4 py-5 text-sm"><dt className="text-muted-foreground">版本</dt><dd>{preview.version}{current && ' · 已安装'}</dd><dt className="text-muted-foreground">发布者</dt><dd>{preview.publisher || preview.source.repo}</dd><dt className="text-muted-foreground">工具</dt><dd>连接后检测实际可用工具</dd><dt className="text-muted-foreground">来源</dt><dd>AIPP 应用目录</dd></dl>{current && <Button variant="outline" size="sm" disabled={!!busy} onClick={() => void inspect(preview.source)}><RefreshCw className="mr-2 h-4 w-4" />检查更新</Button>}{current && current.preview.digest !== preview.digest && <Button className="ml-2" size="sm" disabled={!!busy || preview.locally_modified} onClick={() => void run('正在更新插件…', async () => { await installPlugin(preview); await reload(); onChanged(); })}>更新插件</Button>}</section>
            {preview.notices.length > 0 && <details className="text-sm"><summary className="cursor-pointer text-muted-foreground">使用条件与兼容性</summary><div className="space-y-2 pt-3">{preview.notices.map(n => <p key={n} className="text-muted-foreground">{n}</p>)}</div></details>}
        </>}
        {uninstallConfirmation}
        {!onConnect && managing && <section aria-label="插件连接管理">{management}</section>}
    </div>;

}
