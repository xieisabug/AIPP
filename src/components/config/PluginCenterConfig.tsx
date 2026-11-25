import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openPath } from '@tauri-apps/plugin-opener';
import { CloudOff, Plus, RefreshCw, Search, XCircle } from 'lucide-react';
import { toast } from 'sonner';
import { isMobileAppPlatform, usePlatform } from '@/hooks/use-platform';
import { pluginRuntime, type LoadedPlugin } from '@/services/PluginRuntime';
import { cn } from '@/utils/utils';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import AppStatusBadge from './apps/AppStatusBadge';
import CatalogListRow from './catalog/CatalogListRow';
import FilterChips from './catalog/FilterChips';
import { Notice } from './catalog/DetailParts';
import PluginCard, { OfficialBadge, PluginTypeIcon } from './plugins/PluginCard';
import PluginDetailSheet, { type PluginDetailActions } from './plugins/PluginDetailSheet';
import PluginSourceInstallDialog from './plugins/PluginSourceInstallDialog';
import { contributionSummary } from './plugins/pluginDescribe';
import {
    buildPluginEntries, matchesPluginQuery, pluginDisplayStatus, pluginTypeLabel, PLUGIN_TYPES, sourceLabel,
    type InstalledPlugin, type OfficialPlugin, type PluginEntry, type PluginInstallState,
} from './plugins/pluginCatalog';

const ALL = 'all';
const TYPE_ORDER: string[] = PLUGIN_TYPES.map(type => type.code);
type Page = 'market' | 'mine';

interface Props { pluginList: LoadedPlugin[]; navigateTo?: (menuKey: string) => void }

/**
 * 插件页：骨架、筛选、详情抽屉沿用「应用与工具」（catalog/ 共享组件），
 * 卡片换成效果预览卡，分类用 AIPP 插件类型的主分类。
 */
export default function PluginCenterConfig({ pluginList, navigateTo }: Props) {
    const platform = usePlatform();
    const mobile = isMobileAppPlatform(platform);
    const [page, setPage] = useState<Page>('market');
    const [query, setQuery] = useState('');
    const [type, setType] = useState<string>(ALL);
    const [installed, setInstalled] = useState<InstalledPlugin[]>([]);
    const [installedLoaded, setInstalledLoaded] = useState(false);
    const [scanError, setScanError] = useState('');
    const [official, setOfficial] = useState<OfficialPlugin[]>([]);
    const [officialState, setOfficialState] = useState<'loading' | 'ready' | 'error'>('loading');
    const [officialError, setOfficialError] = useState('');
    const [runtimePlugins, setRuntimePlugins] = useState<LoadedPlugin[]>(pluginList);
    const [installs, setInstalls] = useState<Record<string, PluginInstallState>>({});
    const [selectedCode, setSelectedCode] = useState<string>();
    const [busy, setBusy] = useState(false);
    const [sourceOpen, setSourceOpen] = useState(false);

    useEffect(() => { setRuntimePlugins(pluginList); }, [pluginList]);

    const syncRuntime = useCallback(async () => {
        const items = await pluginRuntime.reloadPlugins();
        setRuntimePlugins(items);
        return items;
    }, []);

    const loadInstalled = useCallback(async () => {
        try {
            setInstalled(await invoke<InstalledPlugin[]>('list_plugins'));
            setScanError('');
            await syncRuntime().catch(error => console.error('[PluginCenterConfig] 刷新插件运行时失败', error));
        } catch (error) {
            setScanError(String(error));
        } finally {
            setInstalledLoaded(true);
        }
    }, [syncRuntime]);

    const loadOfficial = useCallback(async (useProxy = false) => {
        setOfficialState('loading');
        try {
            setOfficial(await invoke<OfficialPlugin[]>('fetch_official_plugins', { useProxy }));
            setOfficialState('ready'); setOfficialError('');
        } catch (error) {
            setOfficialError(String(error)); setOfficialState('error');
        }
    }, []);

    useEffect(() => {
        void loadInstalled();
        void loadOfficial();
        const unlisten = listen('plugin_registry_changed', () => { void loadInstalled(); });
        return () => { void unlisten.then(fn => fn()); };
    }, [loadInstalled, loadOfficial]);

    const loadedByCode = useMemo(() => new Map(runtimePlugins.map(item => [item.code, item])), [runtimePlugins]);
    const entries = useMemo(() => buildPluginEntries(official, installed), [official, installed]);
    const statusOf = useCallback((entry: PluginEntry) => pluginDisplayStatus(entry, { install: installs[entry.code], loaded: loadedByCode.get(entry.code), mobile }), [installs, loadedByCode, mobile]);

    // 市场 = 官方目录；我的插件 = 本机已安装（含第三方），出问题的排最前。
    const pageEntries = useMemo(() => {
        const list = page === 'market'
            ? entries.filter(entry => entry.official).sort((a, b) => TYPE_ORDER.indexOf(a.primaryType) - TYPE_ORDER.indexOf(b.primaryType))
            : entries.filter(entry => entry.installed).map((entry, index) => ({ entry, index, attention: statusOf(entry).attention }))
                .sort((a, b) => Number(b.attention) - Number(a.attention) || a.index - b.index).map(item => item.entry);
        return list;
    }, [entries, page, statusOf]);
    const typeItems = useMemo(() => {
        const counts = new Map<string, number>();
        for (const entry of pageEntries) counts.set(entry.primaryType, (counts.get(entry.primaryType) ?? 0) + 1);
        return [{ value: ALL, label: '全部', count: pageEntries.length },
            ...[...counts.keys()].sort((a, b) => TYPE_ORDER.indexOf(a) - TYPE_ORDER.indexOf(b)).map(code => ({ value: code, label: pluginTypeLabel(code), count: counts.get(code) }))];
    }, [pageEntries]);
    const visible = pageEntries.filter(entry => (type === ALL || entry.primaryType === type) && matchesPluginQuery(entry, query));
    const installedCount = entries.filter(entry => entry.installed).length;
    const attentionCount = entries.filter(entry => entry.installed && statusOf(entry).attention).length;
    const selected = entries.find(entry => entry.code === selectedCode);

    const openFolder = useCallback(() => {
        void invoke<string>('get_plugin_root_dir').then(openPath).catch(error => toast.error(`打开插件文件夹失败：${String(error)}`));
    }, []);

    const actions: PluginDetailActions = useMemo(() => ({
        install: async (entry, useProxy = false) => {
            const item = entry.official;
            if (!item?.source) { toast.error(`${entry.name} 缺少安装来源`); return; }
            setInstalls(prev => ({ ...prev, [entry.code]: { status: 'installing' } }));
            try {
                await invoke('install_plugin_archive_source', {
                    source: item.source, selections: item.dirs ?? [], expectedSha256: item.sha256 ?? null, useProxy, enableAfterInstall: true, officialId: item.id,
                });
                setInstalls(prev => { const next = { ...prev }; delete next[entry.code]; return next; });
                toast.success(`${entry.installed ? '已更新' : '已安装'}：${entry.name}`);
                await loadInstalled();
            } catch (error) {
                setInstalls(prev => ({ ...prev, [entry.code]: { status: 'failed', error: String(error) } }));
            }
        },
        toggle: async plugin => {
            setBusy(true);
            try {
                await invoke(plugin.isActive ? 'disable_plugin' : 'enable_plugin', { pluginId: plugin.pluginId });
                await loadInstalled();
            } catch (error) {
                toast.error(`${plugin.isActive ? '停用' : '启用'}插件失败：${String(error)}`);
            } finally { setBusy(false); }
        },
        uninstall: async plugin => {
            setBusy(true);
            try {
                await invoke('uninstall_plugin', { pluginId: plugin.pluginId });
                toast.success(`已卸载插件：${plugin.name}`);
                await loadInstalled();
            } catch (error) {
                toast.error(`卸载插件失败：${String(error)}`);
                throw error;
            } finally { setBusy(false); }
        },
        retryLoad: async plugin => {
            setBusy(true);
            try {
                if (plugin.entryAccessError) { await loadInstalled(); return; }
                const items = await syncRuntime();
                if (items.some(item => item.code === plugin.code && item.instance)) toast.success('插件运行时已刷新');
            } catch (error) {
                toast.error(`刷新插件运行时失败：${String(error)}`);
            } finally { setBusy(false); }
        },
        openFolder,
    }), [loadInstalled, openFolder, syncRuntime]);

    const switchPage = (next: string) => { setPage(next as Page); setType(ALL); };
    const refreshing = officialState === 'loading';

    const marketBody = () => officialState === 'error' && !official.length
        ? <Notice tone="warn" icon={<CloudOff />} title="无法获取插件市场" actions={<>
            <Button size="sm" variant="outline" onClick={() => void loadOfficial()}>重试</Button>
            <Button size="sm" variant="ghost" onClick={() => void loadOfficial(true)}>使用代理重试</Button>
        </>}>{officialError}</Notice>
        : officialState === 'loading' && !official.length
            ? <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">{[0, 1, 2, 3, 4, 5].map(i => <div key={i} className="h-[196px] animate-pulse rounded-xl bg-muted" />)}</div>
            : !visible.length ? <p className="rounded-xl border border-dashed py-12 text-center text-sm text-muted-foreground">没有匹配的插件。</p>
                : <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">{visible.map(entry =>
                    <PluginCard key={entry.code} entry={entry} install={installs[entry.code]} loaded={loadedByCode.get(entry.code)} mobile={mobile} onOpen={() => setSelectedCode(entry.code)} />)}</div>;

    const mineBody = () => !installedLoaded
        ? <div className="space-y-2">{[0, 1, 2].map(i => <div key={i} className="h-16 animate-pulse rounded-lg bg-muted" />)}</div>
        : !visible.length
            ? <div className="flex flex-col items-center gap-3 rounded-xl border border-dashed py-12 text-sm text-muted-foreground">
                <p>{installedCount ? '没有匹配的插件。' : '还没有安装插件。'}</p>
                {!installedCount && <Button size="sm" variant="outline" onClick={() => switchPage('market')}>浏览插件市场</Button>}
            </div>
            : <div className="divide-y overflow-hidden rounded-xl border">{visible.flatMap(entry => {
                const plugin = entry.installed;
                if (!plugin) return [];
                return <CatalogListRow key={entry.code} onClick={() => setSelectedCode(entry.code)}
                    icon={<PluginTypeIcon type={entry.primaryType} size="sm" />}
                    title={entry.name}
                    titleExtra={<>{entry.verifiedOfficial && <OfficialBadge />}<span className="shrink-0 text-xs font-normal text-muted-foreground">{pluginTypeLabel(entry.primaryType)}</span></>}
                    subtitle={entry.verifiedOfficial ? `v${plugin.version}` : `${sourceLabel(entry)} · v${plugin.version}`}
                    status={<AppStatusBadge status={statusOf(entry)} />}
                    detail={contributionSummary(plugin)}
                    control={plugin.isInstalled
                        ? <Switch aria-label={`${plugin.isActive ? '停用' : '启用'} ${plugin.name}`} checked={plugin.isActive} disabled={busy || (mobile && !!plugin.mobileUnavailableReason && !plugin.isActive)}
                            onCheckedChange={() => void actions.toggle(plugin)} />
                        : <span className="w-8" />} />;
            })}</div>;

    return <div className={cn('h-full overflow-y-auto bg-background', mobile && '[&_button]:min-h-11')}>
        <div className="space-y-6 px-4 py-6 sm:px-6">
            <header className="flex flex-wrap items-start justify-between gap-4">
                <div>
                    <h2 className="text-xl font-semibold tracking-tight">插件</h2>
                    <p className="mt-1 text-sm text-muted-foreground">
                        改变 AIPP 本身：主题、形象、消息渲染、快捷命令和新的界面。
                        {navigateTo && <>想让助手连接外部服务？<button type="button" className="text-foreground underline decoration-border underline-offset-4 hover:decoration-foreground" onClick={() => navigateTo('mcp-config')}>前往应用与工具</button></>}
                    </p>
                </div>
                <div className="flex items-center gap-1">
                    <Button variant="ghost" size="icon" aria-label="刷新插件市场和已安装插件" disabled={refreshing} onClick={() => { void loadOfficial(); void loadInstalled(); }}>
                        <RefreshCw className={cn('h-4 w-4', refreshing && 'animate-spin')} />
                    </Button>
                    <DropdownMenu>
                        <DropdownMenuTrigger asChild><Button variant="outline"><Plus className="h-4 w-4" />安装</Button></DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-52">
                            <DropdownMenuItem onSelect={() => setSourceOpen(true)}>从 GitHub / ZIP 安装…</DropdownMenuItem>
                            <DropdownMenuSeparator />
                            {!mobile && <DropdownMenuItem onSelect={openFolder}>打开插件文件夹</DropdownMenuItem>}
                            <DropdownMenuItem onSelect={() => void loadInstalled()}>重新扫描插件目录</DropdownMenuItem>
                        </DropdownMenuContent>
                    </DropdownMenu>
                </div>
            </header>

            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <Tabs value={page} onValueChange={switchPage}>
                    <TabsList>
                        <TabsTrigger value="market" className="px-4">插件市场</TabsTrigger>
                        <TabsTrigger value="mine" className="gap-1.5 px-4">
                            我的插件{installedCount > 0 && <span className="text-xs text-muted-foreground">{installedCount}</span>}
                            {attentionCount > 0 && <span aria-label={`${attentionCount} 个需要处理`} className="h-1.5 w-1.5 rounded-full bg-amber-500" />}
                        </TabsTrigger>
                    </TabsList>
                </Tabs>
                <div className="relative sm:w-72">
                    <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                    <Input className="pl-9" aria-label="搜索插件" placeholder="搜索插件或能力…" value={query} onChange={e => setQuery(e.target.value)} />
                </div>
            </div>

            {typeItems.length > 2 && <FilterChips label="插件分类" items={typeItems} value={type} onChange={setType} />}
            {scanError && <Notice tone="error" icon={<XCircle />} title={scanError.startsWith('PLUGIN_FILE_PERMISSION_DENIED:') ? '系统拒绝读取插件文件' : '读取已安装插件失败'}
                actions={<Button size="sm" variant="outline" onClick={() => void loadInstalled()}>重新扫描插件</Button>}>
                <p className="whitespace-pre-wrap break-all">{scanError}</p></Notice>}
            {page === 'mine' && attentionCount > 0 && <p className="-mt-3 text-xs text-muted-foreground">{attentionCount} 个插件需要处理，已排在最前。</p>}
            {page === 'market' ? marketBody() : mineBody()}
        </div>

        <PluginDetailSheet entry={selected} install={selected ? installs[selected.code] : undefined} loaded={selected ? loadedByCode.get(selected.code) : undefined}
            mobile={mobile} busy={busy} actions={actions} onClose={() => setSelectedCode(undefined)} />
        <PluginSourceInstallDialog open={sourceOpen} onOpenChange={setSourceOpen} mobile={mobile} onInstalled={loadInstalled} />
    </div>;
}
