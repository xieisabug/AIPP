import { useEffect, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AlertTriangle, ArrowUpRight, CircleSlash, FolderOpen, Info, Loader2, MoreHorizontal, RefreshCcw, ShieldAlert, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { usePlatform } from '@/hooks/use-platform';
import type { LoadedPlugin } from '@/services/PluginRuntime';
import { ConfigSheetBody } from '@/components/common/ConfigSaveFooter';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import PluginViewHost from '@/components/plugin/PluginViewHost';
import AnimatedAvatar, { coreColorOf } from '@/components/avatar/AnimatedAvatar';
import { DeclarativeArtwork } from '@/components/avatar/DeclarativeArtwork';
import { toAvatarDefinition, type PluginAvatarAssetBundle } from '@/components/avatar/avatarRegistry';
import { defaultAppearance } from '@/components/butler/butlerAppearance';
import ConfirmDialog from '../../ConfirmDialog';
import AppStatusBadge from '../apps/AppStatusBadge';
import { InfoList, Notice } from '../catalog/DetailParts';
import PluginAdvancedPanel from './PluginAdvancedPanel';
import { OfficialBadge, PluginTypeIcon } from './PluginCard';
import { PluginPreviewGallery, pluginTypeIcon } from './PluginPreview';
import { contributionSummary, describeContributions, describePermissions } from './pluginDescribe';
import { pluginDisplayStatus, pluginTypeLabel, runtimeProblem, sourceLabel, type InstalledPlugin, type PluginEntry, type PluginInstallState } from './pluginCatalog';

export const PLUGIN_CENTER_VIEW_LOCATION = 'config.plugin-center';
const tabClass = '-mb-px h-9 flex-none rounded-none border-0 border-b-2 border-transparent px-0 shadow-none data-[state=active]:border-foreground data-[state=active]:bg-transparent data-[state=active]:shadow-none dark:data-[state=active]:bg-transparent';

export interface PluginDetailActions {
    install: (entry: PluginEntry, useProxy?: boolean) => Promise<void>;
    toggle: (plugin: InstalledPlugin) => Promise<void>;
    uninstall: (plugin: InstalledPlugin) => Promise<void>;
    retryLoad: (plugin: InstalledPlugin) => Promise<void>;
    openFolder: () => void;
}

function repositoryUrl(entry: PluginEntry): string | undefined {
    const source = entry.installed?.installSource;
    if (source?.kind === 'github') return `https://github.com/${source.sourceLabel.split('#')[0]}`;
    const official = entry.official;
    if (official?.sourceUrl) return official.sourceUrl;
    if (official?.source?.type === 'github' && official.source.repo) return `https://github.com/${official.source.repo}`;
    return undefined;
}

function PermissionGuide({ detail }: { detail: string }) {
    const platform = usePlatform();
    return <>
        <p>{platform === 'macos'
            ? '在访达中选中下方路径对应的项目，打开「显示简介 → 共享与权限」，确认当前用户可以读取。如果之前拒绝过 AIPP 访问该位置，可在「系统设置 → 隐私与安全 → 文件和文件夹」打开对应开关，然后重试。'
            : platform === 'windows'
                ? '打开下方路径对应文件或文件夹的「属性 → 安全」，确认当前用户有读取权限，然后重试。'
                : '请检查下方路径对应文件及其所在目录的读取权限，然后重试。'}</p>
        <p className="mt-1 whitespace-pre-wrap break-all font-mono text-xs">{detail}</p>
    </>;
}

function AvatarAssets({ plugin }: { plugin: InstalledPlugin }) {
    const [bundle, setBundle] = useState<PluginAvatarAssetBundle | null>(null);
    const [error, setError] = useState('');
    useEffect(() => {
        let active = true;
        invoke<PluginAvatarAssetBundle>('get_plugin_avatar_assets', { code: plugin.code })
            .then(value => { if (active) { setBundle(value); setError(''); } })
            .catch(e => { if (active) setError(String(e)); });
        return () => { active = false; };
    }, [plugin.code]);
    if (error) return <Notice tone="error" icon={<AlertTriangle />} title="形象与配饰资产校验失败">{error}</Notice>;
    if (!bundle || (!bundle.avatars.length && !bundle.accessories.length)) return null;
    const palette = { primary: defaultAppearance.colors.primary, secondary: defaultAppearance.colors.secondary, core: coreColorOf(defaultAppearance) };
    return <section className="space-y-4" data-aipp-slot="plugin-avatar-preview">
        {bundle.avatars.length > 0 && <div className="space-y-3">
            <h4 className="text-sm font-medium">包含的助手形象（{bundle.avatars.length}）</h4>
            <div className="grid grid-cols-3 gap-3 sm:grid-cols-4">{bundle.avatars.map(asset => {
                const def = toAvatarDefinition(asset);
                if (!def) return null;
                const appearance = { ...defaultAppearance, shape: def.id, colors: {
                    primary: def.defaultColors?.primary ?? defaultAppearance.colors.primary,
                    secondary: def.defaultColors?.secondary ?? defaultAppearance.colors.secondary,
                } };
                return <div key={asset.id} className="flex flex-col items-center gap-2 rounded-lg border bg-muted/30 p-3 text-center">
                    <AnimatedAvatar appearance={appearance} definitionOverride={def} size={64} animated state="idle" />
                    <span className="text-xs font-medium">{asset.label}</span>
                </div>;
            })}</div>
        </div>}
        {bundle.accessories.length > 0 && <div className="space-y-3">
            <h4 className="text-sm font-medium">包含的配饰（{bundle.accessories.length}）</h4>
            <div className="grid grid-cols-3 gap-3 sm:grid-cols-4">{bundle.accessories.map(asset =>
                <div key={asset.id} className="flex flex-col items-center gap-2 rounded-lg border bg-muted/30 p-3 text-center">
                    <svg aria-hidden="true" viewBox={asset.previewViewBox} className="size-12"><DeclarativeArtwork layers={asset.layers} palette={palette} /></svg>
                    <span className="text-xs font-medium">{asset.label}</span>
                    <span className="text-[10px] text-muted-foreground">{asset.slot === 'eyes' ? '脸部' : '头部'}</span>
                </div>)}</div>
        </div>}
        <p className="text-xs text-muted-foreground">{plugin.isActive ? '已启用，可在助手资料的「形象」面板中选择使用。' : '启用此插件后，即可在助手资料的「形象」面板中选择使用。'}</p>
    </section>;
}

/** 「设置」：插件自己提供的界面（config.plugin-center 视图）。 */
function PluginSettings({ plugin, loaded, onRetry, busy }: { plugin: InstalledPlugin; loaded?: LoadedPlugin; onRetry: () => void; busy: boolean }) {
    const blocked = !plugin.isInstalled ? '插件目录缺失，无法加载插件界面。'
        : !plugin.isActive ? '插件已停用，启用后可以使用插件界面。'
            : (plugin.runtime?.type ?? 'js') !== 'js' ? '这个插件不是 JS 运行时，没有插件界面。'
                : !loaded ? '插件运行时还没有加载这个插件。'
                    : loaded.loadError ? loaded.loadError
                        : !loaded.instance ? '插件加载失败（实例为空），请检查插件脚本导出。'
                            : typeof (loaded.instance as { renderView?: unknown }).renderView !== 'function' ? '插件已加载，但没有实现 renderView()。'
                                : null;
    if (blocked) return <div className="space-y-3 rounded-lg border px-4 py-3 text-sm text-muted-foreground">
        <p>{blocked}</p>
        {plugin.isInstalled && plugin.isActive && <Button size="sm" variant="outline" disabled={busy} onClick={onRetry}><RefreshCcw className={busy ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />刷新插件运行时</Button>}
    </div>;
    return <div className="space-y-2">
        <p className="text-xs text-muted-foreground">以下内容由插件提供</p>
        <PluginViewHost pluginList={[loaded!]} location={PLUGIN_CENTER_VIEW_LOCATION} emptyDescription="当前插件没有声明插件界面。" />
    </div>;
}

export default function PluginDetailSheet({ entry, install, loaded, mobile, busy, actions, onClose }: {
    entry?: PluginEntry;
    install?: PluginInstallState;
    loaded?: LoadedPlugin;
    mobile: boolean;
    busy: boolean;
    actions: PluginDetailActions;
    onClose: () => void;
}) {
    const [tab, setTab] = useState('overview');
    const [uninstalling, setUninstalling] = useState(false);
    const [confirmUninstall, setConfirmUninstall] = useState(false);
    const plugin = entry?.installed;
    const official = entry?.official;
    const status = entry ? pluginDisplayStatus(entry, { install, loaded, mobile }) : undefined;
    const problem = plugin ? runtimeProblem(plugin, loaded) : undefined;
    const repo = entry ? repositoryUrl(entry) : undefined;
    const installing = install?.status === 'installing';
    const hasSettings = !!plugin?.contributions?.views?.some(view => view.location === PLUGIN_CENTER_VIEW_LOCATION);
    const currentTab = tab === 'settings' && !hasSettings ? 'overview' : tab;
    const canInstallOfficial = !!official?.source && !installing;
    const blockedOnDevice = mobile && !!plugin?.mobileUnavailableReason;

    const close = () => { setTab('overview'); onClose(); };
    const openRepo = () => { if (repo) void invoke('open_source_url', { url: repo }).catch(e => toast.error(`打开仓库失败：${String(e)}`)); };
    const installOfficial = (useProxy = false) => { if (entry) void actions.install(entry, useProxy); };

    const notice = (() => {
        if (!entry) return null;
        if (install?.status === 'failed') return <Notice tone="error" icon={<AlertTriangle />} title="安装失败" actions={<>
            <Button size="sm" variant="outline" disabled={!canInstallOfficial} onClick={() => installOfficial()}>重试</Button>
            <Button size="sm" variant="ghost" disabled={!canInstallOfficial} onClick={() => installOfficial(true)}><RefreshCcw className="h-4 w-4" />使用代理重试</Button>
        </>}>{install.error}</Notice>;
        if (!plugin) return null;
        if (!plugin.isInstalled) return <Notice tone="error" icon={<AlertTriangle />} title="插件目录缺失" actions={<>
            {official && <Button size="sm" variant="outline" disabled={!canInstallOfficial} onClick={() => installOfficial()}>重新安装</Button>}
            {!mobile && <Button size="sm" variant="ghost" onClick={actions.openFolder}><FolderOpen className="h-4 w-4" />打开插件文件夹</Button>}
            <Button size="sm" variant="ghost" className="text-destructive hover:text-destructive" onClick={() => setConfirmUninstall(true)}>卸载</Button>
        </>}>plugin/{plugin.code} 不存在，插件不会加载。可以重新安装，或卸载这条记录。</Notice>;
        if (problem) return <Notice tone="error" icon={<AlertTriangle />} title={problem.fileAccessDenied ? '系统拒绝读取插件文件' : '插件加载失败'} actions={<>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => void actions.retryLoad(plugin)}>{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCcw className="h-4 w-4" />}重试加载</Button>
            {problem.fileAccessDenied && !mobile && <Button size="sm" variant="ghost" onClick={actions.openFolder}><FolderOpen className="h-4 w-4" />打开插件文件夹</Button>}
        </>}>{problem.fileAccessDenied ? <PermissionGuide detail={problem.message} /> : <p className="whitespace-pre-wrap break-all">{problem.message}</p>}</Notice>;
        if (blockedOnDevice) return <Notice icon={<CircleSlash />} title="此设备不能运行">{plugin.mobileUnavailableReason}。配置仍保留。</Notice>;
        if (entry.updateVersion) return <Notice tone="warn" icon={<ArrowUpRight />} title={`${entry.updateVersion} 可更新`} actions={
            <Button size="sm" disabled={!canInstallOfficial} onClick={() => installOfficial()}>{installing && <Loader2 className="h-4 w-4 animate-spin" />}更新到 {entry.updateVersion}</Button>}>
            更新会保留插件配置与数据。</Notice>;
        if (entry.thirdPartyShadowsOfficial || (plugin.installSource && plugin.installSource.kind !== 'official')) return <Notice icon={<ShieldAlert />} title="非官方插件" actions={
            <>{repo && <Button size="sm" variant="outline" onClick={openRepo}>前往仓库<ArrowUpRight className="h-4 w-4" /></Button>}
                {entry.thirdPartyShadowsOfficial && <Button size="sm" variant="ghost" disabled={!canInstallOfficial} onClick={() => installOfficial()}>改装官方版本</Button>}</>}>
            {sourceLabel(entry)}。AIPP 没有审核过它的代码，启用前请确认来源可信。</Notice>;
        if (entry.unknownSource && official) return <Notice icon={<Info />} title="来源未记录" actions={
            <Button size="sm" variant="outline" disabled={!canInstallOfficial} onClick={() => installOfficial()}>从市场重新安装</Button>}>
            官方目录里有同名插件，但本机没有这次安装的来源记录。从市场重新安装一次即可确认是官方版本。</Notice>;
        return null;
    })();

    const headerActions = (() => {
        if (!entry) return null;
        const menu = [
            repo && <DropdownMenuItem key="repo" onSelect={openRepo}>前往仓库</DropdownMenuItem>,
            !plugin && official && <DropdownMenuItem key="proxy" disabled={!canInstallOfficial} onSelect={() => installOfficial(true)}>使用代理安装</DropdownMenuItem>,
            plugin && !mobile && <DropdownMenuItem key="folder" onSelect={actions.openFolder}>打开插件文件夹</DropdownMenuItem>,
            plugin && official && plugin.isInstalled && !entry.updateVersion && <DropdownMenuItem key="reinstall" disabled={!canInstallOfficial} onSelect={() => installOfficial()}>重新安装</DropdownMenuItem>,
        ].filter(Boolean);
        return <div className="flex shrink-0 items-center gap-1">
            {!plugin && <Button size="sm" disabled={!canInstallOfficial} onClick={() => installOfficial()}>{installing && <Loader2 className="h-4 w-4 animate-spin" />}{installing ? '安装中' : '安装'}</Button>}
            {plugin && <Switch aria-label={plugin.isActive ? '停用插件' : '启用插件'} checked={plugin.isActive}
                disabled={busy || !plugin.isInstalled || (blockedOnDevice && !plugin.isActive)} onCheckedChange={() => void actions.toggle(plugin)} />}
            {(menu.length > 0 || plugin) && <DropdownMenu>
                <DropdownMenuTrigger asChild><Button variant="ghost" size="icon" className="h-8 w-8" aria-label="更多插件操作"><MoreHorizontal className="h-4 w-4" /></Button></DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                    {menu}
                    {plugin && <>{menu.length > 0 && <DropdownMenuSeparator />}
                        <DropdownMenuItem className="text-destructive" disabled={busy} onSelect={() => setConfirmUninstall(true)}><Trash2 className="h-4 w-4" />卸载插件</DropdownMenuItem></>}
                </DropdownMenuContent>
            </DropdownMenu>}
        </div>;
    })();

    const overview = entry && <div className="space-y-8">
        <PluginPreviewGallery entry={entry} />
        <p className="text-sm leading-7">{entry.description || '暂无描述'}</p>
        {plugin && (plugin.contributions?.avatars?.length || plugin.contributions?.accessories?.length) && plugin.isInstalled ? <AvatarAssets plugin={plugin} /> : null}
        <section className="space-y-3">
            <h4 className="text-sm font-medium">{plugin ? '出现在哪里' : '装上后会出现在'}</h4>
            <div className="divide-y rounded-lg border text-sm">{describeContributions(entry).map(line => {
                const Icon = pluginTypeIcon(entry.types.find(type => pluginTypeLabel(type) === line.where) ?? entry.primaryType);
                return <div key={`${line.where}-${line.what}`} className="flex items-center gap-3 px-4 py-2.5">
                    <Icon className="h-4 w-4 shrink-0 text-muted-foreground" /><span className="w-16 shrink-0 font-medium">{line.where}</span><span className="min-w-0 text-muted-foreground">{line.what}</span>
                </div>;
            })}</div>
        </section>
        {(() => {
            const permissions = describePermissions(plugin?.permissions ?? official?.permissions);
            return permissions.length > 0 && <section className="space-y-3">
                <h4 className="text-sm font-medium">需要的权限</h4>
                <ul className="space-y-2 text-sm">{permissions.map(text => <li key={text} className="flex items-center gap-2"><span className="h-1 w-1 shrink-0 rounded-full bg-muted-foreground" />{text}</li>)}</ul>
            </section>;
        })()}
        <InfoList rows={[
            ['提供方', <span className="inline-flex items-center gap-2">{entry.verifiedOfficial ? entry.author || 'AIPP' : sourceLabel(entry)}{entry.verifiedOfficial && <OfficialBadge />}</span>],
            ['分类', <>{pluginTypeLabel(entry.primaryType)}{entry.types.length > 1 && <span className="ml-2 text-muted-foreground">同时包含：{entry.types.filter(type => type !== entry.primaryType).map(pluginTypeLabel).join('、')}</span>}</>],
            ['版本', plugin ? `已安装 ${plugin.version}${official?.version && official.version !== plugin.version ? ` · 最新 ${official.version}` : ''}` : official?.version ?? '—'],
            ...(official?.minAippVersion ? [['最低版本', `AIPP ${official.minAippVersion}`] as [string, ReactNode]] : []),
            ...(repo ? [['来源', <button type="button" className="inline-flex items-center gap-1 underline-offset-4 hover:underline" onClick={openRepo}>{repo.replace(/^https:\/\/(github\.com\/)?/, '')}<ArrowUpRight className="h-3.5 w-3.5" /></button>] as [string, ReactNode]] : []),
            ...(plugin?.installSource ? [['安装方式', `${plugin.installSource.kind === 'official' ? '从插件市场安装，安装包已校验' : '从来源链接安装'} · ${plugin.installSource.installedAt.slice(0, 10)}`] as [string, ReactNode]] : []),
        ]} />
    </div>;

    return <>
        <Sheet open={!!entry} onOpenChange={open => { if (!open) close(); }}>
            <SheetContent className="w-full gap-0 overflow-hidden p-0 sm:max-w-2xl">
                {entry && status && <ConfigSheetBody>
                    <SheetHeader className="space-y-4 border-b px-6 pb-0 pt-6">
                        <div className="flex items-start gap-4 pr-6">
                            <PluginTypeIcon type={entry.primaryType} size="lg" />
                            <div className="min-w-0 flex-1 space-y-1">
                                <SheetTitle className="flex min-w-0 items-center gap-2 text-lg"><span className="truncate">{entry.name}</span>{entry.verifiedOfficial && <OfficialBadge large />}</SheetTitle>
                                <SheetDescription className="truncate">{[entry.verifiedOfficial ? entry.author || 'AIPP' : sourceLabel(entry), pluginTypeLabel(entry.primaryType), plugin ? `已安装 ${plugin.version}` : official?.version && `版本 ${official.version}`].filter(Boolean).join(' · ')}</SheetDescription>
                                <div className="flex flex-wrap items-center gap-x-3 gap-y-1 pt-1 text-xs text-muted-foreground">
                                    <AppStatusBadge status={status} />
                                    {plugin && contributionSummary(plugin) && <><span aria-hidden>·</span><span>{contributionSummary(plugin)}</span></>}
                                </div>
                            </div>
                            {headerActions}
                        </div>
                        {notice}
                        {plugin ? <Tabs value={currentTab} onValueChange={setTab}>
                            <TabsList className="h-auto gap-4 rounded-none bg-transparent p-0">
                                <TabsTrigger value="overview" className={tabClass}>概览</TabsTrigger>
                                {hasSettings && <TabsTrigger value="settings" className={tabClass}>设置</TabsTrigger>}
                                <TabsTrigger value="advanced" className={tabClass}>高级</TabsTrigger>
                            </TabsList>
                        </Tabs> : <div className="pb-2" />}
                    </SheetHeader>
                    <div className="px-6 py-6">
                        {plugin ? <Tabs value={currentTab} onValueChange={setTab}>
                            <TabsContent value="overview" className="mt-0">{overview}</TabsContent>
                            {hasSettings && <TabsContent value="settings" className="mt-0"><PluginSettings plugin={plugin} loaded={loaded} busy={busy} onRetry={() => void actions.retryLoad(plugin)} /></TabsContent>}
                            <TabsContent value="advanced" className="mt-0"><PluginAdvancedPanel plugin={plugin} /></TabsContent>
                        </Tabs> : overview}
                    </div>
                </ConfigSheetBody>}
            </SheetContent>
        </Sheet>
        <ConfirmDialog isOpen={confirmUninstall && !!plugin} title="卸载插件"
            confirmText={`卸载后会删除插件「${plugin?.name ?? ''}」的文件夹和数据库记录，且不可恢复。`}
            onCancel={() => { if (!uninstalling) setConfirmUninstall(false); }}
            onConfirm={() => {
                if (!plugin || uninstalling) return;
                setUninstalling(true);
                void actions.uninstall(plugin).then(() => { setConfirmUninstall(false); close(); }).finally(() => setUninstalling(false));
            }} />
    </>;
}
