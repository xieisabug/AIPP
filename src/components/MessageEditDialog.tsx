import React, { useState, useEffect } from 'react';
import { X } from 'lucide-react';
import { Button } from './ui/button';
import { Textarea } from './ui/textarea';
import { useIsMobile } from '../hooks/use-mobile';
import { useIsMobilePlatform } from '../hooks/use-platform';

interface MessageEditDialogProps {
    isOpen: boolean;
    initialContent: string;
    messageType: string;
    onClose: () => void;
    onSave: (content: string) => void;
    onSaveAndRegenerate: (content: string) => void;
}

const MessageEditDialog: React.FC<MessageEditDialogProps> = ({
    isOpen,
    initialContent,
    messageType,
    onClose,
    onSave,
    onSaveAndRegenerate,
}) => {
    const [content, setContent] = useState(initialContent);
    const narrow = useIsMobile();
    const mobile = useIsMobilePlatform();
    const compact = narrow || mobile;

    useEffect(() => {
        if (isOpen) {
            setContent(initialContent);
        }
    }, [isOpen, initialContent]);

    if (!isOpen) return null;

    const handleSave = () => {
        onSave(content);
        onClose();
    };

    const handleSaveAndRegenerate = () => {
        onSaveAndRegenerate(content);
        onClose();
    };

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
            {/* 背景遮罩 */}
            <div
                className="absolute inset-0 bg-black/50 backdrop-blur-sm"
                onClick={onClose}
            />

            {/* 模态框内容 */}
            <div role="dialog" aria-modal="true" aria-label="编辑消息" data-mobile-panel
                onKeyDown={event => { if (event.key === 'Escape') { event.stopPropagation(); onClose(); } }}
                className={compact ? "fixed inset-0 bg-background flex flex-col safe-top safe-bottom" : "relative flex h-[70%] max-h-[88dvh] min-h-[28rem] w-[50%] max-w-[90%] min-w-[32rem] flex-col overflow-hidden rounded-lg border border-border bg-background shadow-lg"}>
                {/* 标题栏 */}
                <div className="flex shrink-0 items-center justify-between gap-2.5 border-b border-border px-4 py-2.5 sm:px-5">
                    <h2 className="truncate text-base font-semibold text-foreground">编辑消息</h2>
                    <button
                        title="关闭编辑"
                        type="button"
                        onClick={onClose}
                        aria-label="关闭编辑"
                        className="aipp-icon-button flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                    >
                        <X className="h-4 w-4" />
                    </button>
                </div>

                {/* 内容区域 */}
                <div className="flex min-h-0 flex-1 flex-col px-4 py-4 sm:px-5">
                    <Textarea
                        value={content}
                        onChange={(e) => setContent(e.target.value)}
                        className="min-h-0 w-full flex-1 resize-none"
                        placeholder="请输入消息内容..."
                        autoFocus
                    />
                </div>

                {/* 按钮区域 */}
                <div className="flex shrink-0 flex-wrap items-center justify-end gap-2 border-t border-border bg-background px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
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
                        variant={messageType === "user" ? "outline" : "default"}
                        onClick={handleSave}
                        className="min-h-11 px-4 sm:min-h-9"
                    >
                        修改
                    </Button>
                    {messageType === "user" && (
                        <Button
                            type="button"
                            onClick={handleSaveAndRegenerate}
                            className="min-h-11 px-5 sm:min-h-9"
                        >
                            修改并重新生成
                        </Button>
                    )}
                </div>
            </div>
        </div>
    );
};

export default MessageEditDialog;
