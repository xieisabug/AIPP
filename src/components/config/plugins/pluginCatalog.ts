import type { LoadedPlugin } from '@/services/PluginRuntime';
import type { DisplayStatus } from '../apps/appStatus';

export interface PluginInstallSource {
    folderName: string;
    kind: 'official' | 'github' | 'zip' | 'localZip' | string;
    officialId?: string | null;
    sourceLabel: string;
    archiveSha256?: string | null;
    installedAt: string;
}

export interface PluginContributionsSummary {
    bangs?: Array<{ name: string; aliases?: string[]; description?: string | null }>;
    hooks?: Array<{ name: string; kind?: string; priority?: number; timeoutMs?: number; failurePolicy?: string; isActive?: boolean }>;
    views?: Array<{ id: string; location: string; title: string; description?: string | null }>;
    actions?: Array<{ id: string; location: string; title: string; description?: string | null }>;
    slots?: Array<{ id: string; location: string }>;
    assistantFormFields?: Array<{ key: string; label: string; type: string }>;
    avatars?: unknown[];
    accessories?: unknown[];
}

export interface InstalledPlugin {
    entryAccessError?: string | null;
    mobileUnavailableReason?: string | null;
    pluginId: number;
    name: string;
    version: string;
    code: string;
    description?: string | null;
    author?: string | null;
    pluginType: string[];
    permissions?: string[];
    runtime?: { type: string; entry: string; protocol?: string | null; checksum?: string | null } | null;
    contributions?: PluginContributionsSummary;
    isActive: boolean;
    isInstalled: boolean;
    installSource?: PluginInstallSource | null;
    preview?: string[];
}

export interface PluginInstallRecipeSource {
    type: 'github' | 'zip' | 'localZip';
    repo?: string | null;
    ref?: string;
    url?: string | null;
    path?: string | null;
}

export interface PluginInstallRecipeDir { from: string; to: string }

export interface OfficialPlugin {
    id: string;
    code: string;
    name: string;
    description: string;
    version?: string | null;
    author?: string | null;
    tags?: string[];
    pluginTypes: string[];
    permissions: string[];
    minAippVersion?: string | null;
    isExperimental?: boolean;
    source?: PluginInstallRecipeSource | null;
    dirs?: PluginInstallRecipeDir[];
    sourceUrl?: string | null;
    sha256?: string | null;
    isInstalled?: boolean;
    installedVersion?: string | null;
    isActive?: boolean;
    previewImages?: string[];
}

/**
 * AIPP 允许的 9 种插件类型（与 plugin_api.rs normalize_plugin_type_name 一致），按「主分类」优先级排列：
 * 一个插件声明多个类型时取排在最前的那个。大多数插件会顺带声明 interfaceType / applicationType，
 * 所以这两个放在最后，否则「界面」会吞掉几乎所有插件。
 */
export const PLUGIN_TYPES = [
    { code: 'themeType', label: '主题' },
    { code: 'avatarType', label: '形象' },
    { code: 'markdownType', label: '消息渲染' },
    { code: 'toolType', label: '快捷命令' },
    { code: 'messageType', label: '消息' },
    { code: 'exportType', label: '导出' },
    { code: 'assistantType', label: '助手' },
    { code: 'interfaceType', label: '界面' },
    { code: 'applicationType', label: '后台' },
] as const;

export type PluginTypeCode = (typeof PLUGIN_TYPES)[number]['code'];
const TYPE_ORDER: string[] = PLUGIN_TYPES.map(type => type.code);

export const pluginTypeLabel = (code: string) => PLUGIN_TYPES.find(type => type.code === code)?.label ?? code;

export function primaryPluginType(types: string[] | undefined): string {
    const known = (types ?? []).filter(type => TYPE_ORDER.includes(type));
    if (!known.length) return types?.[0] ?? 'assistantType';
    return known.reduce((best, type) => TYPE_ORDER.indexOf(type) < TYPE_ORDER.indexOf(best) ? type : best);
}

/** 只比较数字段；`0.2.0` > `0.1.10`，忽略预发布后缀之外的字符。 */
export function compareVersions(a: string, b: string): number {
    const parse = (value: string) => value.replace(/^v/i, '').split(/[.+-]/).map(part => Number.parseInt(part, 10));
    const left = parse(a); const right = parse(b);
    for (let i = 0; i < Math.max(left.length, right.length); i++) {
        const x = Number.isFinite(left[i]) ? left[i] : 0;
        const y = Number.isFinite(right[i]) ? right[i] : 0;
        if (x !== y) return x > y ? 1 : -1;
    }
    return 0;
}

/** 市场与「我的插件」共用的条目：同一 code 的官方目录条目与本机安装合并成一条。 */
export interface PluginEntry {
    code: string;
    name: string;
    description: string;
    author?: string | null;
    types: string[];
    primaryType: string;
    official?: OfficialPlugin;
    installed?: InstalledPlugin;
    /** 本机安装经后端核对为官方；未安装时看是否来自官方目录。 */
    verifiedOfficial: boolean;
    /** 已安装但没有安装来源记录（老版本安装或手动放入目录）。 */
    unknownSource: boolean;
    /** 已安装的是第三方来源，而官方目录有同名插件。 */
    thirdPartyShadowsOfficial: boolean;
    updateVersion?: string;
}

export function buildPluginEntries(official: OfficialPlugin[], installed: InstalledPlugin[]): PluginEntry[] {
    const byCode = new Map<string, PluginEntry>();
    const make = (code: string, o?: OfficialPlugin, i?: InstalledPlugin): PluginEntry => {
        const types = i?.pluginType?.length ? i.pluginType : o?.pluginTypes ?? [];
        const kind = i?.installSource?.kind;
        const unknownSource = !!i && !i.installSource;
        const fromOfficial = kind === 'official' && (!o || i?.installSource?.officialId === o.id);
        const updatable = !!i && !!o?.version && (fromOfficial || unknownSource) && compareVersions(o.version, i.version) > 0;
        return {
            code,
            name: i?.name ?? o?.name ?? code,
            description: (i?.description || o?.description) ?? '',
            author: i?.author ?? o?.author,
            types,
            primaryType: primaryPluginType(types),
            official: o,
            installed: i,
            verifiedOfficial: i ? kind === 'official' : !!o,
            unknownSource,
            thirdPartyShadowsOfficial: !!i && !!o && !!kind && kind !== 'official',
            updateVersion: updatable ? o?.version ?? undefined : undefined,
        };
    };
    for (const item of official) byCode.set(item.code, make(item.code, item));
    for (const item of installed) byCode.set(item.code, make(item.code, byCode.get(item.code)?.official, item));
    return [...byCode.values()];
}

export type PluginInstallState = { status: 'installing' } | { status: 'failed'; error: string };

export interface PluginRuntimeProblem { message: string; fileAccessDenied: boolean }

/** 已安装插件当前有没有加载问题；目录缺失单独判断。 */
export function runtimeProblem(plugin: InstalledPlugin, loaded?: LoadedPlugin): PluginRuntimeProblem | undefined {
    if (!plugin.isActive) return undefined;
    if (plugin.entryAccessError) return { message: plugin.entryAccessError, fileAccessDenied: true };
    if (loaded?.loadError) return { message: loaded.loadError, fileAccessDenied: !!loaded.fileAccessDenied };
    return undefined;
}

/** 卡片底栏、列表与详情头部共用的一个状态。 */
export function pluginDisplayStatus(entry: PluginEntry, opts: { install?: PluginInstallState; loaded?: LoadedPlugin; mobile?: boolean } = {}): DisplayStatus {
    if (opts.install?.status === 'installing') return { label: '安装中…', tone: 'pending', attention: false };
    if (opts.install?.status === 'failed') return { label: '安装失败', tone: 'error', attention: true };
    const plugin = entry.installed;
    if (!plugin) return { label: '未安装', tone: 'idle', attention: false };
    if (!plugin.isInstalled) return { label: '插件目录缺失', tone: 'error', attention: true };
    if (runtimeProblem(plugin, opts.loaded)) return { label: '加载失败', tone: 'error', attention: true };
    if (opts.mobile && plugin.mobileUnavailableReason) return { label: '此设备不可运行', tone: 'idle', attention: false };
    if (entry.updateVersion) return { label: `${entry.updateVersion} 可更新`, tone: 'warn', attention: true };
    return plugin.isActive ? { label: '已启用', tone: 'ok', attention: false } : { label: '已停用', tone: 'idle', attention: false };
}

/** 卡片右下角的动作词。 */
export function pluginCardAction(entry: PluginEntry, status: DisplayStatus): string | undefined {
    if (status.tone === 'pending') return undefined;
    if (!entry.installed) return status.tone === 'error' ? '重试' : '查看';
    if (status.tone === 'error') return '修复';
    if (entry.updateVersion) return '更新';
    return '管理';
}

export function matchesPluginQuery(entry: PluginEntry, query: string): boolean {
    const q = query.trim().toLowerCase();
    if (!q) return true;
    return [entry.name, entry.description, entry.code, entry.author ?? '', ...entry.types.map(pluginTypeLabel)]
        .some(text => text.toLowerCase().includes(q));
}

export function sourceLabel(entry: PluginEntry): string {
    const source = entry.installed?.installSource;
    if (!entry.installed) return entry.author || 'AIPP';
    if (!source) return '来源未记录';
    if (source.kind === 'official') return entry.author || 'AIPP';
    const where = source.kind === 'github' ? 'GitHub' : source.kind === 'zip' ? 'ZIP 链接' : '本地 ZIP';
    return `来自 ${where} · ${source.sourceLabel}`;
}

/** 解析用户粘贴的来源：owner/repo、owner/repo#ref、GitHub 仓库 / tree 链接、ZIP 链接。 */
export function parsePluginSourceInput(input: string): { source: PluginInstallRecipeSource; dirs?: PluginInstallRecipeDir[] } {
    const value = input.trim();
    if (!value) throw new Error('请输入 GitHub 仓库或 ZIP 链接');
    if (/^https?:\/\/.+\.zip(\?.*)?$/i.test(value)) return { source: { type: 'zip', url: value } };
    if (/^[a-zA-Z]:[\\/]|^\//.test(value) && /\.zip$/i.test(value)) return { source: { type: 'localZip', path: value } };
    const shorthand = value.match(/^([\w.-]+\/[\w.-]+)(?:#(.+))?$/);
    if (shorthand) return { source: { type: 'github', repo: shorthand[1], ref: shorthand[2] || 'main' } };
    let url: URL;
    try { url = new URL(value); } catch { throw new Error('无法识别的来源，请输入 owner/repo、GitHub 链接或 ZIP 链接'); }
    if (url.hostname !== 'github.com') throw new Error('只支持 GitHub 仓库链接或 ZIP 下载链接');
    const segments = url.pathname.split('/').filter(Boolean);
    if (segments.length < 2) throw new Error('GitHub 链接必须包含 owner/repo');
    const repo = `${segments[0]}/${segments[1]}`;
    if (segments[2] === 'tree' && segments[3]) {
        const path = segments.slice(4).join('/');
        return {
            source: { type: 'github', repo, ref: segments[3] },
            dirs: path ? [{ from: path, to: path.split('/').filter(Boolean).pop() || segments[1] }] : undefined,
        };
    }
    return { source: { type: 'github', repo, ref: 'main' } };
}
