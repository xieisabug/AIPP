import React, { useState, useEffect, useCallback } from 'react';
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Sparkles } from 'lucide-react';
import { Button } from './ui/button';
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from './ui/dialog';
import { Input } from './ui/input';
import { Label } from './ui/label';
import { getErrorMessage, isInsufficientMessagesError } from "../utils/error";

interface ConversationTitleEditDialogProps {
    isOpen: boolean;
    conversationId: number;
    initialTitle: string;
    onClose: () => void;
    onSave?: (title: string) => void; // 可选的保存回调
}

const isEnterSubmit = (event: React.KeyboardEvent) =>
    event.key === "Enter" && !event.nativeEvent.isComposing && event.keyCode !== 229;

const ConversationTitleEditDialog: React.FC<ConversationTitleEditDialogProps> = ({
    isOpen,
    conversationId,
    initialTitle,
    onClose,
    onSave,
}) => {
    const [title, setTitle] = useState<string>("");
    const [isRegeneratingTitle, setIsRegeneratingTitle] = useState<boolean>(false);

    // 当对话框打开时，同步初始标题
    useEffect(() => {
        if (isOpen) {
            setTitle(initialTitle || "");
        }
    }, [isOpen, initialTitle]);

    // 监听标题变化事件，实时更新输入框内容
    useEffect(() => {
        if (!isOpen || !conversationId) return;

        const unsubscribe = listen("title_change", (event) => {
            const [eventConversationId, newTitle] = event.payload as [number, string];

            // 只更新当前正在编辑的对话的标题
            if (eventConversationId === conversationId) {
                setTitle(newTitle);
            }
        });

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [isOpen, conversationId]);

    // 重新生成标题处理
    const handleRegenerateTitle = useCallback(async () => {
        if (!conversationId || isRegeneratingTitle) return;

        setIsRegeneratingTitle(true);

        try {
            const id = Number(conversationId);
            if (!Number.isFinite(id)) {
                throw new Error("无效的对话 ID");
            }
            await invoke("regenerate_conversation_title", {
                conversation_id: id,
                conversationId: id, // 兼容旧参数名
            });
            toast.success("标题已重新生成");
        } catch (error) {
            console.error("重新生成标题失败:", error);

            // 处理特定的错误类型，提供更友好的提示
            if (isInsufficientMessagesError(error)) {
                toast.error("对话内容不足，需要至少包含一条用户消息才能生成标题");
            } else {
                toast.error("重新生成标题失败: " + getErrorMessage(error));
            }
        } finally {
            setIsRegeneratingTitle(false);
        }
    }, [conversationId, isRegeneratingTitle]);

    // 提交表单处理
    const handleSubmit = useCallback(() => {
        const id = Number(conversationId);
        if (!Number.isFinite(id)) {
            toast.error("无效的对话 ID");
            return;
        }
        const name = title;
        invoke("update_conversation", {
            conversation_id: id,
            conversationId: id, // 兼容旧参数名
            name,
        }).then(() => {
            if (onSave) {
                onSave(name);
            }
            onClose();
            toast.success("标题已更新");
        }).catch((error) => {
            toast.error("更新标题失败: " + getErrorMessage(error));
        });
    }, [conversationId, title, onSave, onClose]);

    return (
        <Dialog open={isOpen} onOpenChange={(open) => { if (!open) onClose(); }}>
            <CompactDialogContent className="sm:max-w-md">
                <CompactDialogHeader
                    title="修改对话标题"
                    description="编辑当前对话标题或使用 AI 重新生成"
                />

                <CompactDialogBody className="space-y-4">
                    <div className="space-y-1.5">
                        <div className="flex items-center justify-between gap-2">
                            <Label htmlFor="conversation-title-input">标题</Label>
                            <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                onClick={handleRegenerateTitle}
                                disabled={isRegeneratingTitle}
                                className="h-7 gap-1 px-2 text-xs text-muted-foreground hover:text-foreground"
                                title="重新生成标题"
                            >
                                <Sparkles className={`h-3.5 w-3.5 ${isRegeneratingTitle ? 'animate-pulse' : ''}`} />
                                {isRegeneratingTitle ? '生成中…' : 'AI 生成'}
                            </Button>
                        </div>
                        <Input
                            id="conversation-title-input"
                            type="text"
                            autoComplete="off"
                            enterKeyHint="done"
                            name="name"
                            value={title}
                            onChange={(e) => setTitle(e.target.value)}
                            onKeyDown={(event) => {
                                if (!isEnterSubmit(event)) return;
                                event.preventDefault();
                                handleSubmit();
                            }}
                            placeholder="请输入对话标题"
                            autoFocus
                        />
                    </div>
                </CompactDialogBody>

                <CompactDialogFooter>
                    <Button
                        type="button"
                        variant="outline"
                        onClick={onClose}
                        className="min-h-11 px-4 sm:min-h-9"
                    >
                        取消
                    </Button>
                    <Button
                        type="button"
                        onClick={handleSubmit}
                        className="min-h-11 px-5 sm:min-h-9"
                    >
                        确认
                    </Button>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
};

export default ConversationTitleEditDialog;