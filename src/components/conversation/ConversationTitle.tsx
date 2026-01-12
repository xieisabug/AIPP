import React, { useState, useCallback, useMemo } from "react";
import IconButton from "../IconButton";
import Edit from "../../assets/edit.svg?react";
import Delete from "../../assets/delete.svg?react";
import { Conversation } from "../../data/Conversation";
import { useAntiLeakage } from "../../contexts/AntiLeakageContext";
import { maskTitle } from "../../utils/antiLeakage";
import ConfirmDialog from "../ConfirmDialog";
import useConversationManager from "../../hooks/useConversationManager";
import { ConversationStatsDialog } from "../token-statistics";
import ConversationExportDialog from "./ConversationExportDialog";

const ConversationTitle: React.FC<{
    conversation: Conversation | undefined;
    onEdit: () => void;
    onDelete: () => void;
}> = React.memo(({ conversation, onEdit, onDelete }) => {
    const [deleteDialogIsOpen, setDeleteDialogIsOpen] = useState<boolean>(false);
    const { deleteConversation } = useConversationManager();
    const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();

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
            <div className="flex justify-between flex-none h-[68px] items-center px-6 box-border border-b border-border bg-background rounded-t-xl z-20">
                <div className="flex-1 overflow-hidden">
                    <div className="text-base font-semibold overflow-hidden text-ellipsis whitespace-nowrap text-foreground cursor-pointer" onClick={onEdit}>{displayName}</div>
                    <div className="text-xs text-muted-foreground overflow-hidden text-ellipsis whitespace-nowrap mt-0.5">{displayAssistantName}</div>
                </div>
                <div className="flex items-center flex-none w-64 justify-end gap-2">
                    <ConversationStatsDialog conversationId={conversation?.id.toString() || ""} />
                    <ConversationExportDialog conversationId={conversation?.id.toString() || ""} />
                    <IconButton icon={<Edit className="fill-foreground" />} onClick={onEdit} border />
                    <IconButton icon={<Delete className="fill-foreground" />} onClick={openDeleteDialog} border />
                </div>
            </div>

            <ConfirmDialog
                title="确认删除对话"
                confirmText={`确定要删除对话 "${conversation?.name}" 吗？此操作无法撤销。`}
                onConfirm={handleConfirmDelete}
                onCancel={closeDeleteDialog}
                isOpen={deleteDialogIsOpen}
            />
        </>
    );
});

export default ConversationTitle;