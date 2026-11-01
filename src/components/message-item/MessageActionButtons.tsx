import React, { useState } from "react";
import { Edit2, GitBranch, Copy, Check, RefreshCw, Send } from "lucide-react";
import IconButton from "../IconButton";
import { MessageTokenTooltip } from "../token-statistics";
import MessageExportDialog from "./MessageExportDialog";
import { useIsMobilePlatform } from "../../hooks/use-platform";

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

    return (
        <div
            data-aipp-slot="message-action-bar"
            className={
                mobileMode
                    ? "flex aipp-mobile-action-bar z-10 -ml-[15px] w-fit max-w-[calc(100%+15px)] flex-nowrap overflow-x-auto items-center"
                    : `${isTokenTooltipOpen || isExportDialogOpen ? "flex" : "hidden group-hover:flex"} z-10 items-center absolute -bottom-9 py-3 px-4 box-border h-10 rounded-[21px] border border-border bg-background ${isUserMessage ? "right-0" : "left-0"}`
            }
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
