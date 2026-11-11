import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Boxes, Code2, Globe, Notebook, Search, RefreshCw, Plug, Wrench, ArrowUpRight, MoreHorizontal } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { AppConnection, AppDefinition, CatalogResponse, connectionStatus } from '@/data/ConnectedApp';
import { MCPServer, MCPServerRequest, MCPServerTool } from '@/data/MCP';
import { useDataRefresh } from '@/hooks/useDataRefresh';
import { useIsMobilePlatform } from '@/hooks/use-platform';
import { isMobileExecutableMcpServer, visibleSearchTools } from '@/lib/mobileUnsupported';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription } from '@/components/ui/sheet';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import MCPConfig from './MCPConfig';
import ConfirmDialog from '../ConfirmDialog';
import MCPServerDialog from './MCPServerDialog';
import JSONImportDialog from './JSONImportDialog';
import BuiltinToolDialog from './BuiltinToolDialog';
import AppConnectForm from './apps/AppConnectForm';
import AppAssistantAccess from './apps/AppAssistantAccess';
import AppRelatedSkills from './apps/AppRelatedSkills';
import AgentPluginsConfig, { PluginSource, PluginCatalog } from './apps/AgentPluginsConfig';
import AppTransportSettings from './apps/AppTransportSettings';
import { getQuickStartTemplates } from '@/data/MCPTemplates';

interface Builtin { id: string; name: string; description: string; command: string }
type Selection = { kind: 'app'; appId: string } | { kind: 'server'; serverId: number } | { kind: 'plugin'; source: PluginSource; name: string };

function AppIcon({ icon, builtin = false, official }: { icon?: string | null; builtin?: boolean; official?: {icon?: string | null; icon_dark?: string | null; icon_small?: string | null} }) {
    const [failed, setFailed] = useState<string[]>([]);
    const Icon = builtin ? Wrench : icon === 'github' ? Code2 : icon === 'notebook' ? Notebook : Globe;
    const image = (url?: string | null) => {
        const trusted = url?.startsWith('https://aipp-helper.xiejingyang.com/agent-plugin-icons/');
        return trusted && url && !failed.includes(url) ? <img src={url} alt="" className="h-7 w-7 object-contain" onError={() => setFailed(previous => [...previous, url])} /> : <Icon className="h-6 w-6 text-muted-foreground" />;
    };
    return <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl border bg-muted/30">
        <span className="dark:hidden">{image(official?.icon ?? official?.icon_small)}</span>
        <span className="hidden dark:block">{image(official?.icon_dark ?? official?.icon ?? official?.icon_small)}</span>
    </span>;
}

export default function ConnectedAppsConfig() {
    const mobile = useIsMobilePlatform();
    const [page, setPage] = useState('catalog');
    const [query, setQuery] = useState('');
    const [category, setCategory] = useState('全部');
    const [catalog, setCatalog] = useState<CatalogResponse>();
    const [catalogBusy, setCatalogBusy] = useState(false);
    const [servers, setServers] = useState<MCPServer[]>([]);
    const [connections, setConnections] = useState<AppConnection[]>([]);
    const [builtins, setBuiltins] = useState<Builtin[]>([]);
    const [tools, setTools] = useState<Map<number, MCPServerTool[]>>(new Map());
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState('');
    const [selection, setSelection] = useState<Selection>();
    const [detailTab, setDetailTab] = useState('overview');
    const [configOpen, setConfigOpen] = useState(false);
    const [importOpen, setImportOpen] = useState(false);
    const [initialConfig, setInitialConfig] = useState<Partial<MCPServerRequest>>();
    const [builtinId, setBuiltinId] = useState<string>();
    const [busyId, setBusyId] = useState<number>();
    const [formGeneration, setFormGeneration] = useState(0);
    const [configuringConnection, setConfiguringConnection] = useState(false);
    const [pluginsOpen, setPluginsOpen] = useState(false);
    const [pluginCatalog, setPluginCatalog] = useState<PluginCatalog>();
    const [pluginCatalogError, setPluginCatalogError] = useState('');
    const [pluginSource, setPluginSource] = useState<PluginSource>();
    const [removingServer, setRemovingServer] = useState<MCPServer>();

    const refresh = useCallback(async (isCurrent: () => boolean = () => true) => {
        const [nextServers, nextConnections, nextBuiltins] = await Promise.all([
            invoke<MCPServer[]>('get_mcp_servers'), invoke<AppConnection[]>('list_app_connections'), invoke<Builtin[]>('list_aipp_builtin_templates'),
        ]);
        const entries = await Promise.all(nextServers.map(async server => [server.id,
            visibleSearchTools(await invoke<MCPServerTool[]>('get_mcp_server_tools', { serverId: server.id }), server.command, mobile)] as const));
        if (!isCurrent()) return;
        setServers(nextServers); setConnections(nextConnections); setBuiltins(nextBuiltins); setTools(new Map(entries));
        setError(''); setLoading(false);
    }, [mobile]);
    useDataRefresh({ events: ['mcp_state_changed'], initial: true, refresh,
        onError: e => { setLoading(false); setError(String(e)); } });

    const loadCatalog = useCallback(async () => {
        setCatalogBusy(true);
        try { setCatalog(await invoke<CatalogResponse>('fetch_app_catalog')); }
        catch (e) { setCatalog(old => ({ catalog: old?.catalog ?? null, fetched_at: old?.fetched_at ?? null, warning: String(e) })); }
        finally { setCatalogBusy(false); }
    }, []);
    useEffect(() => { void loadCatalog(); }, [loadCatalog]);
    useEffect(() => {
        let alive = true;
        if (!mobile) void invoke<PluginCatalog>('fetch_agent_plugin_catalog', {repo:'openai/plugins',gitRef:'main'}).then(value => { if (!Array.isArray(value?.entries)) throw new Error('应用详情目录格式无效'); if (alive) setPluginCatalog(value); }).catch(e => { if (alive) setPluginCatalogError(String(e)); });
        return () => { alive = false; };
    }, [mobile]);

    const catalogApp = selection?.kind === 'app' ? catalog?.catalog?.apps.find(a => a.id === selection.appId) : undefined;
    const requestedSource = selection?.kind === 'plugin' ? selection.source : catalogApp?.plugin_source;
    const sourceMatches = (a?: PluginSource | null, b?: PluginSource | null) => !!a && !!b && a.repo.toLowerCase() === b.repo.toLowerCase() && a.path === b.path;
    const matchingConnections = connections.filter(c => (catalogApp && c.app_id === catalogApp.id) || sourceMatches(c.definition.plugin_source, requestedSource));
    // A catalog entry and its sole local connection share the same detail view.
    const selectedServerId = selection?.kind === 'server' ? selection.serverId : matchingConnections.length === 1 ? matchingConnections[0].server_id : undefined;
    const selectedServer = servers.find(s => s.id === selectedServerId);
    const selectedConnection = connections.find(c => c.server_id === selectedServerId);
    const selectedApp = selectedConnection?.definition ?? catalogApp;
    const selectedPluginSource = requestedSource ?? selectedApp?.plugin_source ?? catalog?.catalog?.apps.find(a => a.id === selectedApp?.id)?.plugin_source;
    const officialIcon = (source?: PluginSource | null) => pluginCatalog?.entries.find(entry => sourceMatches(entry.source, source));
    const pluginOverview = !mobile && !!selectedPluginSource;
    const filteredApps = useMemo(() => (catalog?.catalog?.apps ?? []).filter(app =>
        (category === '全部' || category === app.category) && `${app.name} ${app.description} ${app.publisher}`.toLowerCase().includes(query.toLowerCase())), [catalog, category, query]);
    const filteredBuiltins = builtins.filter(b => (category === '全部' || category === '内置工具') && `${b.name} ${b.description}`.toLowerCase().includes(query.toLowerCase()));
    const filteredServers = servers.filter(server => {
        const connection = connections.find(c => c.server_id === server.id);
        const type = server.is_builtin ? '内置工具' : connection ? '外部服务' : '自定义 MCP';
        return (category === '全部' || category === type) && `${server.name} ${server.description} ${connection?.label ?? ''}`.toLowerCase().includes(query.toLowerCase());
    });
    const filteredPlugins = (pluginCatalog?.entries ?? []).filter(entry => !(catalog?.catalog?.apps ?? []).some(a => sourceMatches(a.plugin_source, entry.source)) && (category === '全部' || category === entry.category) && `${entry.name} ${entry.description}`.toLowerCase().includes(query.toLowerCase()));
    const categories = page === 'catalog' ? ['全部', '内置工具', ...new Set([...(catalog?.catalog?.apps ?? []).map(app => app.category), ...(pluginCatalog?.entries ?? []).map(entry => entry.category)])] : ['全部', '内置工具', '外部服务', '自定义 MCP'];

    const select = (value: Selection) => { setSelection(value); setConfiguringConnection(false); setDetailTab('overview'); setFormGeneration(v => v + 1); };
    const action = async (id: number, command: string, revokeRemote = true) => {
        setBusyId(id);
        try {
            const result = await invoke<unknown>(command, { serverId: id, revokeRemote });
            if (typeof result === 'string') toast.success(result);
            await refresh();
        }
        catch (e) { toast.error(String(e)); await refresh().catch(() => {}); }
        finally { setBusyId(undefined); }
    };
    const onConnected = (id: number) => { void refresh().then(() => { setSelection({ kind: 'server', serverId: id }); setDetailTab('overview'); }).catch(e => setError(String(e))); };
    const importConfigs = async (configs: MCPServerRequest[]) => {
        setImportOpen(false);
        if (configs.length === 1) { setInitialConfig(configs[0]); setConfigOpen(true); return; }
        try {
            for (const request of configs) await invoke('add_mcp_server', { request: { ...request, is_enabled: false } });
            await refresh(); setPage('installed'); setCategory('全部');
            toast.success('已导入配置，保持停用；请检查后启用并测试连接。');
        } catch (e) { toast.error(`导入未全部完成：${String(e)}`); await refresh().catch(() => {}); }
    };
    const platformSupported = (app: AppDefinition) => !mobile || app.platforms.includes('android');

    return <div className="h-full overflow-y-auto bg-background">
        <div className="mx-auto max-w-6xl space-y-6 p-4 sm:p-8">
            <div className="flex items-center justify-between gap-3">
                <div><h2 className="text-xl font-semibold">应用与工具</h2><p className="mt-1 text-sm text-muted-foreground">连接外部服务，管理助手可以使用的工具。</p></div>
                <DropdownMenu><DropdownMenuTrigger asChild><Button variant="outline">添加</Button></DropdownMenuTrigger><DropdownMenuContent align="end">
                    <DropdownMenuItem onSelect={() => { setInitialConfig(undefined); setConfigOpen(true); }}>自定义 MCP</DropdownMenuItem>
                    {getQuickStartTemplates().filter(template => !mobile || isMobileExecutableMcpServer(template.template)).map(template =>
                        <DropdownMenuItem key={template.id} onSelect={() => { setInitialConfig(template.template); setConfigOpen(true); }}>{template.name} 模板</DropdownMenuItem>)}
                    {!mobile && <DropdownMenuItem onSelect={() => { setPluginSource(undefined); setPluginsOpen(true); }}>高级插件来源管理</DropdownMenuItem>}
                    <DropdownMenuItem onSelect={() => setImportOpen(true)}>从 JSON 导入</DropdownMenuItem>
                </DropdownMenuContent></DropdownMenu>
            </div>
            <Tabs value={page} onValueChange={value => { setPage(value); setCategory('全部'); }}>
                <TabsList><TabsTrigger value="catalog">应用目录</TabsTrigger><TabsTrigger value="installed">我的应用 {servers.length || ''}</TabsTrigger></TabsList>
            </Tabs>
            <div className="flex gap-2"><div className="relative flex-1"><Search className="absolute left-3 top-3 h-4 w-4 text-muted-foreground" /><Input className="pl-9" aria-label="搜索应用与工具" placeholder="搜索应用、工具或用途…" value={query} onChange={e => setQuery(e.target.value)} /></div>
                <Button variant="ghost" size="icon" aria-label="刷新应用目录和连接" disabled={catalogBusy} onClick={() => { void loadCatalog(); void refresh().catch(e => setError(String(e))); }}><RefreshCw className={`h-4 w-4 ${catalogBusy ? 'animate-spin' : ''}`} /></Button>
            </div>
            <div className="flex flex-wrap gap-2">{categories.map(item => <Button key={item} size="sm" variant={category === item ? 'secondary' : 'ghost'} className="rounded-full" onClick={() => setCategory(item)}>{item}</Button>)}</div>
            {error && <p role="alert" className="rounded-lg border border-destructive/30 p-3 text-sm text-destructive">{error}</p>}
            {page === 'catalog' ? <>
                {catalog?.warning && <div role="status" className="rounded-lg border p-3 text-sm text-muted-foreground">{catalog.warning}<br />{catalog.catalog ? `正在显示缓存目录（${catalog.fetched_at}），现有连接仍可管理。` : '外部应用目录暂不可用，内置工具和现有连接仍可管理。'}</div>}
                {filteredBuiltins.length > 0 && <section className="space-y-3"><h3 className="text-sm font-medium">AIPP 内置工具</h3><div className="grid gap-3 md:grid-cols-2">{filteredBuiltins.map(builtin => {
                    const server = servers.find(s => s.is_builtin && s.command === builtin.command);
                    return <button key={builtin.id} className="flex items-center gap-4 rounded-xl border p-4 text-left transition-colors hover:bg-muted/40" onClick={() => server ? select({ kind: 'server', serverId: server.id }) : setBuiltinId(builtin.id)}>
                        <AppIcon builtin /><div className="min-w-0 flex-1"><div className="font-medium">{builtin.name} <span className="text-xs text-muted-foreground">内置</span></div><p className="mt-1 line-clamp-2 text-sm text-muted-foreground">{builtin.description}</p></div><span className="shrink-0 text-sm">{server ? '管理' : '配置'}</span>
                    </button>;
                })}</div></section>}
                <section className="space-y-3">{(filteredApps.length > 0 || (!mobile && filteredPlugins.length > 0)) && <h3 className="text-sm font-medium">应用与配套技能</h3>}<div className="grid gap-3 md:grid-cols-2">{filteredApps.map(app => {
                    const appConnections = connections.filter(c => c.app_id === app.id || sourceMatches(c.definition.plugin_source, app.plugin_source));
                    const count = appConnections.length;
                    return <button key={app.id} className="flex items-center gap-4 rounded-xl border p-4 text-left transition-colors hover:bg-muted/40" onClick={() => select({ kind: 'app', appId: app.id })}>
                        <AppIcon icon={app.icon} official={officialIcon(app.plugin_source)} /><div className="min-w-0 flex-1"><div className="font-medium">{app.name}</div><p className="mt-1 line-clamp-2 text-sm text-muted-foreground">{app.description}</p><p className="mt-2 text-xs text-muted-foreground">{app.publisher} · {app.category}</p></div><span className="shrink-0 text-sm">{count === 1 ? connectionStatus(appConnections[0].status) : count ? `${count} 个连接` : '连接'}</span>
                    </button>;
                })}
                {!mobile && filteredPlugins.map(entry => <button key={entry.source.path} className="flex items-center gap-4 rounded-xl border p-4 text-left transition-colors hover:bg-muted/40" onClick={() => select({kind:'plugin',source:entry.source,name:entry.name})}><AppIcon official={entry} /><div className="min-w-0 flex-1"><div className="font-medium">{entry.name}</div><p className="mt-1 line-clamp-2 text-sm text-muted-foreground">{entry.description}</p></div></button>)}
                </div>{!mobile && pluginCatalogError && <p role="status" className="text-xs text-muted-foreground">应用详情目录暂不可用：{pluginCatalogError}</p>}</section>
                {loading && <p className="py-8 text-center text-sm text-muted-foreground">正在读取应用…</p>}
                {!loading && !catalogBusy && !filteredBuiltins.length && !filteredApps.length && (mobile || !filteredPlugins.length) && <p className="py-8 text-center text-sm text-muted-foreground">没有匹配的应用。</p>}
            </> : <div className="divide-y rounded-xl border">
                {loading && <p className="p-6 text-sm text-muted-foreground">正在读取应用…</p>}
                {!loading && !filteredServers.length && <p className="p-6 text-sm text-muted-foreground">没有匹配的连接，可从应用目录或添加菜单开始。</p>}
                {filteredServers.map(server => {
                    const connection = connections.find(c => c.server_id === server.id);
                    const serverTools = tools.get(server.id);
                    const supported = !mobile || isMobileExecutableMcpServer(server);
                    return <button key={server.id} className="flex w-full items-center gap-3 p-4 text-left hover:bg-muted/30" onClick={() => select({ kind: 'server', serverId: server.id })}>
                        <AppIcon builtin={server.is_builtin} icon={connection?.definition.icon} official={officialIcon(connection?.definition.plugin_source)} /><div className="min-w-0 flex-1"><div className="truncate font-medium">{connection?.label ?? server.name}</div><p className="truncate text-xs text-muted-foreground">{server.is_builtin ? '内置工具' : connection ? connection.definition.name : `自定义 MCP · ${server.transport_type}`}</p></div>
                        <div className="text-right"><p className="text-sm">{!supported ? '此设备不可执行' : connection && ['connecting', 'auth_required', 'disconnected'].includes(connection.status) ? connectionStatus(connection.status) : !server.is_enabled ? '已停用' : connection ? connectionStatus(connection.status) : server.is_builtin ? '已启用' : '待验证'}</p><p className="mt-1 text-xs text-muted-foreground">{serverTools?.length || server.is_builtin || connection?.status === 'available' ? `${serverTools?.filter(t => t.is_enabled).length ?? 0} / ${serverTools?.length ?? 0} 个工具` : '工具未检测'}</p></div>
                    </button>;
                })}
            </div>}
        </div>
        <Sheet open={!!selection} onOpenChange={open => { if (!open) { setSelection(undefined); void refresh().catch(e => setError(String(e))); } }}>
            <SheetContent className="w-full overflow-y-auto sm:max-w-3xl">
                <SheetHeader><div className="flex items-center gap-3 pr-8"><AppIcon builtin={selectedServer?.is_builtin} icon={selectedApp?.icon} official={officialIcon(selectedPluginSource)} /><div><SheetTitle>{selectedConnection?.label ?? selectedApp?.name ?? selectedServer?.name ?? (selection?.kind === 'plugin' ? selection.name : '应用详情')}</SheetTitle><SheetDescription>{selectedApp?.description ?? selectedServer?.description ?? '连接配置已移除'}</SheetDescription>{selectedConnection && <p className="mt-2 text-xs text-muted-foreground">{['connecting', 'auth_required', 'disconnected'].includes(selectedConnection.status) ? connectionStatus(selectedConnection.status) : !selectedServer?.is_enabled ? '已停用' : connectionStatus(selectedConnection.status)}</p>}</div>{selectedServer?.is_deletable && <DropdownMenu><DropdownMenuTrigger asChild><Button variant="ghost" size="icon" aria-label="更多连接操作" className="ml-auto shrink-0"><MoreHorizontal className="h-4 w-4" /></Button></DropdownMenuTrigger><DropdownMenuContent align="end"><DropdownMenuItem className="text-destructive" disabled={busyId !== undefined} onSelect={() => setRemovingServer(selectedServer)}>删除连接</DropdownMenuItem></DropdownMenuContent></DropdownMenu>}</div></SheetHeader>
                <div className="space-y-5 px-5 pb-8">
                    {!selectedConnection && matchingConnections.length > 1 && <div className="space-y-2"><p className="text-sm text-muted-foreground">选择要管理的连接</p>{matchingConnections.map(connection => <Button key={connection.server_id} variant="outline" className="w-full justify-between" onClick={() => select({kind:'server',serverId:connection.server_id})}>{connection.label}<span>{connectionStatus(connection.status)}</span></Button>)}</div>}
                    <Tabs value={detailTab} onValueChange={setDetailTab}>
                        <TabsList><TabsTrigger value="overview">概览</TabsTrigger>{selectedServer && <TabsTrigger value="tools">工具与管理</TabsTrigger>}{selectedServer && !selectedServer.is_builtin && <TabsTrigger value="access">助手授权</TabsTrigger>}{selectedApp && <TabsTrigger value="settings">{selectedServer ? '连接配置' : '连接'}</TabsTrigger>}</TabsList>
                        <TabsContent value="overview" className="space-y-5 pt-4">
                            {pluginOverview && <AgentPluginsConfig key={JSON.stringify(selectedPluginSource)} embedded hasConnection={!!selectedConnection || matchingConnections.length > 0} initialSource={selectedPluginSource!} app={selectedApp} onConnect={selectedApp ? () => { setDetailTab('settings'); } : undefined} onChanged={() => void refresh().catch(e => setError(String(e)))} />}

                            {!pluginOverview && selectedApp && <><p className="text-sm">{selectedApp.notice}</p><p className="text-sm text-muted-foreground">{selectedApp.connection.auth === 'oauth' ? `请求的授权范围：${selectedApp.connection.scopes?.join('、') || '由服务商决定'}。实际权限以授权页面为准。` : selectedApp.connection.auth === 'fields' ? '实际访问权限由你提供的密钥或令牌决定。' : '此应用不要求登录。'}</p><AppRelatedSkills key={selectedApp.id} ids={selectedApp.skill_ids ?? []} /><p className="text-xs text-muted-foreground">{selectedApp.publisher} · 版本 {selectedApp.version} · {selectedApp.category}</p>{selectedApp.homepage?.startsWith('https://') && <Button variant="link" className="px-0" onClick={() => void openUrl(selectedApp.homepage!).catch(e => toast.error(String(e)))}>服务说明 <ArrowUpRight className="ml-1 h-4 w-4" /></Button>}

                                {!selectedServer && <Button disabled={!platformSupported(selectedApp)} onClick={() => setDetailTab('settings')}><Plug className="mr-2 h-4 w-4" />{matchingConnections.length ? '添加另一个连接' : '连接应用'}</Button>}
                                {!platformSupported(selectedApp) && <p className="text-sm text-muted-foreground">此应用尚未支持当前设备的安全凭据存储，暂不能在此设备建立认证连接。</p>}</>}
                            {selectedServer && <><div className="flex items-center gap-2 text-sm"><Boxes className="h-4 w-4 text-muted-foreground" />{tools.get(selectedServer.id)?.length || selectedServer.is_builtin || selectedConnection?.status === 'available' ? `${tools.get(selectedServer.id)?.filter(t => t.is_enabled).length ?? 0} / ${tools.get(selectedServer.id)?.length ?? 0} 个工具已启用` : '工具尚未检测，连接验证后显示'}</div>{selectedServer.is_builtin && <p className="text-sm text-muted-foreground">内置工具由 AIPP 提供。工具适用场景、平台限制和系统保护继续生效；总管家专用能力不会向普通助手开放。</p>}</>}
                        </TabsContent>
                        {selectedServer && <TabsContent value="tools" className="pt-4"><MCPConfig key={selectedServer.id} initialServerId={selectedServer.id} detailOnly onEditManaged={selectedConnection ? () => setDetailTab('settings') : undefined} /></TabsContent>}
                        {selectedServer && <TabsContent value="access" className="pt-4"><AppAssistantAccess serverId={selectedServer.id} /></TabsContent>}
                        {selectedApp && <TabsContent value="settings" className="space-y-6 pt-4">
                            {selectedConnection && <div className="space-y-3">
                                <div className="flex flex-wrap gap-2">
                                    {selectedConnection.status === 'connecting' ? <Button size="sm" onClick={() => void invoke<string | null>('get_app_authorization_url', {serverId:selectedConnection.server_id}).then(url => { if (!url) throw new Error('授权已结束，请重新连接'); return openUrl(url); }).catch(e => toast.error(String(e)))}>继续授权</Button> : !['disconnected','auth_required'].includes(selectedConnection.status) && <Button size="sm" variant="outline" disabled={busyId !== undefined || !selectedServer?.is_enabled} onClick={() => void action(selectedConnection.server_id, 'verify_app_connection')}>测试连接</Button>}
                                    {!configuringConnection && <Button size="sm" variant="outline" onClick={() => { setConfiguringConnection(true); setFormGeneration(v => v + 1); }}>重新配置</Button>}
                                </div>
                                {selectedConnection.error && <p role="alert" className="break-words text-sm text-destructive">{selectedConnection.error}</p>}
                            </div>}

                            {(!selectedConnection || configuringConnection) && (selectedApp.connection.transport === 'stdio' ? <Button variant="outline" onClick={() => { void invoke<{preview: {key: string; source: PluginSource}}[]>('list_agent_plugins').then(items => { const found = items.find(i => selectedApp.id.startsWith(`agent-plugin:${i.preview.key}:`)); if (!found) throw new Error('插件已卸载，请从 GitHub 插件重新安装'); setPluginSource(found.preview.source); setPluginsOpen(true); }).catch(e => setError(String(e))); }}>管理插件本地连接</Button> : platformSupported(selectedApp) ? <AppConnectForm key={`${selectedApp.id}-${selectedServer?.id ?? 'new'}-${formGeneration}`} app={selectedApp} connection={selectedConnection} onConnected={onConnected} /> : <p className="text-sm text-muted-foreground">当前平台不支持此应用的认证连接。</p>)}
                            {selectedConnection && selectedServer && <AppTransportSettings key={`${selectedServer.id}-${selectedServer.timeout}-${selectedServer.proxy_enabled}`} server={selectedServer} onSaved={() => { toast.success('网络设置已保存'); void refresh().catch(e => setError(String(e))); }} />}
                            {selectedConnection?.credential_status && <p className="text-xs text-muted-foreground">{selectedConnection.credential_status}</p>}
                            {selectedConnection && <div className="space-y-2 border-t pt-5"><Button variant="outline" disabled={busyId !== undefined} onClick={() => void action(selectedConnection.server_id, 'disconnect_app_connection')}>断开连接</Button>{selectedConnection.status === 'disconnected' && selectedConnection.error && <Button variant="ghost" disabled={busyId !== undefined} onClick={() => void action(selectedConnection.server_id, 'disconnect_app_connection', false)}>删除凭据，不撤销授权</Button>}<p className="text-xs text-muted-foreground">断开会停用连接、删除凭据，并尝试撤销服务商支持的 OAuth 授权。启用同步时，连接断开和凭据删除会同步到其他设备；跳过撤销只表示保留服务商侧授权，不代表仅影响本机。</p></div>}
                        </TabsContent>}
                    </Tabs>
                </div>
            </SheetContent>
        </Sheet>
        <ConfirmDialog isOpen={!!removingServer} title="删除连接" confirmText={`删除 ${removingServer?.name ?? ''} 的连接、工具配置和凭据。GitHub 插件包和复用此服务的其他连接不会被卸载。`} onCancel={() => { if (busyId === undefined) setRemovingServer(undefined); }} onConfirm={() => {
            if (!removingServer || busyId !== undefined) return;
            const id = removingServer.id; setBusyId(id);
            void invoke('delete_mcp_server', { id }).then(async () => { setRemovingServer(undefined); setSelection(undefined); await refresh(); toast.success('连接已删除'); }).catch(e => { setRemovingServer(undefined); setError(String(e)); toast.error(String(e)); }).finally(() => setBusyId(undefined));
        }} />
        <MCPServerDialog isOpen={configOpen} initialConfig={initialConfig} onClose={() => setConfigOpen(false)} onSubmit={() => { setConfigOpen(false); setPage('installed'); setCategory('全部'); void refresh().catch(e => setError(String(e))); }} />
        <JSONImportDialog isOpen={importOpen} onClose={() => setImportOpen(false)} onImport={configs => void importConfigs(configs)} />
        <Sheet open={pluginsOpen} onOpenChange={setPluginsOpen}><SheetContent className="w-full overflow-y-auto sm:max-w-3xl"><SheetHeader><SheetTitle>GitHub 插件</SheetTitle><SheetDescription>高级来源管理；应用详情与安装请从应用目录进入。</SheetDescription></SheetHeader>{pluginsOpen && <div className="px-5 pb-8"><AgentPluginsConfig initialSource={pluginSource} onChanged={() => void refresh().catch(e => setError(String(e)))} /></div>}</SheetContent></Sheet>
        <BuiltinToolDialog isOpen={!!builtinId} initialTemplateId={builtinId} onClose={() => setBuiltinId(undefined)} onSubmit={() => { setBuiltinId(undefined); void refresh().catch(e => setError(String(e))); }} />
    </div>;
}
