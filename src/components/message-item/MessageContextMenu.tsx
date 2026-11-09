import React, { useState } from "react";
import { createPortal } from "react-dom";
import { Copy, Download, Edit2, GitBranch, Info, RefreshCw, Send, TextSelect } from "lucide-react";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { MessageTokenStatsDialog } from "../token-statistics";
import MessageExportDialog from "./MessageExportDialog";
import type { MessageActionButtonsProps } from "./MessageActionButtons";
import { MessageActionSurfaceContext } from "./messageActionSurface";

interface Props {
    /** 与悬浮操作条同一份参数：右键菜单与悬浮条的可用操作始终一致。null 时不接管右键。 */
    actions: MessageActionButtonsProps | null;
    children: React.ReactElement<React.HTMLAttributes<HTMLElement>>;
}

interface MenuState {
    x: number;
    y: number;
    selection: string;
}

/** 只取落在本条消息里的选中文本；选区在别处时不出现「复制选中内容」。 */
function selectedTextWithin(element: HTMLElement): string {
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed) return "";
    const inside = (node: Node | null) => !!node && element.contains(node);
    if (!inside(selection.anchorNode) && !inside(selection.focusNode)) return "";
    const text = selection.toString();
    return text.trim() ? text : "";
}

// 等菜单关闭、Radix 焦点/指针锁收尾之后再开对话框，避免 body 残留 pointer-events: none。
const afterMenuClosed = (action: () => void) => () => window.setTimeout(action, 0);

/**
 * 桌面端消息右键菜单：提供与悬浮操作条相同的功能。
 * 用受控 DropdownMenu + 光标处的不可见锚点实现，而不是 Radix ContextMenu：
 * 插件操作（含管家「引用回复」）是任意按钮节点，受控 open 才能在点击它们后关闭菜单。
 */
export default function MessageContextMenu({ actions, children }: Props) {
    const isMobile = useIsMobilePlatform();
    const [menu, setMenu] = useState<MenuState | null>(null);
    const [tokenOpen, setTokenOpen] = useState(false);
    const [exportOpen, setExportOpen] = useState(false);

    // 移动端长按留给系统文本选择；移动端操作已常驻在气泡底部。
    if (isMobile || !actions) return children;

    const {
        messageId, messageType, onCopy, onEdit, onRegenerate, onFork,
        onResendToFeishuDebug, isResendToFeishuDebugPending = false, messageContent, pluginActions,
    } = actions;
    const showEditRegenerate = messageType === "assistant" || messageType === "response" || messageType === "user";
    const hasPluginActions = React.Children.toArray(pluginActions).length > 0;

    const handleContextMenu = (event: React.MouseEvent<HTMLElement>) => {
        children.props.onContextMenu?.(event);
        // 内层自带右键菜单的元素（如图片）已经处理了这次右键。
        if (event.defaultPrevented) return;
        event.preventDefault();
        setMenu({ x: event.clientX, y: event.clientY, selection: selectedTextWithin(event.currentTarget) });
    };

    return (
        <>
            {React.cloneElement(children, { onContextMenu: handleContextMenu })}
            {menu && (
                <DropdownMenu open onOpenChange={(open) => { if (!open) setMenu(null); }}>
                    {createPortal(
                        <DropdownMenuTrigger asChild>
                            <span aria-hidden="true" data-aipp-slot="message-context-menu-anchor"
                                style={{ position: "fixed", left: menu.x, top: menu.y, width: 0, height: 0, pointerEvents: "none" }} />
                        </DropdownMenuTrigger>,
                        document.body,
                    )}
                    <DropdownMenuContent align="start" side="bottom" sideOffset={2} className="min-w-40"
                        data-aipp-slot="message-context-menu" onCloseAutoFocus={(event) => event.preventDefault()}>
                        {menu.selection && (
                            <DropdownMenuItem onSelect={() => void navigator.clipboard.writeText(menu.selection)}>
                                <TextSelect className="h-4 w-4" />复制选中内容
                            </DropdownMenuItem>
                        )}
                        <DropdownMenuItem onSelect={onCopy}><Copy className="h-4 w-4" />复制消息</DropdownMenuItem>
                        {showEditRegenerate && onEdit && (
                            <DropdownMenuItem onSelect={onEdit}><Edit2 className="h-4 w-4" />编辑消息</DropdownMenuItem>
                        )}
                        {showEditRegenerate && onRegenerate && (
                            <DropdownMenuItem onSelect={onRegenerate}><RefreshCw className="h-4 w-4" />重新生成</DropdownMenuItem>
                        )}
                        {messageType === "response" && onFork && (
                            <DropdownMenuItem onSelect={onFork}><GitBranch className="h-4 w-4" />分叉会话</DropdownMenuItem>
                        )}
                        {(messageType === "response" || messageType === "tool_result") && onResendToFeishuDebug && (
                            <DropdownMenuItem disabled={isResendToFeishuDebugPending} onSelect={onResendToFeishuDebug}>
                                <Send className="h-4 w-4" />调试：重新发送到飞书
                            </DropdownMenuItem>
                        )}
                        {(messageType === "response" || messageContent) && <DropdownMenuSeparator />}
                        {messageType === "response" && (
                            <DropdownMenuItem onSelect={afterMenuClosed(() => setTokenOpen(true))}>
                                <Info className="h-4 w-4" />消息信息
                            </DropdownMenuItem>
                        )}
                        {messageContent && (
                            <DropdownMenuItem onSelect={afterMenuClosed(() => setExportOpen(true))}>
                                <Download className="h-4 w-4" />导出消息
                            </DropdownMenuItem>
                        )}
                        {hasPluginActions && (
                            <>
                                <DropdownMenuSeparator />
                                {/* 插件按钮原样复用，点击后收起菜单；内置操作（如「引用回复」）在菜单里渲染成整行菜单项。 */}
                                <div className="flex flex-wrap items-center gap-0.5"
                                    data-aipp-slot="message-context-menu-plugins" onClick={() => setMenu(null)}>
                                    <MessageActionSurfaceContext.Provider value="menu">
                                        {pluginActions}
                                    </MessageActionSurfaceContext.Provider>
                                </div>
                            </>
                        )}
                    </DropdownMenuContent>
                </DropdownMenu>
            )}
            {tokenOpen && <MessageTokenStatsDialog messageId={messageId} open onOpenChange={setTokenOpen} />}
            {exportOpen && messageContent && (
                <MessageExportDialog messageContent={messageContent} messageType={messageType}
                    open onOpenChange={setExportOpen} hideTrigger />
            )}
        </>
    );
}
