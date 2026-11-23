import React, { useState, useCallback, useMemo } from "react";
import IconButton from "../IconButton";
import { Edit2, Trash2 } from "lucide-react";
import { Conversation } from "../../data/Conversation";
import { useAntiLeakage } from "../../contexts/AntiLeakageContext";
import { maskTitle } from "../../utils/antiLeakage";
import ConfirmDialog from "../ConfirmDialog";
import useConversationManager from "../../hooks/useConversationManager";
import { ConversationStatsDialog } from "../token-statistics";
import ConversationExportDialog from "./ConversationExportDialog";
import DeepSeekBalanceBadge from "./DeepSeekBalanceBadge";
import { WindowControls, useFramelessTitleBar } from "../window/WindowTitleBar";

const ConversationTitle: React.FC<{
    conversation: Conversation | undefined;
    onEdit: () => void;
    onDelete: () => void;
    statsOpen?: boolean;
    onStatsOpenChange?: (open: boolean) => void;
    exportOpen?: boolean;
    onExportOpenChange?: (open: boolean) => void;
    allowRename?: boolean;
    allowDelete?: boolean;
    extraActions?: React.ReactNode;
    /** 标题栏贴着窗口右上角时，在末尾显示 Windows 窗口控制按钮（其他平台不渲染）。 */
    showWindowControls?: boolean;
}> = React.memo(({
    conversation,
    onEdit,
    onDelete,
    statsOpen,
    onStatsOpenChange,
    exportOpen,
    onExportOpenChange,
    allowRename = true,
    allowDelete = true,
    extraActions,
    showWindowControls = false,
}) => {
    const [deleteDialogIsOpen, setDeleteDialogIsOpen] = useState<boolean>(false);
    const { deleteConversation } = useConversationManager();
    const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();
    // 没有系统标题栏（macOS / Windows）时，标题栏空白处承担窗口拖动
    const framelessTitleBar = useFramelessTitleBar();

    // 判断是否需要脱敏
    const shouldMask = antiLeakageEnabled && !isRevealed;
    const displayName = useMemo(() => {
        if (!conversation?.name) return "";
        return shouldMask ? maskTitle(conversation.name) : conversation.name;
    }, [conversation?.name, shouldMask]);
    const displayAssistantName = useMemo(() => {
        if (!conversation?.assistant_name) return "";
        return shouldMask ? maskTitle(conversation.assistant_name) : conversation.assistant_name;
    }, [conversation?.assistant_name, shouldMask]);

    const openDeleteDialog = useCallback(() => {
        setDeleteDialogIsOpen(true);
    }, []);

    const closeDeleteDialog = useCallback(() => {
        setDeleteDialogIsOpen(false);
    }, []);

    const handleConfirmDelete = useCallback(() => {
        if (conversation) {
            deleteConversation(conversation.id.toString(), {
                onSuccess: async () => {
                    closeDeleteDialog();
                    onDelete(); // 通知父组件更新
                },
            });
        }
    }, [conversation, deleteConversation, onDelete, closeDeleteDialog]);

    return (
        <>
            <div className="flex justify-between flex-none h-[68px] items-center px-6 box-border border-b border-border bg-background z-20" data-tauri-drag-region={framelessTitleBar || undefined} data-aipp-slot="chat-conversation-title-bar">
                <div className="flex-1 overflow-hidden" data-tauri-drag-region={framelessTitleBar || undefined} data-aipp-slot="chat-conversation-title">
                    <div
                        className={`text-base font-semibold overflow-hidden text-ellipsis whitespace-nowrap text-icon ${allowRename ? "cursor-pointer" : ""}`}
                        onClick={allowRename ? onEdit : undefined}
                        data-aipp-slot="chat-conversation-title-text"
                    >
                        {displayName}
                    </div>
                    <div className="text-xs text-muted-foreground overflow-hidden text-ellipsis whitespace-nowrap mt-0.5" data-aipp-slot="chat-conversation-title-subtext">{displayAssistantName}</div>
                </div>
                <div className="flex items-center flex-none shrink-0 justify-end gap-2 pl-4" data-aipp-slot="chat-conversation-title-actions">
                    <DeepSeekBalanceBadge
                        conversationId={conversation?.id ?? null}
                        assistantId={conversation?.assistant_id ?? null}
                    />
                    {extraActions}
                    <ConversationStatsDialog conversationId={conversation?.id.toString() || ""} externalOpen={statsOpen} onExternalOpenChange={onStatsOpenChange} />
                    <ConversationExportDialog conversationId={conversation?.id.toString() || ""} externalOpen={exportOpen} onExternalOpenChange={onExportOpenChange} />
                    {allowRename ? (
                        <IconButton icon={<Edit2 size={16} className="text-icon" />} onClick={onEdit} ghost title="重命名" dataAippSlot="chat-conversation-title-edit" />
                    ) : null}
                    {allowDelete ? (
                        <IconButton icon={<Trash2 size={16} className="text-icon" />} onClick={openDeleteDialog} ghost title="删除对话" dataAippSlot="chat-conversation-title-delete" />
                    ) : null}
                    {/* Windows 窗口控制按钮：贴近窗口右上角（吃掉一半右内边距） */}
                    {showWindowControls && <WindowControls className="-mr-3 ml-2" />}
                </div>
            </div>

            <ConfirmDialog
                title="确认删除对话"
                confirmText={`确定要删除对话 "${conversation?.name}" 吗？此操作无法撤销。`}
                onConfirm={handleConfirmDelete}
                onCancel={closeDeleteDialog}
                isOpen={deleteDialogIsOpen && allowDelete}
            />
        </>
    );
});

export default ConversationTitle;
