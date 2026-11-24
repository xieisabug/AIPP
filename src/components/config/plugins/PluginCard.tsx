import { ShieldCheck } from 'lucide-react';
import type { LoadedPlugin } from '@/services/PluginRuntime';
import AppStatusBadge from '../apps/AppStatusBadge';
import CatalogCard, { CatalogTag } from '../catalog/CatalogCard';
import PluginPreview, { pluginTypeIcon } from './PluginPreview';
import { pluginCardAction, pluginDisplayStatus, pluginTypeLabel, type PluginEntry, type PluginInstallState } from './pluginCatalog';

/** 「官方」标记：只给官方目录条目和后端核对过的官方安装。 */
export function OfficialBadge({ large = false }: { large?: boolean }) {
    return <span title="AIPP 官方发布，安装包已校验" className={`inline-flex shrink-0 items-center gap-0.5 rounded border border-foreground/20 px-1.5 font-medium text-foreground ${large ? 'h-5 text-[11px]' : 'h-[18px] text-[10.5px]'}`}>
        <ShieldCheck className={large ? 'h-3 w-3' : 'h-[11px] w-[11px]'} strokeWidth={2.2} />官方
    </span>;
}

export function PluginTypeIcon({ type, size = 'xs' }: { type: string; size?: 'xs' | 'sm' | 'lg' }) {
    const Icon = pluginTypeIcon(type);
    const box = size === 'xs' ? 'h-6 w-6 rounded-md' : size === 'sm' ? 'h-9 w-9 rounded-lg' : 'h-14 w-14 rounded-2xl';
    const glyph = size === 'xs' ? 'h-3.5 w-3.5' : size === 'sm' ? 'h-4 w-4' : 'h-6 w-6';
    return <span className={`grid shrink-0 place-items-center border bg-muted/40 text-muted-foreground ${box}`}><Icon className={glyph} /></span>;
}

export default function PluginCard({ entry, install, loaded, mobile, onOpen }: {
    entry: PluginEntry; install?: PluginInstallState; loaded?: LoadedPlugin; mobile: boolean; onOpen: () => void;
}) {
    const status = pluginDisplayStatus(entry, { install, loaded, mobile });
    return <CatalogCard compactHeader descriptionLines={1} onClick={onOpen}
        media={<PluginPreview entry={entry} />}
        icon={<PluginTypeIcon type={entry.primaryType} />}
        name={entry.name}
        badges={<>{entry.verifiedOfficial && <OfficialBadge />}{entry.official?.isExperimental && <CatalogTag>实验</CatalogTag>}</>}
        trailing={<span className="shrink-0 text-xs text-muted-foreground">{pluginTypeLabel(entry.primaryType)}</span>}
        description={entry.description || '暂无描述'}
        status={<AppStatusBadge status={status} />}
        action={pluginCardAction(entry, status)} />;
}
