import { cn } from '@/utils/utils';
import type { DisplayStatus, StatusTone } from './appStatus';

// Only the dot carries color; text stays in the neutral palette (errors excepted).
const dot: Record<StatusTone, string> = {
    ok: 'bg-emerald-500',
    idle: 'bg-muted-foreground/40',
    pending: 'bg-foreground/60 animate-pulse',
    warn: 'bg-amber-500',
    error: 'bg-destructive',
};

export default function AppStatusBadge({ status, className }: { status: DisplayStatus; className?: string }) {
    return <span data-tone={status.tone} className={cn('inline-flex items-center gap-1.5 whitespace-nowrap text-xs',
        status.tone === 'error' ? 'text-destructive' : status.tone === 'idle' ? 'text-muted-foreground' : 'text-foreground', className)}>
        <span aria-hidden className={cn('h-1.5 w-1.5 shrink-0 rounded-full', dot[status.tone])} />
        {status.label}
    </span>;
}
