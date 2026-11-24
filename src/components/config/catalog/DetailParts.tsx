import type { ReactNode } from 'react';
import { cn } from '@/utils/utils';

/** 详情抽屉里的提示条与信息列表；应用详情与插件详情共用。 */
export type NoticeTone = 'neutral' | 'warn' | 'error';

export function Notice({ tone = 'neutral', icon, title, children, actions }: { tone?: NoticeTone; icon: ReactNode; title: string; children?: ReactNode; actions?: ReactNode }) {
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

export function InfoList({ title = '信息', rows }: { title?: string; rows: [string, ReactNode][] }) {
    return <section className="space-y-3">
        <h4 className="text-sm font-medium">{title}</h4>
        <dl className="grid grid-cols-[6.5rem_1fr] gap-x-4 gap-y-3 rounded-lg border px-4 py-3 text-sm">
            {rows.map(([term, value]) => <div key={term} className="contents"><dt className="text-muted-foreground">{term}</dt><dd className="min-w-0 break-words">{value}</dd></div>)}
        </dl>
    </section>;
}
