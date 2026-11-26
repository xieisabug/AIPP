import React, { useState, useCallback } from 'react';
import { Download, AlertCircle, Loader2 } from 'lucide-react';
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
import { PasswordInput } from './ui/password-input';
import { Textarea } from './ui/textarea';
import { parseProviderShareMessage } from "@/utils/providerShareMessage";
import { toast } from 'sonner';

interface ImportDialogProps {
    title: string;
    provider?: boolean;
    isOpen: boolean;
    requiresPassword?: boolean;
    onClose: () => void;
    onImport: (shareCode: string, password?: string, newName?: string) => Promise<boolean | void>;
}

const ImportDialog: React.FC<ImportDialogProps> = ({
    title,
    provider,
    isOpen,
    requiresPassword = false,
    onClose,
    onImport,
}) => {
    const [shareCode, setShareCode] = useState('');
    const [password, setPassword] = useState('');
    const [newName, setNewName] = useState('');
    const [loading, setLoading] = useState(false);

    const parsed = provider ? parseProviderShareMessage(shareCode) : null;
    const handleSubmit = useCallback(async () => {
        if (!shareCode.trim()) {
            toast.error('请输入分享码');
            return;
        }

        if (provider && !parsed) { toast.error("分享码好像不完整，请让对方把整段消息重新复制给你"); return; }
        if (requiresPassword && !parsed?.password && !password.trim()) {
            toast.error('请输入密码');
            return;
        }

        setLoading(true);
        try {
            const imported = await onImport(
                parsed?.code ?? shareCode.trim(),
                parsed?.password || password.trim() || undefined,
                newName.trim() || undefined,
            );

            setShareCode('');
            setPassword('');
            setNewName('');
            onClose();
            if (imported !== false) {
                toast.success('导入成功');
            }
        } catch (error) {
            toast.error(error instanceof Error ? error.message : String(error));
        } finally {
            setLoading(false);
        }
    }, [shareCode, password, newName, requiresPassword, onImport, onClose, provider, parsed]);

    const handleClose = useCallback(() => {
        if (loading) return;
        setShareCode('');
        setPassword('');
        setNewName('');
        onClose();
    }, [loading, onClose]);

    return (
        <Dialog open={isOpen} onOpenChange={(open) => { if (!open) handleClose(); }}>
            <CompactDialogContent className="sm:max-w-md">
                <CompactDialogHeader
                    icon={<Download className="h-4 w-4" />}
                    title={`导入 ${title}`}
                    description="粘贴分享码以导入配置"
                    closeDisabled={loading}
                />

                <CompactDialogBody className="space-y-4">
                    <div className="space-y-1.5">
                        <Label htmlFor="import-share-code">分享码 *</Label>
                        <Textarea
                            id="import-share-code"
                            value={shareCode}
                            onChange={(e) => setShareCode(e.target.value)}
                            placeholder={provider ? "粘贴整段分享消息或分享码" : "请粘贴分享码..."}
                            disabled={loading}
                            autoCapitalize="none"
                            autoFocus
                            className="h-24 resize-none font-mono text-xs"
                        />
                    </div>

                    {requiresPassword && (!provider || (!!shareCode && !parsed?.password)) && (
                        <div className="space-y-1.5">
                            <Label htmlFor="import-password">密码 *</Label>
                            <PasswordInput
                                id="import-password"
                                value={password}
                                onChange={(e) => setPassword(e.target.value)}
                                placeholder="请输入解密密码"
                                disabled={loading}
                                autoComplete="off"
                            />
                        </div>
                    )}

                    <div className="space-y-1.5">
                        <Label htmlFor="import-custom-name">自定义名称（可选）</Label>
                        <Input
                            id="import-custom-name"
                            value={newName}
                            onChange={(e) => setNewName(e.target.value)}
                            placeholder="留空则使用默认名称"
                            disabled={loading}
                            autoComplete="off"
                        />
                    </div>

                    <div className="flex items-start gap-2.5 rounded-lg border border-border bg-muted/40 px-3 py-2.5 text-xs text-muted-foreground">
                        <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                        <div className="space-y-1">
                            <p className="font-medium text-foreground">导入说明</p>
                            <ul className="space-y-0.5">
                                <li>• 请确保分享码完整且正确</li>
                                {requiresPassword && <li>• Provider 配置需要输入正确的密码</li>}
                                <li>{provider ? "相同配置会复用已有提供商" : "导入的配置会作为新项目添加，不会覆盖现有配置"}</li>
                            </ul>
                        </div>
                    </div>
                </CompactDialogBody>

                <CompactDialogFooter>
                    <Button
                        type="button"
                        variant="outline"
                        onClick={handleClose}
                        disabled={loading}
                        className="min-h-11 px-4 sm:min-h-9"
                    >
                        取消
                    </Button>
                    <Button
                        type="button"
                        onClick={handleSubmit}
                        disabled={loading || !shareCode.trim()}
                        className="min-h-11 px-5 sm:min-h-9"
                    >
                        {loading && <Loader2 className="h-4 w-4 animate-spin" />}
                        {loading ? '导入中...' : '确认导入'}
                    </Button>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
};

export default ImportDialog;