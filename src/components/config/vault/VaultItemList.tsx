import { Plus, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { VaultItem } from "@/data/Vault";
import { cn } from "@/utils/utils";

interface VaultItemListProps {
    /** 已按搜索词过滤后的条目。 */
    items: VaultItem[];
    selectedId: string | undefined;
    onSelect: (id: string) => void;
    onCreate: () => void;
    search: string;
    /** 不传则不显示搜索框（条目少时没有必要）。 */
    onSearchChange?: (value: string) => void;
}

/**
 * 保险箱面板左栏：新建按钮、（条目多时）搜索、每个条目一行。
 * 只用主题变量和 data-aipp-slot，主题可以整体替换外观；选中样式沿用设置菜单的 primary/10 着色。
 */
export default function VaultItemList({ items, selectedId, onSelect, onCreate, search, onSearchChange }: VaultItemListProps) {
    return (
        <nav
            aria-label="保险箱条目"
            className="flex w-52 shrink-0 flex-col border-r border-border bg-muted/30 lg:w-60 xl:w-64"
            data-aipp-slot="vault-list"
        >
            <div className="space-y-2 p-3">
                <Button variant="outline" size="sm" className="h-9 w-full justify-start gap-2 bg-background" onClick={onCreate} data-aipp-slot="vault-list-new">
                    <Plus className="h-4 w-4" />
                    新建条目
                </Button>
                {onSearchChange && (
                    <div className="relative" data-aipp-slot="vault-list-search">
                        <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                        <Input
                            type="search"
                            aria-label="搜索条目"
                            placeholder="搜索条目..."
                            className="h-8 bg-background pl-8 text-sm"
                            value={search}
                            onChange={(event) => onSearchChange(event.target.value)}
                        />
                    </div>
                )}
            </div>
            <ul className="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-3 pb-3 thin-scrollbar">
                {items.map((item) => {
                    const selected = item.id === selectedId;
                    return (
                        <li key={item.id}>
                            <button
                                type="button"
                                aria-current={selected ? "true" : undefined}
                                className={cn(
                                    "flex h-9 w-full items-center gap-2 rounded-md px-3 text-left text-sm transition-colors",
                                    selected
                                        ? "bg-primary/10 font-medium text-primary"
                                        : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
                                )}
                                onClick={() => onSelect(item.id)}
                                data-aipp-slot="vault-list-item"
                            >
                                <span className="min-w-0 flex-1 truncate">{item.name}</span>
                                {item.status === "locked" && <span className="shrink-0 text-xs text-muted-foreground">待解锁</span>}
                            </button>
                        </li>
                    );
                })}
                {items.length === 0 && <li className="px-3 py-6 text-center text-xs text-muted-foreground">没有匹配的条目</li>}
            </ul>
        </nav>
    );
}
