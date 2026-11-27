import React, { useEffect, useState, useCallback, useRef } from 'react';
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
import { Checkbox } from './ui/checkbox';
import { Textarea } from './ui/textarea';
import { providerShareMessage } from "@/utils/providerShareMessage";
import { toast } from 'sonner';

interface ShareDialogProps {
    title: string;
    provider?: {name: string; password: string};
    shareCode: string;
    isOpen: boolean;
    onClose: () => void;
}

const ShareDialog: React.FC<ShareDialogProps> = ({
    title,
    provider,
    shareCode,
    isOpen,
    onClose,
}) => {
    const [copied, setCopied] = useState(false);
    const textareaRef = useRef<HTMLTextAreaElement>(null);

    const [includePassword, setIncludePassword] = useState(true);
    useEffect(() => {if (isOpen) setIncludePassword(true);}, [isOpen]);
    const preview = provider ? providerShareMessage(provider.name, shareCode, includePassword ? provider.password : undefined) : shareCode;
    const handleCopy = useCallback(async (value = preview) => {
        const message = provider && value !== shareCode ? '分享消息已复制到剪贴板' : '分享码已复制到剪贴板';
        try {
            await navigator.clipboard.writeText(value);
            setCopied(true);
            toast.success(message);
            setTimeout(() => setCopied(false), 2000);
        } catch (error) {
            if (textareaRef.current && value === preview) {
                textareaRef.current.select();
                textareaRef.current.setSelectionRange(0, 99999);
                document.execCommand('copy');
                setCopied(true);
                toast.success(message);
                setTimeout(() => setCopied(false), 2000);
            } else {
                toast.error('复制失败，请手动复制');
            }
        }
    }, [preview, provider, shareCode]);

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
                    description={provider ? "可将整段消息转发给接收方" : "复制分享码发送给其他人以导入相同配置"}
                />

                <CompactDialogBody className="space-y-4">
                    <div className="space-y-1.5">
                        <Label htmlFor="share-code-textarea">{provider ? "分享消息" : "分享码"}</Label>
                        <div className="relative">
                            <Textarea
                                id="share-code-textarea"
                                ref={textareaRef}
                                value={preview}
                                readOnly
                                onClick={handleSelectAll}
                                className="h-32 resize-none bg-muted/30 pr-11 font-mono text-xs"
                                placeholder="生成中..."
                            />
                            <Button
                                type="button"
                                size="icon"
                                variant="ghost"
                                onClick={() => void handleCopy()}
                                aria-label={provider ? "复制完整消息" : "复制分享码"}
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
                            点击文本区域全选。
                        </p>
                    </div>

                    {provider ? <div className="space-y-3 text-sm"><label className="flex items-center gap-2"><Checkbox checked={includePassword} onCheckedChange={v => setIncludePassword(v === true)} />消息里包含密码</label><p>拿到这段消息的人都能用你的 Key 调用模型，费用记在你的账户上。</p></div> : <div className="rounded-lg border border-border bg-muted/40 px-3 py-2.5 text-xs text-muted-foreground">
                        <p className="mb-1 font-medium text-foreground">分享说明</p>
                        <ul className="space-y-1">
                            <li>• 将此分享码发送给其他人，他们可以导入相同的配置</li>
                            <li>• 请完整复制分享码</li>

                        </ul>
                    </div>}
                </CompactDialogBody>

                <CompactDialogFooter>
                    {provider && <Button type="button" variant="outline" onClick={() => void handleCopy(shareCode)}>只复制分享码</Button>}
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
                        onClick={() => void handleCopy()}
                        className="min-h-11 px-5 sm:min-h-9"
                    >
                        {copied ? '已复制' : provider ? '复制分享消息' : '复制分享码'}
                    </Button>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
};

export default ShareDialog;