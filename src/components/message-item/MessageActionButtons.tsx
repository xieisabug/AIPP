import React, { useEffect, useRef, useState } from "react";
import { Edit2, GitBranch, Copy, Check, RefreshCw, Send } from "lucide-react";
import IconButton from "../IconButton";
import { MessageTokenTooltip } from "../token-statistics";
import MessageExportDialog from "./MessageExportDialog";
import { useIsMobilePlatform } from "../../hooks/use-platform";

/** 点击这些元素时不切换移动端操作栏 */
const MOBILE_TOGGLE_IGNORE_SELECTOR =
    "a,button,input,textarea,select,summary,pre,[role=button],[data-no-action-toggle],[data-aipp-slot=message-action-bar]";

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
    // 移动平台没有 hover（Tailwind v4 的 group-hover 包在 @media (hover:hover) 里）：
    // 点击所在气泡切换操作栏；桌面保持原 hover 行为不变。
    const mobileMode = useIsMobilePlatform();
    const [mobileOpen, setMobileOpen] = useState(false);
    const barRef = useRef<HTMLDivElement | null>(null);

    useEffect(() => {
        if (!mobileMode) return;
        const bubble = barRef.current?.parentElement;
        if (!bubble) return;
        const handleClick = (event: MouseEvent) => {
            const target = event.target as HTMLElement | null;
            if (target?.closest(MOBILE_TOGGLE_IGNORE_SELECTOR)) return;
            if (window.getSelection()?.toString()) return;
            setMobileOpen((open) => !open);
        };
        bubble.addEventListener("click", handleClick);
        return () => bubble.removeEventListener("click", handleClick);
    }, [mobileMode]);

    return (
        <div
            ref={barRef}
            data-aipp-slot="message-action-bar"
            className={
                mobileMode
                    ? `${mobileOpen || isTokenTooltipOpen || isExportDialogOpen ? "flex" : "hidden"} aipp-mobile-action-bar z-10 mt-3 w-fit max-w-full flex-wrap items-center gap-1 px-1 box-border rounded-[26px] border border-border bg-background ${isUserMessage ? "ml-auto" : ""}`
                    : `${isTokenTooltipOpen || isExportDialogOpen ? "flex" : "hidden group-hover:flex"} z-10 items-center absolute -bottom-9 py-3 px-4 box-border h-10 rounded-[21px] border border-border bg-background ${isUserMessage ? "right-0" : "left-0"}`
            }
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
            />
        </div>
    );
};

export default MessageActionButtons;
