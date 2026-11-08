import type { ReactNode } from "react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import type { AssistantListItem } from "@/data/Assistant";

export type AssistantItemAction = "edit" | "copy" | "clear" | "delete" | "share";
export default function AssistantItemContextMenu({ assistant, children, onAction, canClear = false }: {
    assistant: AssistantListItem;
    children: ReactNode;
    onAction: (assistant: AssistantListItem, action: AssistantItemAction) => void;
    canClear?: boolean;
}) {
    return <ContextMenu><ContextMenuTrigger asChild><div className="w-full">{children}</div></ContextMenuTrigger>
        <ContextMenuContent>
            <ContextMenuItem onSelect={() => onAction(assistant, "edit")}>编辑</ContextMenuItem>
            <ContextMenuItem disabled={assistant.id === 1} onSelect={() => onAction(assistant, "copy")}>复制</ContextMenuItem>
            {canClear && <ContextMenuItem onSelect={() => onAction(assistant, "clear")}>清空消息历史</ContextMenuItem>}
            <ContextMenuItem variant="destructive" disabled={assistant.id === 1} onSelect={() => onAction(assistant, "delete")}>删除</ContextMenuItem>
            <ContextMenuItem onSelect={() => onAction(assistant, "share")}>分享</ContextMenuItem>
        </ContextMenuContent>
    </ContextMenu>;
}
