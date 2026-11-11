import type { ReactNode } from "react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import type { AssistantListItem } from "@/data/Assistant";

export type AssistantItemAction = "edit" | "copy" | "reset" | "delete" | "share";
/** canReset：管家联系人列表专用，重开该助手的私聊（旧私聊归档）。 */
export default function AssistantItemContextMenu({ assistant, children, onAction, canReset = false }: {
    assistant: AssistantListItem;
    children: ReactNode;
    onAction: (assistant: AssistantListItem, action: AssistantItemAction) => void;
    canReset?: boolean;
}) {
    return <ContextMenu><ContextMenuTrigger asChild><div className="w-full">{children}</div></ContextMenuTrigger>
        <ContextMenuContent>
            <ContextMenuItem onSelect={() => onAction(assistant, "edit")}>编辑</ContextMenuItem>
            <ContextMenuItem disabled={assistant.id === 1} onSelect={() => onAction(assistant, "copy")}>复制</ContextMenuItem>
            {canReset && <ContextMenuItem onSelect={() => onAction(assistant, "reset")}>重开新会话</ContextMenuItem>}
            <ContextMenuItem variant="destructive" disabled={assistant.id === 1} onSelect={() => onAction(assistant, "delete")}>删除</ContextMenuItem>
            <ContextMenuItem onSelect={() => onAction(assistant, "share")}>分享</ContextMenuItem>
        </ContextMenuContent>
    </ContextMenu>;
}
