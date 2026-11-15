import { useState } from 'react';
import { AppWindow, Bot, Code2, Globe, KeyRound, MousePointerClick, Network, Notebook, Search, ShieldCheck, SquareTerminal, Wrench, type LucideIcon } from 'lucide-react';
import { cn } from '@/utils/utils';

export interface OfficialIcon { icon?: string | null; icon_dark?: string | null; icon_small?: string | null }

const sizes = {
    sm: { box: 'h-9 w-9 rounded-lg', glyph: 'h-4 w-4' },
    md: { box: 'h-11 w-11 rounded-xl', glyph: 'h-5 w-5' },
    lg: { box: 'h-14 w-14 rounded-2xl', glyph: 'h-6 w-6' },
};

const builtinIcons: Record<string, { glyph: LucideIcon; color: string }> = {
    'aipp:vault': { glyph: KeyRound, color: 'text-amber-700 dark:text-amber-400' },
    'aipp:agent': { glyph: Bot, color: 'text-indigo-600 dark:text-indigo-400' },
    'aipp:ui_interaction': { glyph: MousePointerClick, color: 'text-pink-600 dark:text-pink-400' },
    'aipp:search': { glyph: Search, color: 'text-sky-700 dark:text-sky-400' },
    'aipp:operation': { glyph: SquareTerminal, color: 'text-emerald-700 dark:text-emerald-400' },
    'aipp:artifact': { glyph: AppWindow, color: 'text-violet-600 dark:text-violet-400' },
    'aipp:superadmin': { glyph: ShieldCheck, color: 'text-orange-700 dark:text-orange-400' },
    'aipp:dynamic_mcp': { glyph: Network, color: 'text-teal-700 dark:text-teal-400' },
};

const TRUSTED_ICON_PREFIX = 'https://aipp-helper.xiejingyang.com/agent-plugin-icons/';

/** Helper-hosted official icon (light/dark) with a neutral glyph fallback. */
export default function AppIcon({ icon, builtinCommand, official, size = 'md' }: { icon?: string | null; builtinCommand?: string | null; official?: OfficialIcon; size?: keyof typeof sizes }) {
    const [failed, setFailed] = useState<string[]>([]);
    const builtin = builtinCommand ? builtinIcons[builtinCommand] : undefined;
    const Icon = builtinCommand ? builtin?.glyph ?? Wrench : icon === 'github' ? Code2 : icon === 'notebook' ? Notebook : Globe;
    const image = (url?: string | null) => url?.startsWith(TRUSTED_ICON_PREFIX) && !failed.includes(url)
        ? <img src={url} alt="" className="block h-full w-full object-cover" onError={() => setFailed(previous => [...previous, url])} />
        : <Icon className={cn(sizes[size].glyph, builtin?.color ?? 'text-muted-foreground')} />;
    return <span className={cn('block shrink-0 overflow-hidden border bg-muted/40', sizes[size].box)}>
        <span className="grid h-full w-full place-items-center dark:hidden">{image(official?.icon ?? official?.icon_small ?? icon)}</span>
        <span className="hidden h-full w-full place-items-center dark:grid">{image(official?.icon_dark ?? official?.icon ?? official?.icon_small ?? icon)}</span>
    </span>;
}
