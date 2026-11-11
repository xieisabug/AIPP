import { ReactNode, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { AlertTriangle, ArrowLeft, ArrowUpRight, CircleSlash, Info, Loader2, LogIn, MoreHorizontal, Plug, Power, RefreshCw, Unplug, Users } from 'lucide-react';
import { AppConnection, AppDefinition, visibleAppFields } from '@/data/ConnectedApp';
import { MCPServer, MCPServerTool } from '@/data/MCP';
import { isMobileExecutableMcpServer } from '@/lib/mobileUnsupported';
import { cn } from '@/utils/utils';
import { Button } from '@/components/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import ConfirmDialog from '../../ConfirmDialog';
import MCPConfig from '../MCPConfig';
import AppIcon from './AppIcon';
import AppStatusBadge from './AppStatusBadge';
import AppConnectForm from './AppConnectForm';
import AppAssistantAccess from './AppAssistantAccess';
import AppRelatedSkills from './AppRelatedSkills';
import AppTransportSettings from './AppTransportSettings';
import AgentPluginsConfig, { PluginCatalog, PluginSource } from './AgentPluginsConfig';
import { authLabel, displayStatus, toolSummary } from './appStatus';

export type Selection = { kind: 'app'; appId: string } | { kind: 'server'; serverId: number } | { kind: 'plugin'; source: PluginSource; name: string };

const safeOrigin = (url: string) => { try { return new URL(url).origin; } catch { return '（地址格式无效）'; } };

export const sourceMatches = (a?: PluginSource | null, b?: PluginSource | null) => !!a && !!b && a.repo.toLowerCase() === b.repo.toLowerCase() && a.path === b.path;

interface Props {
    selection?: Selection;
    onSelect: (selection?: Selection) => void;
    apps: AppDefinition[];
    servers: MCPServer[];
    connections: AppConnection[];
    tools: Map<number, MCPServerTool[]>;
    pluginCatalog?: PluginCatalog;
    mobile: boolean;
    refresh: () => Promise<void>;
    onOpenPluginManager: (source?: PluginSource) => void;
}

type NoticeTone = 'neutral' | 'warn' | 'error';
function Notice({ tone = 'neutral', icon, title, children, actions }: { tone?: NoticeTone; icon: ReactNode; title: string; children?: ReactNode; actions?: ReactNode }) {
    return <div role={tone === 'neutral' ? 'note' : 'alert'} className={cn('flex gap-3 rounded-lg border px-4 py-3 text-sm',
        tone === 'warn' && 'border-amber-500/40 bg-amber-500/5', tone === 'error' && 'border-destructive/40 bg-destructive/5', tone === 'neutral' && 'bg-muted/40')}>
        <span className={cn('mt-0.5 shrink-0 [&_svg]:h-4 [&_svg]:w-4', tone === 'warn' ? 'text-amber-600 dark:text-amber-400' : tone === 'error' ? 'text-destructive' : 'text-muted-foreground')}>{icon}</span>
        <div className="min-w-0 flex-1 space-y-1">
            <p className="font-medium">{title}</p>
            {children && <div className="break-words text-muted-foreground">{children}</div>}
            {actions && <div className="flex flex-wrap gap-2 pt-2">{actions}</div>}
        </div>
    </div>;
}

function InfoList({ rows }: { rows: [string, ReactNode][] }) {
    return <section className="space-y-3">
        <h4 className="text-sm font-medium">信息</h4>
        <dl className="grid grid-cols-[6.5rem_1fr] gap-x-4 gap-y-3 rounded-lg border px-4 py-3 text-sm">
            {rows.map(([term, value]) => <div key={term} className="contents"><dt className="text-muted-foreground">{term}</dt><dd className="min-w-0 break-words">{value}</dd></div>)}
        </dl>
    </section>;
}

function Panel({ title, description, children, danger = false }: { title: string; description?: ReactNode; children: ReactNode; danger?: boolean }) {
    return <section className={cn('space-y-4 rounded-lg border p-4', danger && 'border-destructive/30')}>
        <div className="space-y-1">
            <h4 className={cn('text-sm font-medium', danger && 'text-destructive')}>{title}</h4>
            {description && <p className="text-xs leading-5 text-muted-foreground">{description}</p>}
        </div>
        {children}
    </section>;
}

export default function AppDetailSheet({ selection, onSelect, apps, servers, connections, tools, pluginCatalog, mobile, refresh, onOpenPluginManager }: Props) {
    const [tab, setTab] = useState('overview');
    // Connect view replaces the tabs: `connection` set = re-login/reconfigure of that instance.
    const [connectView, setConnectView] = useState<{ connection?: AppConnection; generation: number }>();
    const [busy, setBusy] = useState('');
    const [removing, setRemoving] = useState<MCPServer>();
    const [justConnected, setJustConnected] = useState<number>();

    const catalogApp = selection?.kind === 'app' ? apps.find(a => a.id === selection.appId) : undefined;
    const requestedSource = selection?.kind === 'plugin' ? selection.source : catalogApp?.plugin_source;
    const matching = connections.filter(c => (catalogApp && c.app_id === catalogApp.id) || sourceMatches(c.definition.plugin_source, requestedSource));
    // A catalog entry and its sole local connection share one detail view; several connections are listed instead.
    const serverId = selection?.kind === 'server' ? selection.serverId : matching.length === 1 ? matching[0].server_id : undefined;
    const server = servers.find(s => s.id === serverId);
    const connection = connections.find(c => c.server_id === serverId);
    const app = connection?.definition ?? catalogApp;
    const pluginSource = requestedSource ?? app?.plugin_source ?? apps.find(a => a.id === app?.id)?.plugin_source;
    const official = pluginCatalog?.entries.find(entry => sourceMatches(entry.source, pluginSource));
    const pluginOverview = !mobile && !!pluginSource;
    const deviceSupported = !server || !mobile || isMobileExecutableMcpServer(server);
    const appSupported = (definition: AppDefinition) => !mobile || definition.platforms.includes('android');
    const status = server ? displayStatus(server, connection, deviceSupported) : undefined;
    const serverTools = server ? tools.get(server.id) : undefined;
    const summary = server ? toolSummary(serverTools, server, connection) : undefined;
    const isCustom = !!server && !server.is_builtin && !connection;

    const reset = () => { setTab('overview'); setConnectView(undefined); setBusy(''); };
    const go = (next?: Selection) => { reset(); onSelect(next); };
    const close = () => { reset(); setJustConnected(undefined); onSelect(undefined); void refresh().catch(() => {}); };

    const run = async (label: string, action: () => Promise<unknown>) => {
        setBusy(label);
        try {
            const result = await action();
            if (typeof result === 'string') toast.success(result);
        } catch (e) { toast.error(String(e)); }
        finally { setBusy(''); await refresh().catch(() => {}); }
    };

    const startConnect = (existing?: AppConnection) => { setTab('overview'); setConnectView(old => ({ connection: existing, generation: (old?.generation ?? 0) + 1 })); };

    const openPluginConnection = (definition: AppDefinition) => run('', async () => {
        const items = await invoke<{ preview: { key: string; source: PluginSource } }[]>('list_agent_plugins');
        const found = items.find(i => definition.id.startsWith(`agent-plugin:${i.preview.key}:`));
        if (!found) throw new Error('插件已卸载，请从 GitHub 插件重新安装');
        onOpenPluginManager(found.preview.source);
    });

    /** OAuth without secret inputs re-authorizes in one click; anything needing a secret goes through the form. */
    const reconnect = (target: AppConnection) => {
        const definition = target.definition;
        if (definition.connection.transport === 'stdio') { void openPluginConnection(definition); return; }
        const needsInput = definition.connection.auth !== 'oauth'
            || visibleAppFields(definition.fields, target.config).some(f => f.secret || (f.required && !target.config[f.key]));
        if (needsInput) { startConnect(target); return; }
        void run('正在打开授权页面…', async () => {
            const result = await invoke<{ server_id: number; authorization_url: string | null }>('connect_catalog_app', {
                request: { app_id: target.app_id, request_id: crypto.randomUUID(), label: target.label, values: target.config, server_id: target.server_id },
            });
            if (result.authorization_url) await openUrl(result.authorization_url);
        });
    };
    const continueAuthorization = (target: AppConnection) => void run('正在打开授权页面…', async () => {
        const url = await invoke<string | null>('get_app_authorization_url', { serverId: target.server_id });
        if (!url) { reconnect(target); return; }
        await openUrl(url);
    });
    const verify = (id: number) => run('正在测试连接…', async () => { await invoke('verify_app_connection', { serverId: id }); return '连接可用，工具列表已更新'; });
    const disconnect = (id: number, revokeRemote = true) => run('正在断开连接…', () => invoke<string>('disconnect_app_connection', { serverId: id, revokeRemote }));
    const enable = (id: number) => run('正在启用…', async () => { await invoke('toggle_mcp_server', { id, isEnabled: true }); });

    const onConnected = (id: number) => {
        const isNew = !connectView?.connection;
        setConnectView(undefined); setTab('overview');
        if (isNew) setJustConnected(id);
        void refresh().then(() => onSelect({ kind: 'server', serverId: id })).catch(e => toast.error(String(e)));
    };

    const title = connection?.label ?? app?.name ?? server?.name ?? (selection?.kind === 'plugin' ? selection.name : '应用详情');
    const subtitle = connection ? [...new Set([app?.name, app?.publisher, app?.category])].filter(part => part && part !== title).join(' · ')
        : server?.is_builtin ? '内置工具 · AIPP'
            : server ? `自定义 MCP · ${server.transport_type}`
                : app ? [app.publisher, app.category, app.version && `版本 ${app.version}`].filter(Boolean).join(' · ')
                    : 'GitHub 插件';

    const busyIcon = (label: string) => busy === label ? <Loader2 className="h-4 w-4 animate-spin" /> : null;
    const notice = (() => {
        if (!server || connectView) return null;
        if (!deviceSupported) return <Notice icon={<CircleSlash />} title="此设备不能执行">此连接依赖本机进程。可以查看配置和授权，但只能在桌面端运行。</Notice>;
        if (connection) {
            const error = connection.error && <p className="font-mono text-xs leading-5">{connection.error}</p>;
            switch (connection.status) {
                case 'connecting': return <Notice icon={<Loader2 className="animate-spin" />} title="等待浏览器授权"
                    actions={<Button size="sm" variant="outline" disabled={!!busy} onClick={() => continueAuthorization(connection)}>{busyIcon('正在打开授权页面…')}继续授权</Button>}>
                    在浏览器中完成登录后，这里会自动更新。没有看到授权页面时可以重新打开。</Notice>;
                case 'auth_required': return <Notice tone="warn" icon={<AlertTriangle />} title="登录已失效，需要重新授权"
                    actions={<Button size="sm" disabled={!!busy} onClick={() => reconnect(connection)}>{busyIcon('正在打开授权页面…') ?? <LogIn className="h-4 w-4" />}重新登录</Button>}>
                    {error ?? '连接已停用，助手暂时无法使用它的工具。'}</Notice>;
                case 'disconnected': return connection.error
                    ? <Notice tone="warn" icon={<AlertTriangle />} title="断开未完成" actions={<>
                        <Button size="sm" variant="outline" disabled={!!busy} onClick={() => void disconnect(connection.server_id)}>{busyIcon('正在断开连接…')}重试断开</Button>
                        <Button size="sm" variant="ghost" disabled={!!busy} onClick={() => void disconnect(connection.server_id, false)}>删除凭据，不撤销授权</Button>
                    </>}>{error}</Notice>
                    : <Notice icon={<Unplug />} title="连接已断开" actions={<Button size="sm" disabled={!!busy} onClick={() => reconnect(connection)}><Plug className="h-4 w-4" />重新连接</Button>}>
                        本机凭据已删除，助手无法使用此连接。重新连接会沿用这个连接实例和已有的助手授权。</Notice>;
            }
            if (!server.is_enabled) return <Notice icon={<Power />} title="连接已停用" actions={<Button size="sm" variant="outline" disabled={!!busy} onClick={() => void enable(server.id)}>{busyIcon('正在启用…')}启用</Button>}>
                停用期间助手不会调用它的工具。</Notice>;
            if (connection.status === 'error') return <Notice tone="error" icon={<AlertTriangle />} title="上次验证失败" actions={<>
                <Button size="sm" variant="outline" disabled={!!busy} onClick={() => void verify(server.id)}>{busyIcon('正在测试连接…') ?? <RefreshCw className="h-4 w-4" />}重新测试</Button>
                <Button size="sm" variant="ghost" disabled={!!busy} onClick={() => reconnect(connection)}>重新配置</Button>
            </>}>{error}</Notice>;
        } else if (!server.is_enabled) {
            return <Notice icon={<Power />} title="已停用" actions={<Button size="sm" variant="outline" disabled={!!busy} onClick={() => void enable(server.id)}>{busyIcon('正在启用…')}启用</Button>}>停用期间助手不会调用它的工具。</Notice>;
        }
        if (justConnected === server.id && !server.is_builtin && tab !== 'access') return <Notice icon={<Users />} title="下一步：选择可以使用它的助手"
            actions={<Button size="sm" variant="outline" onClick={() => { setTab('access'); setJustConnected(undefined); }}>去授权</Button>}>
            新连接不会自动开放给任何助手。</Notice>;
        return null;
    })();

    const headerActions = (() => {
        if (connectView) return null;
        const menu: ReactNode[] = [];
        if (connection && connection.definition.connection.transport !== 'stdio') menu.push(<DropdownMenuItem key="reconfigure" onSelect={() => startConnect(connection)}>重新配置</DropdownMenuItem>);
        if (app && connection && appSupported(app) && app.connection.transport !== 'stdio') menu.push(<DropdownMenuItem key="another" onSelect={() => startConnect()}>添加另一个连接</DropdownMenuItem>);
        if (server?.is_deletable) {
            if (menu.length) menu.push(<DropdownMenuSeparator key="sep" />);
            menu.push(<DropdownMenuItem key="delete" className="text-destructive" disabled={!!busy} onSelect={() => setRemoving(server)}>删除连接</DropdownMenuItem>);
        }
        const primary = !server && app
            ? app.connection.transport === 'stdio'
                ? <Button size="sm" variant="outline" onClick={() => void openPluginConnection(app)}>管理插件本地连接</Button>
                : <Button size="sm" disabled={!appSupported(app)} onClick={() => startConnect()}><Plug className="h-4 w-4" />{matching.length ? '添加连接' : '连接'}</Button>
            : connection && server?.is_enabled && !status?.attention && connection.status !== 'disconnected'
                ? <Button size="sm" variant="outline" disabled={!!busy} onClick={() => void verify(server.id)}>{busyIcon('正在测试连接…') ?? <RefreshCw className="h-4 w-4" />}测试连接</Button>
                : null;
        return <div className="flex shrink-0 items-center gap-1">
            {primary}
            {menu.length > 0 && <DropdownMenu><DropdownMenuTrigger asChild><Button variant="ghost" size="icon" className="h-8 w-8" aria-label="更多连接操作"><MoreHorizontal className="h-4 w-4" /></Button></DropdownMenuTrigger>
                <DropdownMenuContent align="end">{menu}</DropdownMenuContent></DropdownMenu>}
        </div>;
    })();

    const appInfo = (definition: AppDefinition): [string, ReactNode][] => [
        ['提供方', definition.publisher || '—'],
        ['分类', definition.category || '—'],
        ['版本', definition.version || '—'],
        ['登录方式', authLabel(definition.connection.auth)],
        ...(definition.connection.auth === 'oauth' ? [['授权范围', definition.connection.scopes?.length ? definition.connection.scopes.join('、') : '由服务商决定，以授权页面为准'] as [string, ReactNode]] : []),
        ...(definition.connection.transport === 'http' ? [['服务地址', <span className="font-mono text-xs">{definition.connection.url}</span>] as [string, ReactNode]] : []),
        ['支持平台', definition.platforms.map(p => ({ windows: 'Windows', macos: 'macOS', linux: 'Linux', android: 'Android' } as Record<string, string>)[p] ?? p).join('、') || '—'],
        ...(definition.homepage?.startsWith('https://') ? [['服务说明', <button type="button" className="inline-flex items-center gap-1 underline-offset-4 hover:underline" onClick={() => void openUrl(definition.homepage!).catch(e => toast.error(String(e)))}>打开文档<ArrowUpRight className="h-3.5 w-3.5" /></button>] as [string, ReactNode]] : []),
    ];
    const serverInfo = (target: MCPServer): [string, ReactNode][] => [
        ['类型', target.is_builtin ? '内置工具' : `自定义 MCP（${target.transport_type}）`],
        // Origin only: user-entered URLs and commands may carry tokens in query strings or arguments.
        ...(target.url ? [['服务地址', <span className="font-mono text-xs">{safeOrigin(target.url)}</span>] as [string, ReactNode]] : []),
        ['调用超时', `${target.timeout ?? 30000} 毫秒`],
        ['工具', summary?.label ?? '—'],
    ];

    const description = app ? app.notice || app.description : server?.description;
    const overview = <div className="space-y-8">
        {description && <p className="text-sm leading-7">{description}</p>}
        {server?.is_builtin && <p className="text-sm leading-6 text-muted-foreground">内置工具由 AIPP 提供。工具适用场景、平台限制和系统保护继续生效；总管家专用能力不会向普通助手开放。</p>}
        {!server && app && !appSupported(app) && <Notice icon={<CircleSlash />} title="当前设备暂不支持">此应用尚未支持当前设备的安全凭据存储，暂不能在此设备建立认证连接。</Notice>}
        {!server && matching.length > 1 && <section className="space-y-3">
            <h4 className="text-sm font-medium">我的连接 <span className="font-normal text-muted-foreground">{matching.length}</span></h4>
            <div className="divide-y rounded-lg border">{matching.map(item => {
                const itemServer = servers.find(s => s.id === item.server_id);
                return <button key={item.server_id} type="button" className="flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-muted/40" onClick={() => go({ kind: 'server', serverId: item.server_id })}>
                    <span className="min-w-0 flex-1 truncate text-sm">{item.label}</span>
                    <AppStatusBadge status={displayStatus(itemServer, item)} />
                </button>;
            })}</div>
        </section>}
        {pluginOverview && <AgentPluginsConfig key={JSON.stringify(pluginSource)} embedded hasConnection={!!connection || matching.length > 0} initialSource={pluginSource!} app={app}
            onChanged={() => void refresh().catch(() => {})} />}
        {!pluginOverview && app?.skill_ids && app.skill_ids.length > 0 && <AppRelatedSkills key={app.id} ids={app.skill_ids} />}
        {app && <InfoList rows={appInfo(app)} />}
        {isCustom && server && <InfoList rows={serverInfo(server)} />}
    </div>;

    const connectViewContent = connectView && app && <div className="space-y-5">
        <Button variant="ghost" size="sm" className="-ml-2 text-muted-foreground" onClick={() => setConnectView(undefined)}><ArrowLeft className="h-4 w-4" />返回</Button>
        <div className="space-y-1">
            <h3 className="text-base font-semibold">{connectView.connection ? `重新配置「${connectView.connection.label}」` : `连接 ${app.name}`}</h3>
            {!connectView.connection && matching.length > 0 && <p className="text-sm text-muted-foreground">将新建一个独立连接，不影响已有的 {matching.length} 个连接。</p>}
        </div>
        {appSupported(app)
            ? <AppConnectForm key={`${app.id}-${connectView.connection?.server_id ?? 'new'}-${connectView.generation}`} app={connectView.connection?.definition ?? app} connection={connectView.connection} onConnected={onConnected} onCancel={() => setConnectView(undefined)} />
            : <p className="text-sm text-muted-foreground">当前平台不支持此应用的认证连接。</p>}
    </div>;

    return <>
        <Sheet open={!!selection} onOpenChange={open => { if (!open) close(); }}>
            <SheetContent className="w-full gap-0 overflow-y-auto p-0 sm:max-w-2xl">
                <SheetHeader className="space-y-4 border-b px-6 pb-0 pt-6">
                    <div className="flex items-start gap-4 pr-6">
                        <AppIcon size="lg" builtin={server?.is_builtin} icon={app?.icon} official={official} />
                        <div className="min-w-0 flex-1 space-y-1">
                            <SheetTitle className="truncate text-lg">{title}</SheetTitle>
                            <SheetDescription className="truncate">{subtitle}</SheetDescription>
                            {status && summary && <div className="flex flex-wrap items-center gap-x-3 gap-y-1 pt-1 text-xs text-muted-foreground">
                                <AppStatusBadge status={status} />
                                <span aria-hidden>·</span><span>{summary.known ? `${summary.enabled} / ${summary.total} 个工具已启用` : '工具尚未检测，验证连接后显示'}</span>
                            </div>}
                        </div>
                        {headerActions}
                    </div>
                    {notice}
                    {server && !connectView ? <Tabs value={tab} onValueChange={value => { setTab(value); if (value === 'access') setJustConnected(undefined); }}>
                        <TabsList className="h-auto gap-4 rounded-none bg-transparent p-0">
                            {[['overview', '概览'], ['tools', summary?.known ? `工具 ${summary.total}` : '工具'], ...(!server.is_builtin ? [['access', '助手授权']] : []), ...(connection ? [['settings', '连接设置']] : [])].map(([value, label]) =>
                                <TabsTrigger key={value} value={value} className="-mb-px h-9 flex-none rounded-none border-0 border-b-2 border-transparent px-0 shadow-none data-[state=active]:border-foreground data-[state=active]:bg-transparent data-[state=active]:shadow-none dark:data-[state=active]:bg-transparent">{label}</TabsTrigger>)}
                        </TabsList>
                    </Tabs> : <div className="pb-2" />}
                </SheetHeader>
                <div className="px-6 py-6">
                    {connectView ? connectViewContent : server ? <Tabs value={tab} onValueChange={setTab}>
                        <TabsContent value="overview" className="mt-0">{overview}</TabsContent>
                        <TabsContent value="tools" className="mt-0"><MCPConfig key={server.id} initialServerId={server.id} detailOnly compact
                            onEditManaged={connection ? () => connection.definition.connection.transport === 'stdio' ? void openPluginConnection(connection.definition) : startConnect(connection) : undefined}
                            onRefreshManaged={connection ? () => verify(server.id) : undefined} /></TabsContent>
                        {!server.is_builtin && <TabsContent value="access" className="mt-0"><AppAssistantAccess serverId={server.id} /></TabsContent>}
                        {connection && <TabsContent value="settings" className="mt-0 space-y-4">
                            <Panel title="账号与凭据" description={connection.credential_status}>
                                <div className="flex flex-wrap items-center justify-between gap-3 text-sm">
                                    <div className="space-y-0.5"><p>{connection.label}</p><p className="text-xs text-muted-foreground">{authLabel(connection.definition.connection.auth)}</p></div>
                                    {connection.definition.connection.transport !== 'stdio'
                                        ? <Button size="sm" variant="outline" onClick={() => startConnect(connection)}>重新配置</Button>
                                        : <Button size="sm" variant="outline" onClick={() => void openPluginConnection(connection.definition)}>管理插件本地连接</Button>}
                                </div>
                            </Panel>
                            {server.transport_type === 'http' && <Panel title="网络" description="调整超时和代理不需要重新授权。">
                                <AppTransportSettings key={`${server.id}-${server.timeout}-${server.proxy_enabled}`} server={server} onSaved={() => { toast.success('网络设置已保存'); void refresh().catch(() => {}); }} />
                            </Panel>}
                            <Panel danger title="断开连接" description="断开会停用连接、删除本机凭据，并尝试撤销服务商支持的 OAuth 授权；已执行的操作不会回滚。启用同步时，断开和凭据删除会同步到其他设备。">
                                {connection.status === 'disconnected'
                                    ? <p className="text-sm text-muted-foreground">{connection.error ? '上次断开未完成，请在页面顶部重试或选择不撤销授权。' : '此连接已断开。'}</p>
                                    : <div className="flex justify-end"><Button size="sm" variant="outline" className="border-destructive/40 text-destructive hover:bg-destructive/5 hover:text-destructive" disabled={!!busy} onClick={() => void disconnect(connection.server_id)}>{busyIcon('正在断开连接…') ?? <Unplug className="h-4 w-4" />}断开连接</Button></div>}
                            </Panel>
                        </TabsContent>}
                    </Tabs> : overview}
                    {!server && !connectView && !app && !pluginOverview && <p className="flex items-center gap-2 text-sm text-muted-foreground"><Info className="h-4 w-4" />连接配置已移除。</p>}
                </div>
            </SheetContent>
        </Sheet>
        <ConfirmDialog isOpen={!!removing} title="删除连接" confirmText={`删除「${removing ? connections.find(c => c.server_id === removing.id)?.label ?? removing.name : ''}」的连接、工具配置和凭据。GitHub 插件包和复用此服务的其他连接不会被卸载。`}
            onCancel={() => { if (!busy) setRemoving(undefined); }}
            onConfirm={() => {
                if (!removing || busy) return;
                const id = removing.id;
                setBusy('正在删除…');
                void invoke('delete_mcp_server', { id }).then(async () => { setRemoving(undefined); close(); await refresh(); toast.success('连接已删除'); })
                    .catch(e => { setRemoving(undefined); toast.error(String(e)); })
                    .finally(() => setBusy(''));
            }} />
    </>;
}
