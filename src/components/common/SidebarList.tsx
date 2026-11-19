import React, { memo } from 'react';
import { Search } from "lucide-react";
import { Input } from "../ui/input";

interface SidebarListProps {
    /** 列表名称：作为无障碍名称，不再显示为标题（设置菜单里已经显示了页面名）。 */
    title: string;
    /** 兼容旧调用，不再显示。 */
    description?: string;
    /** 兼容旧调用，不再显示。 */
    icon?: React.ReactNode;
    children: React.ReactNode;
    /** 列表顶部右侧的操作按钮（新建、导入、操作菜单等）。 */
    addButton?: React.ReactNode;
    searchValue?: string;
    onSearchChange?: (value: string) => void;
    searchPlaceholder?: string;
}

/**
 * 设置页左栏：顶部工具栏（搜索 + 页面自己的按钮），下面是一行一项的列表。
 * 外观只用主题变量；主题可通过 data-aipp-slot="config-list*" 定制。
 */
const SidebarList: React.FC<SidebarListProps> = ({
    title,
    children,
    addButton,
    searchValue,
    onSearchChange,
    searchPlaceholder = "搜索..."
}) => {
    const hasToolbar = onSearchChange !== undefined || Boolean(addButton);
    return (
        <nav
            aria-label={title}
            className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-border bg-card"
            data-aipp-slot="config-list"
        >
            {hasToolbar && (
                // 按钮多时（如定时任务的帮助/刷新/新建）换到搜索框下一行右对齐，搜索框保持可用宽度。
                <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border p-3" data-aipp-slot="config-list-toolbar">
                    {onSearchChange !== undefined ? (
                        <div className="relative min-w-32 flex-1 basis-32" data-aipp-slot="config-list-search">
                            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                            <Input
                                type="search"
                                aria-label={searchPlaceholder.replace(/\.+$|…$/, "")}
                                value={searchValue ?? ""}
                                onChange={(e) => onSearchChange(e.target.value)}
                                placeholder={searchPlaceholder}
                                className="h-9 pl-8 text-sm"
                            />
                        </div>
                    ) : (
                        <div className="flex-1" />
                    )}
                    {addButton && (
                        <div className="ml-auto flex shrink-0 items-center gap-2" data-aipp-slot="config-list-actions">
                            {addButton}
                        </div>
                    )}
                </div>
            )}
            {/*
              will-change-transform 让列表独立成层：Windows 上文字统一用灰度抗锯齿渲染。
              否则是否走 ClearType 取决于页面里别处有没有动画（如助手头像），同样的字在不同页面看起来粗细不同。
            */}
            <div className="min-h-0 flex-1 space-y-0.5 overflow-y-auto p-2 thin-scrollbar will-change-transform" data-aipp-slot="config-list-items">
                {children}
            </div>
        </nav>
    );
};

export default memo(SidebarList);
