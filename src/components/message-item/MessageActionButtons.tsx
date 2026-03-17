import React, { useState } from "react";
import { Edit2, GitBranch, Copy, Check, RefreshCw, Send } from "lucide-react";
import IconButton from "../IconButton";
import { MessageTokenTooltip } from "../token-statistics";
import MessageExportDialog from "./MessageExportDialog";

interface MessageActionButtonsProps {
    messageType: string;
    isUserMessage: boolean;
    copyIconState: "copy" | "ok";
    onCopy: () => void;
    onEdit?: () => void;
    onRegenerate?: () => void;
    onFork?: () => void;
    onResendToFeishuDebug?: () => void;
    isResendToFeishuDebugPending?: boolean;
    tokenCount: number;
    inputTokenCount: number;
    outputTokenCount: number;
    ttftMs?: number | null;
    tps?: number | null;
    startTime?: Date | null;
    finishTime?: Date | null;
    messageContent?: string;
}

const MessageActionButtons: React.FC<MessageActionButtonsProps> = ({
    messageType,
    isUserMessage,
    copyIconState,
    onCopy,
    onEdit,
    onRegenerate,
    onFork,
    onResendToFeishuDebug,
    isResendToFeishuDebugPending = false,
    tokenCount,
    inputTokenCount,
    outputTokenCount,
    ttftMs,
    tps,
    startTime,
    finishTime,
    messageContent,
}) => {
    const showEditRegenerate = messageType === "assistant" || messageType === "response" || messageType === "user";
    const [isTokenTooltipOpen, setIsTokenTooltipOpen] = useState(false);
    const [isExportDialogOpen, setIsExportDialogOpen] = useState(false);

    return (
        <div
            className={`${isTokenTooltipOpen || isExportDialogOpen ? "flex" : "hidden group-hover:flex"} z-10 items-center absolute -bottom-9 py-3 px-4 box-border h-10 rounded-[21px] border border-border bg-background ${isUserMessage ? "right-0" : "left-0"}`}
        >
            {showEditRegenerate && onEdit && (
                <IconButton icon={<Edit2 size={16} className="text-icon" />} onClick={onEdit} />
            )}
            {showEditRegenerate && onRegenerate && (
                <IconButton icon={<RefreshCw size={16} className="text-icon" />} onClick={onRegenerate} />
            )}
            {messageType === "response" && onFork && (
                <IconButton icon={<GitBranch size={16} className="text-icon" />} onClick={onFork} />
            )}
            {messageType === "response" && onResendToFeishuDebug && (
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
                tokenCount={tokenCount}
                inputTokenCount={inputTokenCount}
                outputTokenCount={outputTokenCount}
                messageType={messageType}
                ttftMs={ttftMs}
                tps={tps}
                startTime={startTime}
                finishTime={finishTime}
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
            />
        </div>
    );
};

export default MessageActionButtons;
