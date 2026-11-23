import React, { useState, useCallback, useRef } from 'react';
import { Copy, Check, Share2 } from 'lucide-react';
import { Button } from './ui/button';
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from './ui/dialog';
import { Label } from './ui/label';
import { Textarea } from './ui/textarea';
import { toast } from 'sonner';

interface ShareDialogProps {
    title: string;
    shareCode: string;
    isOpen: boolean;
    onClose: () => void;
}

const ShareDialog: React.FC<ShareDialogProps> = ({
    title,
    shareCode,
    isOpen,
    onClose,
}) => {
    const [copied, setCopied] = useState(false);
    const textareaRef = useRef<HTMLTextAreaElement>(null);

    const handleCopy = useCallback(async () => {
        try {
            await navigator.clipboard.writeText(shareCode);
            setCopied(true);
            toast.success('分享码已复制到剪贴板');
            setTimeout(() => setCopied(false), 2000);
        } catch (error) {
            if (textareaRef.current) {
                textareaRef.current.select();
                textareaRef.current.setSelectionRange(0, 99999);
                document.execCommand('copy');
                setCopied(true);
                toast.success('分享码已复制到剪贴板');
                setTimeout(() => setCopied(false), 2000);
            } else {
                toast.error('复制失败，请手动复制');
            }
        }
    }, [shareCode]);

    const handleSelectAll = useCallback(() => {
        if (textareaRef.current) {
            textareaRef.current.select();
            textareaRef.current.setSelectionRange(0, 99999);
        }
    }, []);

    return (
        <Dialog open={isOpen} onOpenChange={(open) => { if (!open) onClose(); }}>
            <CompactDialogContent className="sm:max-w-xl">
                <CompactDialogHeader
                    icon={<Share2 className="h-4 w-4" />}
                    title={`分享 ${title}`}
                    description="复制分享码发送给其他人以导入相同配置"
                />

                <CompactDialogBody className="space-y-4">
                    <div className="space-y-1.5">
                        <Label htmlFor="share-code-textarea">分享码</Label>
                        <div className="relative">
                            <Textarea
                                id="share-code-textarea"
                                ref={textareaRef}
                                value={shareCode}
                                readOnly
                                onClick={handleSelectAll}
                                className="h-32 resize-none bg-muted/30 pr-11 font-mono text-xs"
                                placeholder="生成中..."
                            />
                            <Button
                                type="button"
                                size="icon"
                                variant="ghost"
                                onClick={handleCopy}
                                aria-label="复制分享码"
                                className="absolute right-1.5 top-1.5 h-8 w-8 hover:bg-background/80"
                            >
                                {copied ? (
                                    <Check className="h-4 w-4 text-green-500" />
                                ) : (
                                    <Copy className="h-4 w-4" />
                                )}
                            </Button>
                        </div>
                        <p className="text-xs text-muted-foreground">
                            点击文本区域全选，或点击右上角按钮复制分享码。
                        </p>
                    </div>

                    <div className="rounded-lg border border-border bg-muted/40 px-3 py-2.5 text-xs text-muted-foreground">
                        <p className="mb-1 font-medium text-foreground">分享说明</p>
                        <ul className="space-y-1">
                            <li>• 将此分享码发送给其他人，他们可以导入相同的配置</li>
                            <li>• 分享码已经过压缩，请完整复制</li>
                            <li>• 如果是 Provider 分享，接收方需要输入正确的密码才能导入</li>
                        </ul>
                    </div>
                </CompactDialogBody>

                <CompactDialogFooter>
                    <Button
                        type="button"
                        variant="outline"
                        onClick={onClose}
                        className="min-h-11 px-4 sm:min-h-9"
                    >
                        关闭
                    </Button>
                    <Button
                        type="button"
                        onClick={handleCopy}
                        className="min-h-11 px-5 sm:min-h-9"
                    >
                        {copied ? '已复制' : '复制分享码'}
                    </Button>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
};

export default ShareDialog;