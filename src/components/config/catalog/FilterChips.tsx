import { Button } from '@/components/ui/button';
import { cn } from '@/utils/utils';

/** 目录页的分类筛选（应用与工具「我的应用」、插件市场与「我的插件」共用）。 */
export default function FilterChips<T extends string>({ items, value, onChange, label }: {
    items: { value: T; label: string; count?: number }[];
    value: T;
    onChange: (value: T) => void;
    label: string;
}) {
    return <div role="toolbar" aria-label={label} className="-mt-2 flex gap-1.5 overflow-x-auto pb-1">
        {items.map(item => <Button key={item.value} size="sm" variant={value === item.value ? 'secondary' : 'ghost'} aria-pressed={value === item.value}
            className={cn('h-7 shrink-0 rounded-full px-3 text-xs', value !== item.value && 'text-muted-foreground')} onClick={() => onChange(item.value)}>
            {item.label}{item.count !== undefined && <span className="ml-1 text-muted-foreground">{item.count}</span>}
        </Button>)}
    </div>;
}
