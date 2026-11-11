import { ReactNode, useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ChevronRight, CloudOff, Plus, RefreshCw, Search } from 'lucide-react';
import { toast } from 'sonner';
import { AppConnection, AppDefinition, CatalogResponse } from '@/data/ConnectedApp';
import { MCPServer, MCPServerRequest, MCPServerTool } from '@/data/MCP';
import { useDataRefresh } from '@/hooks/useDataRefresh';
import { useIsMobilePlatform } from '@/hooks/use-platform';
import { isMobileExecutableMcpServer, visibleSearchTools } from '@/lib/mobileUnsupported';
import { cn } from '@/utils/utils';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription } from '@/components/ui/sheet';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import MCPServerDialog from './MCPServerDialog';
import JSONImportDialog from './JSONImportDialog';
import BuiltinToolDialog from './BuiltinToolDialog';
import AgentPluginsConfig, { PluginSource, PluginCatalog } from './apps/AgentPluginsConfig';
import AppDetailSheet, { Selection, sourceMatches } from './apps/AppDetailSheet';
import AppIcon, { OfficialIcon } from './apps/AppIcon';
import AppStatusBadge from './apps/AppStatusBadge';
import { aggregateStatus, DisplayStatus, displayStatus, toolSummary } from './apps/appStatus';
import { getQuickStartTemplates } from '@/data/MCPTemplates';

interface Builtin { id: string; name: string; description: string; command: string }

const ALL = '全部';
const BUILTIN = '内置工具';

function AppCard({ name, meta, description, badge, status, idle, action, icon, onClick }: {
    name: string; meta?: string; description: string; badge?: string; status?: DisplayStatus; idle?: string; action: string; icon: ReactNode; onClick: () => void;
}) {
    return <button type="button" onClick={onClick}
        className="group flex h-full flex-col gap-3 rounded-xl border bg-card p-4 text-left transition-colors hover:border-foreground/20 hover:bg-muted/30 focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50">
        <div className="flex w-full items-start gap-3">
            {icon}
            <div className="min-w-0 flex-1 pt-0.5">
                <div className="flex items-center gap-2">
                    <span className="truncate font-medium">{name}</span>
                    {badge && <span className="shrink-0 rounded border px-1.5 py-px text-[10px] leading-4 text-muted-foreground">{badge}</span>}
                </div>
                {meta && <p className="mt-0.5 truncate text-xs text-muted-foreground">{meta}</p>}
            </div>
        </div>
        <p className="line-clamp-2 min-h-10 w-full flex-1 text-sm leading-5 text-muted-foreground">{description}</p>
        <div className="flex w-full items-center justify-between gap-2 border-t pt-3 text-xs">
            {status ? <AppStatusBadge status={status} /> : <span className="text-muted-foreground">{idle}</span>}
            <span className="inline-flex items-center gap-0.5 text-muted-foreground transition-colors group-hover:text-foreground">{action}<ChevronRight className="h-3.5 w-3.5" /></span>
        </div>
    </button>;
}

function Section({ title, count, children }: { title: string; count?: number; children: ReactNode }) {
    return <section className="space-y-3">
        <h3 className="text-sm font-medium">{title}{count !== undefined && <span className="ml-1.5 font-normal text-muted-foreground">{count}</span>}</h3>
        {children}
    </section>;
}

export default function ConnectedAppsConfig() {
    const mobile = useIsMobilePlatform();
    const [page, setPage] = useState('catalog');
    const [query, setQuery] = useState('');
    const [category, setCategory] = useState(ALL);
    const [catalog, setCatalog] = useState<CatalogResponse>();
    const [catalogBusy, setCatalogBusy] = useState(false);
    const [servers, setServers] = useState<MCPServer[]>([]);
    const [connections, setConnections] = useState<AppConnection[]>([]);
    const [builtins, setBuiltins] = useState<Builtin[]>([]);
    const [tools, setTools] = useState<Map<number, MCPServerTool[]>>(new Map());
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState('');
    const [selection, setSelection] = useState<Selection>();
    const [configOpen, setConfigOpen] = useState(false);
    const [importOpen, setImportOpen] = useState(false);
    const [initialConfig, setInitialConfig] = useState<Partial<MCPServerRequest>>();
    const [builtinId, setBuiltinId] = useState<string>();
    const [pluginsOpen, setPluginsOpen] = useState(false);
    const [pluginCatalog, setPluginCatalog] = useState<PluginCatalog>();
    const [pluginCatalogError, setPluginCatalogError] = useState('');
    const [pluginSource, setPluginSource] = useState<PluginSource>();

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
    const reload = useCallback(() => refresh().catch(e => setError(String(e))), [refresh]);

    const loadCatalog = useCallback(async () => {
        setCatalogBusy(true);
        try { setCatalog(await invoke<CatalogResponse>('fetch_app_catalog')); }
        catch (e) { setCatalog(old => ({ catalog: old?.catalog ?? null, fetched_at: old?.fetched_at ?? null, warning: String(e) })); }
        finally { setCatalogBusy(false); }
    }, []);
    useEffect(() => { void loadCatalog(); }, [loadCatalog]);
    useEffect(() => {
        let alive = true;
        if (!mobile) void invoke<PluginCatalog>('fetch_agent_plugin_catalog', { repo: 'openai/plugins', gitRef: 'main' }).then(value => {
            if (!Array.isArray(value?.entries)) throw new Error('应用详情目录格式无效');
            if (alive) { setPluginCatalog(value); setPluginCatalogError(''); }
        }).catch(e => { if (alive) setPluginCatalogError(String(e)); });
        return () => { alive = false; };
    }, [mobile]);

    const apps = useMemo(() => catalog?.catalog?.apps ?? [], [catalog]);
    const connectionOf = useMemo(() => new Map(connections.map(c => [c.server_id, c])), [connections]);
    const official = (source?: PluginSource | null): OfficialIcon | undefined => pluginCatalog?.entries.find(entry => sourceMatches(entry.source, source));
    const matches = (text: string) => text.toLowerCase().includes(query.trim().toLowerCase());
    const serverStatus = (server: MCPServer) => displayStatus(server, connectionOf.get(server.id), !mobile || isMobileExecutableMcpServer(server));
    const appStatus = (app: AppDefinition) => aggregateStatus(connections
        .filter(c => c.app_id === app.id || sourceMatches(c.definition.plugin_source, app.plugin_source))
        .map(c => displayStatus(servers.find(s => s.id === c.server_id), c)));

    const filteredBuiltins = builtins.filter(b => (category === ALL || category === BUILTIN) && matches(`${b.name} ${b.description}`));
    const filteredApps = apps.filter(app => (category === ALL || category === app.category) && matches(`${app.name} ${app.description} ${app.publisher}`));
    const filteredPlugins = mobile ? [] : (pluginCatalog?.entries ?? []).filter(entry => !apps.some(a => sourceMatches(a.plugin_source, entry.source))
        && (category === ALL || category === entry.category) && matches(`${entry.name} ${entry.description}`));
    const serverKind = (server: MCPServer) => server.is_builtin ? BUILTIN : connectionOf.has(server.id) ? '外部服务' : '自定义 MCP';
    // Problems first, otherwise keep the backend order.
    const filteredServers = servers
        .filter(server => (category === ALL || category === serverKind(server)) && matches(`${server.name} ${server.description ?? ''} ${connectionOf.get(server.id)?.label ?? ''}`))
        .map((server, index) => ({ server, status: serverStatus(server), index }))
        .sort((a, b) => Number(b.status.attention) - Number(a.status.attention) || a.index - b.index);
    const attentionCount = servers.filter(server => serverStatus(server).attention).length;
    const categories = page === 'catalog'
        ? [ALL, BUILTIN, ...new Set([...apps.map(app => app.category), ...(mobile ? [] : pluginCatalog?.entries ?? []).map(entry => entry.category)].filter(Boolean))]
        : [ALL, BUILTIN, '外部服务', '自定义 MCP'];

    const importConfigs = async (configs: MCPServerRequest[]) => {
        setImportOpen(false);
        if (configs.length === 1) { setInitialConfig(configs[0]); setConfigOpen(true); return; }
        try {
            for (const request of configs) await invoke('add_mcp_server', { request: { ...request, is_enabled: false } });
            await refresh(); setPage('installed'); setCategory(ALL);
            toast.success('已导入配置，保持停用；请检查后启用并测试连接。');
        } catch (e) { toast.error(`导入未全部完成：${String(e)}`); await reload(); }
    };
    const catalogEmpty = !filteredBuiltins.length && !filteredApps.length && !filteredPlugins.length;

    const catalogPage = <div className="space-y-8">
        {catalog?.warning && <div role="status" className="flex gap-3 rounded-lg border bg-muted/40 px-4 py-3 text-sm">
            <CloudOff className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
            <div className="min-w-0 space-y-1">
                <p>{catalog.catalog ? `外部应用目录刷新失败，正在显示 ${catalog.fetched_at ? new Date(catalog.fetched_at).toLocaleString() : ''} 的缓存。` : '外部应用目录暂不可用。'}内置工具和现有连接仍可管理。</p>
                <p className="break-words text-xs text-muted-foreground">{catalog.warning}</p>
            </div>
        </div>}
        {filteredBuiltins.length > 0 && <Section title="AIPP 内置工具" count={filteredBuiltins.length}>
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">{filteredBuiltins.map(builtin => {
                const server = servers.find(s => s.is_builtin && s.command === builtin.command);
                return <AppCard key={builtin.id} name={builtin.name} badge="内置" description={builtin.description} icon={<AppIcon builtin />}
                    status={server ? serverStatus(server) : undefined} idle="未添加" action={server ? '管理' : '添加'}
                    onClick={() => server ? setSelection({ kind: 'server', serverId: server.id }) : setBuiltinId(builtin.id)} />;
            })}</div>
        </Section>}
        {(filteredApps.length > 0 || filteredPlugins.length > 0) && <Section title="应用与配套技能" count={filteredApps.length + filteredPlugins.length}>
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
                {filteredApps.map(app => {
                    const status = appStatus(app);
                    return <AppCard key={app.id} name={app.name} meta={`${app.publisher} · ${app.category}`} description={app.description}
                        icon={<AppIcon icon={app.icon} official={official(app.plugin_source)} />} status={status} idle="未连接" action={status ? '管理' : '连接'}
                        onClick={() => setSelection({ kind: 'app', appId: app.id })} />;
                })}
                {filteredPlugins.map(entry => <AppCard key={`${entry.source.repo}/${entry.source.path}`} name={entry.name} meta={entry.category} description={entry.description}
                    icon={<AppIcon official={entry} />} idle="GitHub 插件" action="查看" onClick={() => setSelection({ kind: 'plugin', source: entry.source, name: entry.name })} />)}
            </div>
            {pluginCatalogError && <p className="text-xs text-muted-foreground">插件目录暂不可用：{pluginCatalogError}</p>}
            {!!catalog?.skipped?.length && <details className="text-xs text-muted-foreground">
                <summary className="cursor-pointer">{catalog.skipped.length} 个应用需要更新 AIPP 后才能使用</summary>
                <ul className="mt-2 space-y-1 pl-4">{catalog.skipped.map(item => <li key={item} className="list-disc break-words">{item}</li>)}</ul>
            </details>}
        </Section>}
        {(loading || catalogBusy) && catalogEmpty && <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">{[0, 1, 2, 3].map(i => <div key={i} className="h-40 animate-pulse rounded-xl bg-muted" />)}</div>}
        {!loading && !catalogBusy && catalogEmpty && <p className="py-12 text-center text-sm text-muted-foreground">没有匹配的应用。</p>}
    </div>;

    const installedPage = <div className="space-y-3">
        {attentionCount > 0 && <p className="text-xs text-muted-foreground">{attentionCount} 个连接需要处理，已排在最前。</p>}
        {loading ? <div className="space-y-2">{[0, 1, 2].map(i => <div key={i} className="h-16 animate-pulse rounded-lg bg-muted" />)}</div>
            : !filteredServers.length ? <div className="flex flex-col items-center gap-3 rounded-xl border border-dashed py-12 text-sm text-muted-foreground">
                <p>{servers.length ? '没有匹配的连接。' : '还没有连接任何应用。'}</p>
                {!servers.length && <Button size="sm" variant="outline" onClick={() => setPage('catalog')}>浏览应用目录</Button>}
            </div>
                : <div className="divide-y overflow-hidden rounded-xl border">
                    {filteredServers.map(({ server, status }) => {
                        const connection = connectionOf.get(server.id);
                        const summary = toolSummary(tools.get(server.id), server, connection);
                        return <button key={server.id} type="button" className="flex w-full items-center gap-4 px-4 py-3 text-left transition-colors hover:bg-muted/40 focus-visible:bg-muted/40 focus-visible:outline-none" onClick={() => setSelection({ kind: 'server', serverId: server.id })}>
                            <AppIcon size="sm" builtin={server.is_builtin} icon={connection?.definition.icon} official={official(connection?.definition.plugin_source)} />
                            <div className="min-w-0 flex-1">
                                <p className="truncate text-sm font-medium">{connection?.label ?? server.name}</p>
                                <p className="truncate text-xs text-muted-foreground">{server.is_builtin ? '内置工具' : connection ? connection.definition.name : `自定义 MCP · ${server.transport_type}`}</p>
                            </div>
                            <AppStatusBadge status={status} className="w-28 shrink-0" />
                            <span className={cn('hidden w-24 shrink-0 text-right text-xs sm:block', summary.known ? 'text-foreground/80' : 'text-muted-foreground')}>{summary.label}</span>
                            <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />
                        </button>;
                    })}
                </div>}
    </div>;

    return <div className="h-full overflow-y-auto bg-background">
        <div className="mx-auto max-w-5xl space-y-6 px-4 py-6 sm:px-8 sm:py-8">
            <header className="flex flex-wrap items-start justify-between gap-4">
                <div>
                    <h2 className="text-xl font-semibold tracking-tight">应用与工具</h2>
                    <p className="mt-1 text-sm text-muted-foreground">连接外部服务，并决定哪些助手可以使用它们的工具。</p>
                </div>
                <div className="flex items-center gap-1">
                    <Button variant="ghost" size="icon" aria-label="刷新应用目录和连接" disabled={catalogBusy} onClick={() => { void loadCatalog(); void reload(); }}>
                        <RefreshCw className={cn('h-4 w-4', catalogBusy && 'animate-spin')} />
                    </Button>
                    <DropdownMenu>
                        <DropdownMenuTrigger asChild><Button variant="outline"><Plus className="h-4 w-4" />添加</Button></DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-52">
                            <DropdownMenuItem onSelect={() => { setInitialConfig(undefined); setConfigOpen(true); }}>自定义 MCP</DropdownMenuItem>
                            {getQuickStartTemplates().filter(template => !mobile || isMobileExecutableMcpServer(template.template)).map(template =>
                                <DropdownMenuItem key={template.id} onSelect={() => { setInitialConfig(template.template); setConfigOpen(true); }}>{template.name} 模板</DropdownMenuItem>)}
                            <DropdownMenuItem onSelect={() => setImportOpen(true)}>从 JSON 导入</DropdownMenuItem>
                            {!mobile && <><DropdownMenuSeparator /><DropdownMenuLabel className="text-xs font-normal text-muted-foreground">高级</DropdownMenuLabel>
                                <DropdownMenuItem onSelect={() => { setPluginSource(undefined); setPluginsOpen(true); }}>高级插件来源管理</DropdownMenuItem></>}
                        </DropdownMenuContent>
                    </DropdownMenu>
                </div>
            </header>

            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <Tabs value={page} onValueChange={value => { setPage(value); setCategory(ALL); }}>
                    <TabsList>
                        <TabsTrigger value="catalog" className="px-4">应用目录</TabsTrigger>
                        <TabsTrigger value="installed" className="gap-1.5 px-4">
                            我的应用{servers.length > 0 && <span className="text-xs text-muted-foreground">{servers.length}</span>}
                            {attentionCount > 0 && <span aria-label={`${attentionCount} 个需要处理`} className="h-1.5 w-1.5 rounded-full bg-amber-500" />}
                        </TabsTrigger>
                    </TabsList>
                </Tabs>
                <div className="relative sm:w-72">
                    <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                    <Input className="pl-9" aria-label="搜索应用与工具" placeholder="搜索应用、工具或用途…" value={query} onChange={e => setQuery(e.target.value)} />
                </div>
            </div>
            <div className="-mt-2 flex gap-1.5 overflow-x-auto pb-1">{categories.map(item =>
                <Button key={item} size="sm" variant={category === item ? 'secondary' : 'ghost'} className={cn('h-7 shrink-0 rounded-full px-3 text-xs', category !== item && 'text-muted-foreground')} onClick={() => setCategory(item)}>{item}</Button>)}
            </div>
            {error && <p role="alert" className="rounded-lg border border-destructive/30 px-4 py-3 text-sm text-destructive">{error}</p>}
            {page === 'catalog' ? catalogPage : installedPage}
        </div>

        <AppDetailSheet selection={selection} onSelect={setSelection} apps={apps} servers={servers} connections={connections} tools={tools}
            pluginCatalog={pluginCatalog} mobile={mobile} refresh={reload}
            onOpenPluginManager={source => { setPluginSource(source); setPluginsOpen(true); }} />
        <MCPServerDialog isOpen={configOpen} initialConfig={initialConfig} onClose={() => setConfigOpen(false)} onSubmit={() => { setConfigOpen(false); setPage('installed'); setCategory(ALL); void reload(); }} />
        <JSONImportDialog isOpen={importOpen} onClose={() => setImportOpen(false)} onImport={configs => void importConfigs(configs)} />
        <Sheet open={pluginsOpen} onOpenChange={setPluginsOpen}><SheetContent className="w-full overflow-y-auto sm:max-w-3xl"><SheetHeader><SheetTitle>GitHub 插件</SheetTitle><SheetDescription>高级来源管理；应用详情与安装请从应用目录进入。</SheetDescription></SheetHeader>{pluginsOpen && <div className="px-5 pb-8"><AgentPluginsConfig initialSource={pluginSource} onChanged={() => void reload()} /></div>}</SheetContent></Sheet>
        <BuiltinToolDialog isOpen={!!builtinId} initialTemplateId={builtinId} onClose={() => setBuiltinId(undefined)} onSubmit={() => { setBuiltinId(undefined); void reload(); }} />
    </div>;
}
