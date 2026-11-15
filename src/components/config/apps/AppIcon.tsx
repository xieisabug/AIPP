import { useState } from 'react';
import { Code2, Globe, Notebook, Wrench } from 'lucide-react';
import { cn } from '@/utils/utils';

export interface OfficialIcon { icon?: string | null; icon_dark?: string | null; icon_small?: string | null }

const sizes = {
    sm: { box: 'h-9 w-9 rounded-lg', glyph: 'h-4 w-4' },
    md: { box: 'h-11 w-11 rounded-xl', glyph: 'h-5 w-5' },
    lg: { box: 'h-14 w-14 rounded-2xl', glyph: 'h-6 w-6' },
};

const TRUSTED_ICON_PREFIX = 'https://aipp-helper.xiejingyang.com/agent-plugin-icons/';

/** Helper-hosted official icon (light/dark) with a neutral glyph fallback. */
export default function AppIcon({ icon, builtin = false, official, size = 'md' }: { icon?: string | null; builtin?: boolean; official?: OfficialIcon; size?: keyof typeof sizes }) {
    const [failed, setFailed] = useState<string[]>([]);
    const Icon = builtin ? Wrench : icon === 'github' ? Code2 : icon === 'notebook' ? Notebook : Globe;
    const image = (url?: string | null) => url?.startsWith(TRUSTED_ICON_PREFIX) && !failed.includes(url)
        ? <img src={url} alt="" className="block h-full w-full object-cover" onError={() => setFailed(previous => [...previous, url])} />
        : <Icon className={cn(sizes[size].glyph, 'text-muted-foreground')} />;
    return <span className={cn('block shrink-0 overflow-hidden border bg-muted/40', sizes[size].box)}>
        <span className="grid h-full w-full place-items-center dark:hidden">{image(official?.icon ?? official?.icon_small ?? icon)}</span>
        <span className="hidden h-full w-full place-items-center dark:grid">{image(official?.icon_dark ?? official?.icon ?? official?.icon_small ?? icon)}</span>
    </span>;
}
