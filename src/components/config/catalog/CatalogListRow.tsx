import type { ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';

/**
 * 「我的应用 / 我的插件」列表的一行。`control`（如启用开关）放在行按钮之外，避免按钮嵌套。
 */
export default function CatalogListRow({ icon, title, titleExtra, subtitle, status, detail, control, onClick, label }: {
    icon: ReactNode;
    title: string;
    titleExtra?: ReactNode;
    subtitle: string;
    status: ReactNode;
    detail?: ReactNode;
    control?: ReactNode;
    onClick: () => void;
    label?: string;
}) {
    return <div className="flex items-center transition-colors hover:bg-muted/40 focus-within:bg-muted/40">
        <button type="button" aria-label={label} className="flex min-w-0 flex-1 items-center gap-4 py-3 pl-4 pr-2 text-left focus-visible:outline-none" onClick={onClick}>
            {icon}
            <div className="min-w-0 flex-1">
                <p className="flex min-w-0 items-center gap-2 text-sm font-medium"><span className="truncate">{title}</span>{titleExtra}</p>
                <p className="truncate text-xs text-muted-foreground">{subtitle}</p>
            </div>
            <span className="w-28 shrink-0">{status}</span>
            {detail !== undefined && <span className="hidden w-28 shrink-0 truncate text-right text-xs text-muted-foreground sm:block">{detail}</span>}
        </button>
        {control && <div className="flex shrink-0 items-center pl-2">{control}</div>}
        <button type="button" tabIndex={-1} aria-hidden className="flex shrink-0 items-center py-3 pl-2 pr-4 text-muted-foreground" onClick={onClick}>
            <ChevronRight className="h-4 w-4" />
        </button>
    </div>;
}
