import React, { useState, useLayoutEffect, useRef } from "react";
import { Edit2, GitBranch, Copy, Check, RefreshCw, Send, MoreHorizontal, Info, Download } from "lucide-react";
import IconButton from "../IconButton";
import { MessageTokenTooltip, MessageTokenStatsDialog } from "../token-statistics";
import MessageExportDialog from "./MessageExportDialog";
import { useIsMobilePlatform } from "../../hooks/use-platform";
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";

interface MessageActionButtonsProps {
    messageId: number;
    messageType: string;
    isUserMessage: boolean;
    copyIconState: "copy" | "ok";
    onCopy: () => void;
    onEdit?: () => void;
    onRegenerate?: () => void;
    onFork?: () => void;
    onResendToFeishuDebug?: () => void;
    isResendToFeishuDebugPending?: boolean;
    messageContent?: string;
    pluginActions?: React.ReactNode;
}

const MessageActionButtons: React.FC<MessageActionButtonsProps> = ({
    messageId,
    messageType,
    isUserMessage,
    copyIconState,
    onCopy,
    onEdit,
    onRegenerate,
    onFork,
    onResendToFeishuDebug,
    isResendToFeishuDebugPending = false,
    messageContent,
    pluginActions,
}) => {
    const showEditRegenerate = messageType === "assistant" || messageType === "response" || messageType === "user";
    const [isTokenTooltipOpen, setIsTokenTooltipOpen] = useState(false);
    const [isExportDialogOpen, setIsExportDialogOpen] = useState(false);
    // 移动端操作常驻气泡底部，桌面保持 hover 行为。
    const mobileMode = useIsMobilePlatform();
    const [isMobileSheetOpen, setIsMobileSheetOpen] = useState(false);
    const [isMobileTokenOpen, setIsMobileTokenOpen] = useState(false);
    const [isMobileExportOpen, setIsMobileExportOpen] = useState(false);

    const barRef = useRef<HTMLDivElement>(null);
    const [availableWidth, setAvailableWidth] = useState<number | null>(null);
    useLayoutEffect(() => {
        const bar = barRef.current;
        if (!mobileMode || !bar) return;
        const measure = () => setAvailableWidth(bar.getBoundingClientRect().width);
        measure();
        const observer = new ResizeObserver(measure);
        observer.observe(bar);
        return () => observer.disconnect();
    }, [mobileMode]);

    if (mobileMode) {
        // 分叉、导出与插件固定进菜单；其他操作仅在超出行宽时收起。
        const runAndClose = (action?: () => void) => () => {
            setIsMobileSheetOpen(false);
            action?.();
        };
        const sheetItems: { key: string; label: string; icon: React.ReactNode; onSelect: () => void; disabled?: boolean }[] = [];
        if (showEditRegenerate && onEdit) {
            sheetItems.push({ key: "edit", label: "编辑消息", icon: <Edit2 className="h-4 w-4" />, onSelect: runAndClose(onEdit) });
        }
        if (showEditRegenerate && onRegenerate) {
            sheetItems.push({ key: "regenerate", label: "重新生成", icon: <RefreshCw className="h-4 w-4" />, onSelect: runAndClose(onRegenerate) });
        }
        if (messageType === "response" && onFork) {
            sheetItems.push({ key: "fork", label: "分叉会话", icon: <GitBranch className="h-4 w-4" />, onSelect: runAndClose(onFork) });
        }
        if ((messageType === "response" || messageType === "tool_result") && onResendToFeishuDebug) {
            sheetItems.push({
                key: "resend-feishu",
                label: "调试：重新发送到飞书",
                icon: <Send className="h-4 w-4" />,
                onSelect: runAndClose(onResendToFeishuDebug),
                disabled: isResendToFeishuDebugPending,
            });
        }
        if (messageType === "response") {
            sheetItems.push({ key: "info", label: "消息信息", icon: <Info className="h-4 w-4" />, onSelect: runAndClose(() => setIsMobileTokenOpen(true)) });
        }
        if (messageContent) {
            sheetItems.push({ key: "export", label: "导出消息", icon: <Download className="h-4 w-4" />, onSelect: runAndClose(() => setIsMobileExportOpen(true)) });
        }
        const overflowPlugins = React.Children.toArray(pluginActions);
        const menuOnlyItems = sheetItems.filter(item => item.key === "fork" || item.key === "export");
        const inlineItems = sheetItems.filter(item => item.key !== "fork" && item.key !== "export");
        const hasFixedMenu = menuOnlyItems.length > 0 || overflowPlugins.length > 0;
        const total = inlineItems.length;
        const slots = availableWidth === null ? total + 2 : Math.floor(availableWidth / 44);
        const needsMenu = hasFixedMenu || total + 1 > slots;
        const visibleCount = Math.max(0, slots - 1 - (needsMenu ? 1 : 0));
        const visibleItems = inlineItems.slice(0, visibleCount);
        const overflowItems = [...inlineItems.slice(visibleCount), ...menuOnlyItems];
        const hasMore = overflowItems.length > 0 || overflowPlugins.length > 0;

        return (
            <div
                ref={barRef}
                data-aipp-slot="message-action-bar"
                className="flex aipp-mobile-action-bar z-10 -ml-[15px] w-[calc(100%+15px)] flex-nowrap items-center"
            >
                <IconButton
                    icon={
                        copyIconState === "copy" ? <Copy size={16} className="text-icon" /> : <Check size={16} className="text-icon" />
                    }
                    onClick={onCopy}
                    title="复制消息"
                    className="!h-11 !w-11 shrink-0"
                />
                {visibleItems.map(item => (
                    <IconButton key={item.key} icon={item.icon} title={item.label}
                        onClick={item.onSelect} disabled={item.disabled} className="!h-11 !w-11 shrink-0" />
                ))}
                {hasMore && (
                    <IconButton
                        icon={<MoreHorizontal size={16} className="text-icon" />}
                        onClick={() => setIsMobileSheetOpen(true)}
                        className="!h-11 !w-11 shrink-0"
                        title="更多操作"
                        dataAippSlot="message-action-more"
                    />
                )}
                {hasMore && (
                    <Sheet open={isMobileSheetOpen} onOpenChange={setIsMobileSheetOpen}>
                        <SheetContent
                            side="bottom"
                            className="gap-0 rounded-t-2xl p-2 safe-bottom-bar"
                            aria-describedby={undefined}
                            hideCloseButton
                            data-aipp-slot="message-action-sheet"
                        >
                            <SheetTitle className="px-3 py-2 text-sm font-medium text-muted-foreground">消息操作</SheetTitle>
                            <div className="flex flex-col">
                                {overflowItems.map((item) => (
                                    <Button
                                        key={item.key}
                                        type="button"
                                        variant="ghost"
                                        className="min-h-11 w-full justify-start gap-3 px-3 text-sm font-normal"
                                        disabled={item.disabled}
                                        onClick={item.onSelect}
                                        data-aipp-slot={`message-action-sheet-${item.key}`}
                                    >
                                        {item.icon}
                                        {item.label}
                                    </Button>
                                ))}
                            </div>
                            {overflowPlugins.length > 0 ? (
                                <div className="flex flex-col gap-1 border-t border-border px-3 pt-2" data-aipp-slot="message-action-sheet-plugins">
                                    <span className="text-xs text-muted-foreground">插件操作</span>
                                    <div className="aipp-mobile-action-bar flex flex-wrap items-center">{overflowPlugins}</div>
                                </div>
                            ) : null}
                        </SheetContent>
                    </Sheet>
                )}
                {messageType === "response" && (
                    <MessageTokenStatsDialog
                        messageId={messageId}
                        open={isMobileTokenOpen}
                        onOpenChange={setIsMobileTokenOpen}
                    />
                )}
                {messageContent && (
                    <MessageExportDialog
                        messageContent={messageContent}
                        messageType={messageType}
                        open={isMobileExportOpen}
                        onOpenChange={setIsMobileExportOpen}
                        hideTrigger
                    />
                )}
            </div>
        );
    }

    return (
        <div
            data-aipp-slot="message-action-bar"
            className={`${isTokenTooltipOpen || isExportDialogOpen ? "flex" : "hidden group-hover:flex"} z-10 items-center absolute -bottom-9 py-3 px-4 box-border h-10 rounded-[21px] border border-border bg-background ${isUserMessage ? "right-0" : "left-0"}`}
        >
            {showEditRegenerate && onEdit && (
                <IconButton icon={<Edit2 size={16} className="text-icon" />} onClick={onEdit} title="编辑消息" />
            )}
            {showEditRegenerate && onRegenerate && (
                <IconButton icon={<RefreshCw size={16} className="text-icon" />} onClick={onRegenerate} title="重新生成" />
            )}
            {messageType === "response" && onFork && (
                <IconButton icon={<GitBranch size={16} className="text-icon" />} onClick={onFork} title="分叉会话" />
            )}
            {pluginActions}
            {(messageType === "response" || messageType === "tool_result") && onResendToFeishuDebug && (
                <IconButton
                    icon={
                        isResendToFeishuDebugPending ? (
                            <RefreshCw size={16} className="text-icon animate-spin" />
                        ) : (
                            <Send size={16} className="text-icon" />
                        )
                    }
                    onClick={onResendToFeishuDebug}
                    disabled={isResendToFeishuDebugPending}
                    title="调试：重新发送到飞书"
                    dataAippSlot="message-toolbar-resend-feishu"
                />
            )}
            <MessageTokenTooltip
                messageId={messageId}
                messageType={messageType}
                onOpenChange={setIsTokenTooltipOpen}
            />
            {messageContent && (
                <MessageExportDialog
                    messageContent={messageContent}
                    messageType={messageType}
                    onOpenChange={setIsExportDialogOpen}
                />
            )}
            <IconButton
                icon={
                    copyIconState === "copy" ? <Copy size={16} className="text-icon" /> : <Check size={16} className="text-icon" />
                }
                onClick={onCopy}
                title="复制消息"
            />
        </div>
    );
};

export default MessageActionButtons;
