import React, { useState } from "react";
import { Edit2, GitBranch } from "lucide-react";
import IconButton from "../IconButton";
import Copy from "../../assets/copy.svg?react";
import Ok from "../../assets/ok.svg?react";
import Refresh from "../../assets/refresh.svg?react";
import { MessageTokenTooltip } from "../token-statistics";

interface MessageActionButtonsProps {
    messageType: string;
    isUserMessage: boolean;
    copyIconState: "copy" | "ok";
    onCopy: () => void;
    onEdit?: () => void;
    onRegenerate?: () => void;
    onFork?: () => void;
    tokenCount: number;
    inputTokenCount: number;
    outputTokenCount: number;
    ttftMs?: number | null;
    tps?: number | null;
}

const MessageActionButtons: React.FC<MessageActionButtonsProps> = ({
    messageType,
    isUserMessage,
    copyIconState,
    onCopy,
    onEdit,
    onRegenerate,
    onFork,
    tokenCount,
    inputTokenCount,
    outputTokenCount,
    ttftMs,
    tps,
}) => {
    const showEditRegenerate = messageType === "assistant" || messageType === "response" || messageType === "user";
    const [isTokenTooltipOpen, setIsTokenTooltipOpen] = useState(false);

    return (
        <div
            className={`${isTokenTooltipOpen ? "flex" : "hidden group-hover:flex"} z-10 items-center absolute -bottom-9 py-3 px-4 box-border h-10 rounded-[21px] border border-border bg-background ${isUserMessage ? "right-0" : "left-0"}`}
        >
            {showEditRegenerate && onEdit && (
                <IconButton icon={<Edit2 size={16} className="stroke-foreground" />} onClick={onEdit} />
            )}
            {showEditRegenerate && onRegenerate && (
                <IconButton icon={<Refresh className="fill-foreground" />} onClick={onRegenerate} />
            )}
            {messageType === "response" && onFork && (
                <IconButton icon={<GitBranch size={16} className="stroke-foreground" />} onClick={onFork} />
            )}
            <MessageTokenTooltip
                tokenCount={tokenCount}
                inputTokenCount={inputTokenCount}
                outputTokenCount={outputTokenCount}
                messageType={messageType}
                ttftMs={ttftMs}
                tps={tps}
                onOpenChange={setIsTokenTooltipOpen}
            />
            <IconButton
                icon={
                    copyIconState === "copy" ? <Copy className="fill-foreground" /> : <Ok className="fill-foreground" />
                }
                onClick={onCopy}
            />
        </div>
    );
};

export default MessageActionButtons;
