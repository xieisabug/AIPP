import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Bot, Download, FileText, Layers, LayoutPanelTop, MessageSquareText, Palette, Smile, SquareTerminal, type LucideIcon } from 'lucide-react';
import { pluginRuntime } from '@/services/PluginRuntime';
import AnimatedAvatar from '@/components/avatar/AnimatedAvatar';
import { toAvatarDefinition, type PluginAvatarAssetBundle } from '@/components/avatar/avatarRegistry';
import { defaultAppearance } from '@/components/butler/butlerAppearance';
import { cn } from '@/utils/utils';
import { pluginTypeLabel, type PluginEntry } from './pluginCatalog';

const TYPE_ICONS: Record<string, LucideIcon> = {
    themeType: Palette, avatarType: Smile, markdownType: MessageSquareText, toolType: SquareTerminal, messageType: MessageSquareText,
    exportType: Download, assistantType: Bot, interfaceType: LayoutPanelTop, applicationType: Layers,
};
export const pluginTypeIcon = (type: string): LucideIcon => TYPE_ICONS[type] ?? FileText;

const TRUSTED_PREVIEW_PREFIX = 'https://aipp-helper.xiejingyang.com/plugin-previews/';
// 预览数据在插件页存活期间复用，切换 tab / 重开详情不重复读取。
const localImages = new Map<string, Promise<string>>();
const avatarBundles = new Map<string, Promise<PluginAvatarAssetBundle>>();
const once = <T,>(cache: Map<string, Promise<T>>, key: string, load: () => Promise<T>) => {
    if (!cache.has(key)) cache.set(key, load().catch(error => { cache.delete(key); throw error; }));
    return cache.get(key)!;
};

/**
 * 插件的预览图来源，按优先级：本机实时渲染（主题 / 形象）→ 插件自带图片 → 官方目录图片 → 按类型的通用插画。
 * 官方插件包内的 `preview` 与目录的 `previewImages` 是同一组图（打包时导出给 helper），
 * 所以包内有图时不再拼接目录图，否则详情里每张图会出现两次。
 */
export function previewImageSources(entry: PluginEntry): { local: string[]; remote: string[] } {
    const installed = entry.installed?.isInstalled ? entry.installed : undefined;
    const local = installed?.preview ?? [];
    return {
        local,
        remote: local.length ? [] : (entry.official?.previewImages ?? []).filter(url => url.startsWith(TRUSTED_PREVIEW_PREFIX)),
    };
}

function usePreviewImage(entry: PluginEntry, index: number) {
    const { local, remote } = previewImageSources(entry);
    const localPath = local[index];
    const [src, setSrc] = useState<string>();
    useEffect(() => {
        setSrc(undefined);
        if (!localPath) return;
        let active = true;
        once(localImages, `${entry.code}:${entry.installed?.version}:${localPath}`, () => invoke<string>('get_plugin_preview_image', { code: entry.code, path: localPath }))
            .then(url => { if (active) setSrc(url); })
            .catch(error => console.warn('读取插件预览图失败', entry.code, error));
        return () => { active = false; };
    }, [entry.code, entry.installed?.version, localPath]);
    return localPath ? src : remote[index - local.length];
}

function ThemeSwatches({ variables }: { variables: Record<string, string> }) {
    const color = (key: string) => variables[key] ? `hsl(${variables[key]})` : undefined;
    const swatches = ['--primary', '--background', '--secondary', '--foreground'].map(color).filter(Boolean) as string[];
    return <div className="flex items-center gap-3" aria-hidden>
        <div className="flex gap-1">{swatches.map(value => <span key={value} className="h-3.5 w-3.5 rounded-[4px] border border-black/5" style={{ background: value }} />)}</div>
        <div className="flex w-28 flex-col gap-1.5">
            <span className="ml-auto h-2.5 w-3/5 rounded-full" style={{ background: color('--primary') }} />
            <span className="h-2.5 w-4/5 rounded-full" style={{ background: color('--secondary') ?? color('--muted') }} />
        </div>
    </div>;
}

function AvatarPreview({ code, size }: { code: string; size: number }) {
    const [bundle, setBundle] = useState<PluginAvatarAssetBundle>();
    useEffect(() => {
        let active = true;
        once(avatarBundles, code, () => invoke<PluginAvatarAssetBundle>('get_plugin_avatar_assets', { code }))
            .then(value => { if (active) setBundle(value); })
            .catch(error => console.warn('读取形象预览失败', code, error));
        return () => { active = false; };
    }, [code]);
    const definition = bundle?.avatars[0] ? toAvatarDefinition(bundle.avatars[0]) : undefined;
    if (!definition) return null;
    const appearance = { ...defaultAppearance, shape: definition.id, colors: {
        primary: definition.defaultColors?.primary ?? defaultAppearance.colors.primary,
        secondary: definition.defaultColors?.secondary ?? defaultAppearance.colors.secondary,
    } };
    return <AnimatedAvatar appearance={appearance} definitionOverride={definition} size={size} animated={false} state="idle" />;
}

function TypeIllustration({ type }: { type: string }) {
    const Icon = pluginTypeIcon(type);
    return <div className="flex flex-col items-center gap-1 text-[11px] text-muted-foreground/70">
        <Icon className="h-6 w-6" strokeWidth={1.5} />
        <span>{pluginTypeLabel(type)}插件</span>
    </div>;
}

/** 卡片与详情顶部的效果预览区。 */
export default function PluginPreview({ entry, index = 0, className, large = false }: { entry: PluginEntry; index?: number; className?: string; large?: boolean }) {
    const loadedImage = usePreviewImage(entry, index);
    const [failed, setFailed] = useState<string>();
    const image = loadedImage && loadedImage !== failed ? loadedImage : undefined;
    const installed = entry.installed?.isInstalled ? entry.installed : undefined;
    // 主题要插件运行后才注册变量，只有启用中的主题能实时画；形象资产直接读文件，停用时也能预览。
    const themes = index === 0 && installed?.isActive && entry.primaryType === 'themeType' ? pluginRuntime.listThemeVariablesOf(entry.code) : [];
    const avatar = index === 0 && !image && !!installed && (installed.contributions?.avatars?.length ?? 0) > 0;
    const live = themes.length > 0 || avatar;
    const fallback = !image && !live;
    return <div data-slot="plugin-preview" className={cn('grid w-full place-items-center overflow-hidden rounded-lg border border-border/60 bg-muted/40',
        large ? 'h-56' : 'h-[84px]', fallback && 'bg-[radial-gradient(hsl(var(--border))_1px,transparent_1px)] [background-size:12px_12px]', className)}>
        {image ? <img src={image} alt={`${entry.name} 预览`} className="h-full w-full object-cover" draggable={false} onError={() => setFailed(image)} />
            : themes.length ? <ThemeSwatches variables={themes[0]} />
            : avatar ? <AvatarPreview code={entry.code} size={large ? 140 : 64} />
            : <TypeIllustration type={entry.primaryType} />}
    </div>;
}

/** 详情抽屉的预览：多张图时可以切换；没有真实预览（只能显示通用插画）时不占位置。 */
export function PluginPreviewGallery({ entry }: { entry: PluginEntry }) {
    const { local, remote } = previewImageSources(entry);
    const images = local.length + remote.length;
    const installed = entry.installed?.isInstalled ? entry.installed : undefined;
    const live = !!installed && ((installed.contributions?.avatars?.length ?? 0) > 0
        || (installed.isActive && entry.primaryType === 'themeType' && pluginRuntime.listThemeVariablesOf(entry.code).length > 0));
    const count = Math.max(1, images);
    const [index, setIndex] = useState(0);
    if (!images && !live) return null;
    return <div className="space-y-2">
        <PluginPreview entry={entry} index={index} large />
        {count > 1 && <div className="flex justify-center gap-1.5" role="tablist" aria-label="预览图">
            {Array.from({ length: count }, (_, i) => <button key={i} type="button" role="tab" aria-selected={i === index} aria-label={`第 ${i + 1} 张预览`}
                className={cn('h-1.5 rounded-full transition-all', i === index ? 'w-4 bg-foreground' : 'w-1.5 bg-muted-foreground/30')} onClick={() => setIndex(i)} />)}
        </div>}
    </div>;
}
