import React, { memo } from 'react';
import { cn } from "@/utils/utils";

interface ListItemButtonProps {
    isSelected: boolean;
    onClick: () => void;
    children: React.ReactNode;
    className?: string;
}

/**
 * 设置页左栏的一项：默认一行高，无边框，字重统一为 font-medium（页面内容不必再设字重）；
 * 选中样式与设置菜单一致（primary/10 着色），随主题变化。
 */
const ListItemButton: React.FC<ListItemButtonProps> = ({
    isSelected,
    onClick,
    children,
    className = ""
}) => {
    return (
        <button
            type="button"
            aria-current={isSelected ? "true" : undefined}
            onClick={onClick}
            className={cn(
                "flex min-h-9 w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-sm font-medium transition-colors",
                "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
                isSelected
                    ? "bg-primary/10 text-primary"
                    : "text-foreground hover:bg-muted",
                className,
            )}
            data-aipp-slot="config-list-item"
        >
            {children}
        </button>
    );
};

export default memo(ListItemButton);
