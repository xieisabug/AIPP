import React, { memo, useCallback, useId, useRef } from 'react';
import { Check, ChevronsUpDown } from "lucide-react";
import { cn } from "@/utils/utils";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "../ui/dropdown-menu";

// 分组标题条：铺满列表宽度（-mx-2 抵消 SidebarList 的 p-2），文字与列表项文字左对齐（p-2 + px-3 = px-5）。
const HEADER_BAR = "flex h-8 w-full items-center gap-2 border-b border-border bg-muted/80 px-5 text-left text-xs font-semibold text-foreground";

export interface SidebarListGroup {
    /** 分组唯一键。 */
    key: string;
    /** 分组名称，显示在分组标题上。 */
    label: string;
    /** 分组内条目数（搜索时为匹配数）。 */
    count: number;
    /** 分组内的列表项（通常是若干 ListItemButton）。 */
    children: React.ReactNode;
}

interface SidebarListGroupsProps {
    groups: SidebarListGroup[];
}

/**
 * SidebarList 内的分组列表：分组标题吸顶（滚到哪一组，顶部就显示哪一组），
 * 有两个以上分组时，点标题弹出全部分组，选中即跳到该组开头。
 * 主题可通过 data-aipp-slot="config-list-group*" 定制。
 */
const SidebarListGroups: React.FC<SidebarListGroupsProps> = ({ groups }) => {
    const idPrefix = useId();
    const sectionRefs = useRef(new Map<string, HTMLElement>());
    const switchable = groups.length > 1;

    const triggerOf = (key: string) => sectionRefs.current.get(key)?.querySelector<HTMLElement>('[data-aipp-group-trigger]');

    const jumpTo = useCallback((key: string) => {
        const section = sectionRefs.current.get(key);
        const scroller = section?.closest<HTMLElement>('[data-aipp-slot="config-list-items"]');
        if (!section || !scroller) return;
        // 只滚动列表自身；scrollIntoView 会连带滚动外层容器。
        const top = scroller.scrollTop + section.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
        scroller.scrollTo({ top, behavior: 'smooth' });
        // 键盘用户：焦点移到目标分组标题，不触发额外滚动。
        triggerOf(key)?.focus({ preventScroll: true });
    }, []);

    return (
        <>
            {groups.map((group, index) => {
                const headingId = `${idPrefix}-group-${index}`;
                const heading = (
                    <>
                        <span id={headingId} className="min-w-0 truncate">{group.label}</span>
                        <span className="shrink-0 tabular-nums font-normal text-muted-foreground">{group.count}</span>
                    </>
                );
                return (
                    <section
                        key={group.key}
                        ref={(el) => { if (el) sectionRefs.current.set(group.key, el); else sectionRefs.current.delete(group.key); }}
                        aria-labelledby={headingId}
                        className="mt-2 first:-mt-2"
                        data-aipp-slot="config-list-group"
                    >
                        {/*
                          吸顶位置按列表内边距计算：-top-2 让标题贴住列表可视区顶边，否则上方会露出一条滚过的条目。
                          底色先垫 bg-card，主题把 muted 设成半透明时条目也不会透出来。
                        */}
                        <div className="sticky -top-2 z-10 -mx-2 bg-card" data-aipp-slot="config-list-group-header">
                            {switchable ? (
                                <DropdownMenu modal={false}>
                                    <DropdownMenuTrigger asChild>
                                        <button
                                            type="button"
                                            data-aipp-group-trigger=""
                                            aria-label={`${group.label}，${group.count} 项，切换分组`}
                                            className={cn(HEADER_BAR, "cursor-pointer transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50 data-[state=open]:bg-muted")}
                                        >
                                            {heading}
                                            <ChevronsUpDown className="ml-auto h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                                        </button>
                                    </DropdownMenuTrigger>
                                    <DropdownMenuContent
                                        align="start"
                                        className="w-[var(--radix-dropdown-menu-trigger-width)]"
                                        // 默认会把焦点还给原标题并滚动到它，抵消刚才的跳转；改为不滚动地处理焦点。
                                        onCloseAutoFocus={(event) => {
                                            event.preventDefault();
                                            const active = document.activeElement;
                                            if (!active || active === document.body) triggerOf(group.key)?.focus({ preventScroll: true });
                                        }}
                                    >
                                        {groups.map((target) => (
                                            <DropdownMenuItem key={target.key} onSelect={() => jumpTo(target.key)}>
                                                <Check className={cn("h-3.5 w-3.5", target.key !== group.key && "invisible")} />
                                                <span className="min-w-0 flex-1 truncate">{target.label}</span>
                                                <span className="shrink-0 tabular-nums text-xs text-muted-foreground">{target.count}</span>
                                            </DropdownMenuItem>
                                        ))}
                                    </DropdownMenuContent>
                                </DropdownMenu>
                            ) : (
                                <div className={HEADER_BAR}>
                                    {heading}
                                </div>
                            )}
                        </div>
                        <div className="space-y-0.5 pt-1">{group.children}</div>
                    </section>
                );
            })}
        </>
    );
};

export default memo(SidebarListGroups);
