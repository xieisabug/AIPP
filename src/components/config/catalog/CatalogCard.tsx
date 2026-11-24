import type { ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';
import { cn } from '@/utils/utils';

/**
 * 目录卡片外壳：应用与工具、插件市场共用。
 * `media` 放在卡片顶部（插件的效果预览）；不传时就是应用目录的纯图标卡。
 */
export default function CatalogCard({ media, icon, name, badges, meta, trailing, description, descriptionLines = 2, status, action, onClick, compactHeader = false }: {
    media?: ReactNode;
    icon: ReactNode;
    name: string;
    badges?: ReactNode;
    meta?: string;
    trailing?: ReactNode;
    description: string;
    descriptionLines?: 1 | 2;
    status: ReactNode;
    action?: string;
    onClick: () => void;
    compactHeader?: boolean;
}) {
    return <button type="button" onClick={onClick}
        className="group flex h-full min-w-0 flex-col gap-3 rounded-xl border bg-card p-4 text-left transition-colors hover:border-foreground/20 hover:bg-muted/30 focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50">
        {media}
        <div className={cn('flex w-full gap-3', compactHeader ? 'items-center gap-2' : 'items-start')}>
            {icon}
            <div className={cn('min-w-0 flex-1', !compactHeader && 'pt-0.5')}>
                <div className="flex min-w-0 items-center gap-2">
                    <span className="truncate font-medium">{name}</span>
                    {badges}
                </div>
                {meta && <p className="mt-0.5 truncate text-xs text-muted-foreground">{meta}</p>}
            </div>
            {trailing}
        </div>
        <p className={cn('w-full text-sm leading-5 text-muted-foreground', descriptionLines === 2 ? 'line-clamp-2 min-h-10 flex-1' : 'line-clamp-1')}>{description}</p>
        <div className="mt-auto flex w-full items-center justify-between gap-2 border-t pt-3 text-xs">
            {status}
            {action && <span className="inline-flex items-center gap-0.5 text-muted-foreground transition-colors group-hover:text-foreground">{action}<ChevronRight className="h-3.5 w-3.5" /></span>}
        </div>
    </button>;
}

/** 卡片名字旁的小标签（「内置」「实验」）。 */
export function CatalogTag({ children }: { children: ReactNode }) {
    return <span className="shrink-0 rounded border px-1.5 py-px text-[10px] leading-4 text-muted-foreground">{children}</span>;
}
