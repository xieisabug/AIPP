import { ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, ChevronRight, CloudOff, Plus, RefreshCw, Search } from 'lucide-react';
import { toast } from 'sonner';
import { AppConnection, AppDefinition } from '@/data/ConnectedApp';
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
import { DirectoryCard, useAppDirectory } from './apps/useAppDirectory';
import { importDisabled, type ConnectionCheck } from './apps/connectionManagement';
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

function Section({ title, count, more, children }: { title: string; count?: number; more?: () => void; children: ReactNode }) {
    return <section className="space-y-3">
        <div className="flex items-center justify-between"><h3 className="text-sm font-medium">{title}{count !== undefined && <span className="ml-1.5 font-normal text-muted-foreground">{count}</span>}</h3>{more && <Button variant="ghost" size="sm" onClick={more} aria-label={title+"更多"}>更多<ChevronRight /></Button>}</div>
        {children}
    </section>;
}

export default function ConnectedAppsConfig() {
    const mobile = useIsMobilePlatform();
    const [page, setPage] = useState('catalog');
    const [query, setQuery] = useState('');
    const [category, setCategory] = useState(ALL);
    const [builtinLimit,setBuiltinLimit]=useState(18);
    const builtinAnchor=useRef<HTMLDivElement>(null);
    const [apps, setApps] = useState<AppDefinition[]>([]);
    const detailRequest = useRef(0);
    const scroll = useRef<HTMLDivElement>(null);
    const [refreshCount, setRefreshCount] = useState(0);
    const [checks,setChecks] = useState<ConnectionCheck[]>([]);
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
    const [pluginSource, setPluginSource] = useState<PluginSource>();

    const refresh = useCallback(async (isCurrent: () => boolean = () => true) => {
        const [nextServers, nextConnections, nextBuiltins, nextChecks] = await Promise.all([
            invoke<MCPServer[]>('get_mcp_servers'), invoke<AppConnection[]>('list_app_connections'), invoke<Builtin[]>('list_aipp_builtin_templates'), invoke<ConnectionCheck[]>('list_connection_checks'),
        ]);
        if (!isCurrent()) return;
        setChecks(nextChecks); setServers(nextServers); setConnections(nextConnections); setBuiltins(nextBuiltins);
        setError(''); setLoading(false);
    }, []);
    useDataRefresh({ events: ['mcp_state_changed'], initial: true, refresh,
        onError: e => { setLoading(false); setError(String(e)); } });
    const reload = useCallback(() => refresh().catch(e => setError(String(e))), [refresh]);

    const directory = useAppDirectory(mobile, category === ALL || category === BUILTIN ? '' : category, query,
        page === 'catalog' && category !== BUILTIN, refreshCount);
    const groups = directory.home?.categories ?? [];
    const featured = groups.flatMap(g => g.items);
    const cards = [...featured,...directory.items];
    const [installedCatalogs, setInstalledCatalogs] = useState<Record<string, PluginCatalog>>({});
    // Status refreshes must not fetch the same catalog again.
    const repositories = new Map<string, {repo: string; gitRef: string}>();
    for (const {definition} of connections) {
        const source = definition.plugin_source;
        if (source) repositories.set(JSON.stringify([source.repo, source.git_ref]), {repo:source.repo, gitRef:source.git_ref});
    }
    const catalogSources = JSON.stringify([...repositories.entries()].sort(([a],[b]) => a.localeCompare(b)));
    useEffect(() => {
        if (mobile) return;
        let active = true;
        setInstalledCatalogs({});
        const sources: [string, {repo: string; gitRef: string}][] = JSON.parse(catalogSources);
        for (const [source, args] of sources) {
            const save = (catalog: PluginCatalog) => {
                if (active) setInstalledCatalogs(previous => ({...previous, [source]: catalog}));
            };
            void (async () => {
                const cached = await invoke<{catalog: PluginCatalog; fetched_at: string} | null>('get_cached_agent_plugin_catalog', args);
                if (!active) return;
                if (cached?.catalog) {
                    save(cached.catalog);
                    const age = Date.now() - Date.parse(cached.fetched_at);
                    if (age >= 0 && age < 86_400_000) return;
                }
                save(await invoke<PluginCatalog>('fetch_agent_plugin_catalog', args));
            })().catch(e => console.warn('读取插件图标目录失败', e));
        }
        return () => { active = false; };
    }, [mobile, catalogSources]);
    const pluginCatalog:PluginCatalog = {revision: directory.home?.version ?? '', entries: [...cards.flatMap(c => c.source ?
        [{...c, category: groups.find(g=>g.id===c.group)?.name ?? '', source:c.source}] : []), ...Object.values(installedCatalogs).flatMap(catalog => catalog.entries ?? [])]};
    const catalogBusy = directory.homeBusy;
    useEffect(() => {
        if (page !== 'installed' && !selection) return;
        let active=true;
        const targets=page==='installed' ? servers : servers.filter(server=>selection?.kind==='server' ? selection.serverId===server.id :
            connections.some(c=>c.server_id===server.id && (selection?.kind==='app' ? c.app_id===selection.appId : selection?.kind==='plugin' && sourceMatches(c.definition.plugin_source,selection.source))));
        void Promise.all(targets.map(async server=>[server.id,visibleSearchTools(await invoke<MCPServerTool[]>('get_mcp_server_tools',{serverId:server.id}),server.command,mobile)] as const))
          .then(entries=>{if(active)setTools(new Map(entries));}).catch(e=>{if(active)setError(String(e));});
        return ()=>{active=false;};
    },[servers,connections,page,selection,mobile]);
    useEffect(()=>{if(scroll.current)scroll.current.scrollTop=0;},[category,page]);
    const openCard = async (card:DirectoryCard) => {
        const request=++detailRequest.current;
        if(card.kind==='plugin' && card.source) { setSelection({kind:'plugin',source:card.source,name:card.name});return; }
        try {
            const app=await invoke<AppDefinition>('get_app_directory_detail',{id:card.id,version:directory.home?.version??null});
            if(request!==detailRequest.current)return;
            setApps(old=>[...old.filter(item=>item.id!==app.id),app]);setSelection({kind:'app',appId:app.id});
        } catch(e) {if(request===detailRequest.current)toast.error(String(e));}
    };
    const connectionOf = useMemo(() => new Map(connections.map(c => [c.server_id, c])), [connections]);
    const official = (source?: PluginSource | null): OfficialIcon | undefined => pluginCatalog?.entries.find(entry => sourceMatches(entry.source, source));
    const matches = (text: string) => text.toLowerCase().includes(query.trim().toLowerCase());
    const serverStatus = (server: MCPServer) => displayStatus(server, connectionOf.get(server.id), !mobile || isMobileExecutableMcpServer(server), checks.find(c => c.server_id === server.id));
    const appStatus = (app: DirectoryCard) => aggregateStatus(connections
        .filter(c => c.app_id === app.id || sourceMatches(c.definition.plugin_source, app.source))
        .map(c => displayStatus(servers.find(s => s.id === c.server_id), c)));

    const filteredBuiltins = builtins.filter(b => (category === ALL || category === BUILTIN) && matches(`${b.name} ${b.description}`));
    useEffect(()=>{
        setBuiltinLimit(18);
    },[category,query]);
    useEffect(()=>{
        if(category!==BUILTIN || !builtinAnchor.current || !('IntersectionObserver' in window)) return;
        const observer=new IntersectionObserver(entries=>{
            if(entries.some(e=>e.isIntersecting)) setBuiltinLimit(n=>n+18);
        },{rootMargin:'240px'});
        observer.observe(builtinAnchor.current);
        return ()=>observer.disconnect();
    },[category,builtinLimit,filteredBuiltins.length]);
    const serverKind = (server: MCPServer) => server.is_builtin ? BUILTIN : connectionOf.has(server.id) ? '外部服务' : '自定义 MCP';
    // Problems first, otherwise keep the backend order.
    const filteredServers = servers
        .filter(server => (category === ALL || category === serverKind(server)) && matches(`${server.name} ${server.description ?? ''} ${connectionOf.get(server.id)?.label ?? ''}`))
        .map((server, index) => ({ server, status: serverStatus(server), index }))
        .sort((a, b) => Number(b.status.attention) - Number(a.status.attention) || a.index - b.index);
    const attentionCount = servers.filter(server => serverStatus(server).attention).length;
    const categories = [ALL, BUILTIN, '外部服务', '自定义 MCP'];
    const importConfigs = async (configs: MCPServerRequest[]) => {
        await importDisabled(configs);
        setImportOpen(false); await refresh(); setPage('installed'); setCategory(ALL);
        toast.success('连接已保存为停用状态');

    };
    const listing = category !== ALL || !!query.trim();
    const renderCards = (items: DirectoryCard[]) => <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">{items.map(card => {
        const status = appStatus(card);
        return <AppCard key={card.id} name={card.name} description={card.description} meta={card.publisher}
            icon={<AppIcon icon={card.icon ?? undefined} official={card}/>} status={status}
            idle={card.kind==='app'?'未连接':'应用'} action={status?'管理':card.kind==='app'?'连接':'查看'} onClick={()=>void openCard(card)} />;
    })}</div>;
    const catalogPage = <div className="space-y-8">
        {category!==ALL && <Button variant="ghost" onClick={()=>setCategory(ALL)}><ArrowLeft/>返回应用目录</Button>}
        {directory.homeError && <p role="status" className="flex gap-2 text-sm"><CloudOff/>{directory.home?'正在显示缓存。':''}{directory.homeError}</p>}
        {filteredBuiltins.length > 0 && <Section title="AIPP 内置工具" count={filteredBuiltins.length} more={!listing && filteredBuiltins.length>6?()=>setCategory(BUILTIN):undefined}>
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">{filteredBuiltins.slice(0,listing ? builtinLimit : 6).map(builtin => {
                const server = servers.find(s => s.is_builtin && s.command === builtin.command);
                return <AppCard key={builtin.id} name={builtin.name} badge="内置" description={builtin.description} icon={<AppIcon builtinCommand={builtin.command} />}
                    status={server ? serverStatus(server) : undefined} idle="未添加" action={server ? '管理' : '添加'}
                    onClick={() => server ? setSelection({ kind: 'server', serverId: server.id }) : setBuiltinId(builtin.id)} />;
            })}</div>
            {listing && filteredBuiltins.length>builtinLimit && <div ref={builtinAnchor}><Button variant="ghost" onClick={()=>setBuiltinLimit(n=>n+18)}>加载更多内置工具</Button></div>}
        </Section>}
        {category!==BUILTIN && (!listing ? groups.map(group=><Section key={group.id} title={group.name} count={group.total}
            more={group.total>group.items.length?()=>setCategory(group.id):undefined}>{renderCards(group.items)}</Section>) :
            <Section title={groups.find(g=>g.id===category)?.name ?? '搜索结果'}>
                {renderCards(directory.items)}
                {directory.pageError && <p role="alert">{directory.pageError} <Button variant="link" onClick={directory.more}>重试</Button></p>}
                {directory.next!==null && <div ref={directory.anchor}><Button variant="ghost" disabled={directory.pageBusy} onClick={directory.more}>加载更多</Button></div>}
                {directory.pageBusy && <p role="status">正在加载…</p>}
                {!directory.pageBusy && !directory.items.length && !directory.pageError && <p>没有匹配的应用。</p>}
            </Section>)}
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
                            <AppIcon size="sm" builtinCommand={server.is_builtin ? server.command : undefined} icon={apps.find(app => app.id === connection?.app_id)?.icon ?? connection?.definition.icon} official={official(connection?.definition.plugin_source)} />
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

    return <div ref={scroll} className="h-full overflow-y-auto bg-background">
        <div className="space-y-6 px-4 py-6 sm:py-8">
            <header className="flex flex-wrap items-start justify-between gap-4">
                <div>
                    <h2 className="text-xl font-semibold tracking-tight">应用与工具</h2>
                    <p className="mt-1 text-sm text-muted-foreground">连接外部服务，并决定哪些助手可以使用它们的工具。</p>
                </div>
                <div className="flex items-center gap-1">
                    <Button variant="ghost" size="icon" aria-label="刷新应用目录和连接" disabled={catalogBusy} onClick={() => { setRefreshCount(count => count + 1); void reload(); }}>
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
            {page === 'installed' && <div className="-mt-2 flex gap-1.5 overflow-x-auto pb-1">{categories.map(item =>
                <Button key={item} size="sm" variant={category === item ? 'secondary' : 'ghost'} className={cn('h-7 shrink-0 rounded-full px-3 text-xs', category !== item && 'text-muted-foreground')} onClick={() => setCategory(item)}>{item}</Button>)}
            </div>}
            {error && <p role="alert" className="rounded-lg border border-destructive/30 px-4 py-3 text-sm text-destructive">{error}</p>}
            {page === 'catalog' ? catalogPage : installedPage}
        </div>

        <AppDetailSheet directoryCards={cards} checks={checks} selection={selection} onSelect={setSelection} apps={apps} servers={servers} connections={connections} tools={tools}
            pluginCatalog={pluginCatalog} mobile={mobile} refresh={reload}
            onOpenPluginManager={source => { setPluginSource(source); setPluginsOpen(true); }} />
        <MCPServerDialog isOpen={configOpen} initialConfig={initialConfig} onClose={() => setConfigOpen(false)} onSubmit={() => { setConfigOpen(false); setPage('installed'); setCategory(ALL); void reload(); }} />
        <JSONImportDialog isOpen={importOpen} onClose={() => setImportOpen(false)} onImport={importConfigs} onCatalog={async id => {
            const app = await invoke<AppDefinition>('get_app_directory_detail', {id});
            setApps(old => [...old.filter(a => a.id !== app.id), app]); setImportOpen(false); setSelection({kind:'app',appId:app.id});
        }} />
        <Sheet open={pluginsOpen} onOpenChange={setPluginsOpen}><SheetContent className="w-full overflow-y-auto sm:max-w-3xl"><SheetHeader><SheetTitle>GitHub 插件</SheetTitle><SheetDescription>高级来源管理；应用详情与安装请从应用目录进入。</SheetDescription></SheetHeader>{pluginsOpen && <div className="px-5 pb-8"><AgentPluginsConfig directoryCatalog={pluginCatalog} initialSource={pluginSource} onChanged={() => void reload()} /></div>}</SheetContent></Sheet>
        <BuiltinToolDialog isOpen={!!builtinId} initialTemplateId={builtinId} onClose={() => setBuiltinId(undefined)} onSubmit={() => { setBuiltinId(undefined); void reload(); }} />
    </div>;
}
