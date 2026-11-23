import React from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from './ui/button';
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from './ui/dialog';

interface FormDialogProps {
    title: string;
    subtitle?: string;
    description?: string;
    submitLabel?: string;
    cancelLabel?: string;
    submitDisabled?: boolean;
    submitting?: boolean;
    maxWidthClassName?: string;
    footerLeft?: React.ReactNode;
    onSubmit: () => void;
    onClose: () => void;
    isOpen: boolean;
    children: React.ReactNode;
}

/**
 * 表单对话框。复用 CompactDialog 紧凑顶栏与底栏。
 * 移动端 / IME 打开时：
 * - 高度受 visualViewport CSS 变量约束，确认/取消留在可见区底部
 */
const FormDialog: React.FC<FormDialogProps> = ({
    title,
    subtitle,
    description,
    submitLabel = '确认',
    cancelLabel = '取消',
    submitDisabled = false,
    submitting = false,
    maxWidthClassName,
    footerLeft,
    onSubmit,
    onClose,
    isOpen,
    children,
}) => {
    return (
        <Dialog open={isOpen} onOpenChange={(open) => { if (!open && !submitting) onClose(); }}>
            <CompactDialogContent
                className={maxWidthClassName}
                data-aipp-slot="form-dialog"
            >
                <CompactDialogHeader
                    title={title}
                    subtitle={subtitle}
                    description={description ?? subtitle ?? title}
                    closeDisabled={submitting}
                />

                <CompactDialogBody>
                    {children}
                </CompactDialogBody>

                <CompactDialogFooter className="sticky bottom-0 z-10">
                    {footerLeft && <div className="mr-auto min-w-0 text-xs text-muted-foreground">{footerLeft}</div>}
                    <Button
                        type="button"
                        variant="outline"
                        disabled={submitting}
                        onClick={onClose}
                        className="min-h-11 px-4 sm:min-h-9"
                        data-aipp-slot="form-dialog-cancel"
                    >
                        {cancelLabel}
                    </Button>
                    <Button
                        type="button"
                        disabled={submitDisabled || submitting}
                        onClick={onSubmit}
                        className="min-h-11 px-5 sm:min-h-9"
                        data-aipp-slot="form-dialog-confirm"
                    >
                        {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
                        {submitLabel}
                    </Button>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
};

export default FormDialog;
