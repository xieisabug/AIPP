import React, { useState, useCallback } from 'react';
import { Lock, AlertTriangle, Loader2 } from 'lucide-react';
import { Button } from './ui/button';
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from './ui/dialog';
import { Label } from './ui/label';
import { PasswordInput } from './ui/password-input';
import { toast } from 'sonner';

interface PasswordDialogProps {
    title: string;
    isOpen: boolean;
    onClose: () => void;
    onConfirm: (password: string) => Promise<void>;
}

const isEnterSubmit = (event: React.KeyboardEvent) =>
    event.key === 'Enter' && !event.nativeEvent.isComposing && event.keyCode !== 229;

const PasswordDialog: React.FC<PasswordDialogProps> = ({
    title,
    isOpen,
    onClose,
    onConfirm,
}) => {
    const [password, setPassword] = useState('');
    const [confirmPassword, setConfirmPassword] = useState('');
    const [loading, setLoading] = useState(false);

    const isPasswordValid = password.length >= 6;
    const passwordsMatch = password === confirmPassword;
    const canSubmit = isPasswordValid && passwordsMatch && !loading;

    const handleSubmit = useCallback(async () => {
        if (!canSubmit) return;

        setLoading(true);
        try {
            await onConfirm(password);

            setPassword('');
            setConfirmPassword('');
            onClose();
            toast.success('导出成功');
        } catch (error) {
            toast.error(error instanceof Error ? error.message : '导出失败');
        } finally {
            setLoading(false);
        }
    }, [password, canSubmit, onConfirm, onClose]);

    const handleClose = useCallback(() => {
        if (loading) return;
        setPassword('');
        setConfirmPassword('');
        onClose();
    }, [loading, onClose]);

    const handleKeyDown: React.KeyboardEventHandler<HTMLInputElement> = (event) => {
        if (!isEnterSubmit(event) || !canSubmit) return;
        event.preventDefault();
        void handleSubmit();
    };

    return (
        <Dialog open={isOpen} onOpenChange={(open) => { if (!open) handleClose(); }}>
            <CompactDialogContent className="sm:max-w-md">
                <CompactDialogHeader
                    icon={<Lock className="h-4 w-4" />}
                    title="设置导出密码"
                    subtitle={title}
                    description={`为 ${title} 设置导出加密密码`}
                    closeDisabled={loading}
                />

                <CompactDialogBody className="space-y-4">
                    <p className="text-xs text-muted-foreground">
                        为了保护 {title} 的安全信息（如 API 密钥），请设置一个用于加密的密码。
                    </p>

                    <div className="space-y-1.5">
                        <Label htmlFor="export-password">设置密码 *</Label>
                        <PasswordInput
                            id="export-password"
                            autoFocus
                            value={password}
                            onChange={(e) => setPassword(e.target.value)}
                            onKeyDown={handleKeyDown}
                            placeholder="至少 6 个字符"
                            disabled={loading}
                            autoComplete="new-password"
                        />
                        {password && !isPasswordValid && (
                            <p className="text-xs text-destructive">密码至少需要 6 个字符</p>
                        )}
                    </div>

                    <div className="space-y-1.5">
                        <Label htmlFor="export-password-confirm">确认密码 *</Label>
                        <PasswordInput
                            id="export-password-confirm"
                            value={confirmPassword}
                            onChange={(e) => setConfirmPassword(e.target.value)}
                            onKeyDown={handleKeyDown}
                            placeholder="再次输入密码"
                            disabled={loading}
                            autoComplete="new-password"
                        />
                        {confirmPassword && !passwordsMatch && (
                            <p className="text-xs text-destructive">两次输入的密码不一致</p>
                        )}
                    </div>

                    <div className="flex items-start gap-2.5 rounded-lg border border-border bg-muted/40 px-3 py-2.5 text-xs text-muted-foreground">
                        <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                        <div className="space-y-1">
                            <p className="font-medium text-foreground">安全提示</p>
                            <ul className="space-y-0.5">
                                <li>• 请牢记此密码，丢失后无法恢复</li>
                                <li>• 建议使用强密码，包含字母、数字和特殊字符</li>
                                <li>• 导入时需要输入相同的密码才能解密</li>
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
                        disabled={!canSubmit}
                        className="min-h-11 px-5 sm:min-h-9"
                    >
                        {loading && <Loader2 className="h-4 w-4 animate-spin" />}
                        {loading ? '导出中...' : '确认导出'}
                    </Button>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
};

export default PasswordDialog;