import React from 'react';
import { X } from 'lucide-react';
import { Button } from './ui/button';
import { Dialog, DialogClose, DialogContent, DialogFooter, DialogHeader, DialogTitle } from './ui/dialog';

interface FormDialogProps {
    title: string;
    onSubmit: () => void;
    onClose: () => void;
    isOpen: boolean;
    children: React.ReactNode;
}

/**
 * 表单对话框。移动端 / IME 打开时：
 * - 高度受 visualViewport CSS 变量约束，确认/取消留在可见区底部
 * - 压缩标题区内边距
 */
const FormDialog: React.FC<FormDialogProps> = ({ title, onSubmit, onClose, isOpen, children }) => {
    return (
        <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
            <DialogContent
                showCloseButton={false}
                className="max-w-md gap-0 overflow-hidden p-0 grid grid-rows-[auto_minmax(0,1fr)_auto] !max-h-[min(90dvh,var(--aipp-vv-height,90dvh))] top-[max(1rem,var(--aipp-vv-offset-top,0px))] translate-y-0 data-[state=open]:slide-in-from-top-2 sm:top-[50%] sm:translate-y-[-50%]"
                data-aipp-slot="form-dialog"
            >
                <DialogHeader className="border-b border-border px-4 py-3 sm:px-6 sm:py-6 shrink-0">
                    <div className="flex items-center justify-between gap-4">
                        <DialogTitle className="truncate pr-4 text-lg sm:text-xl">{title}</DialogTitle>
                        <DialogClose asChild>
                            <button
                                type="button"
                                className="flex h-11 w-11 flex-shrink-0 items-center justify-center rounded-lg p-2 transition-colors duration-200 hover:bg-muted"
                                aria-label="关闭"
                            >
                                <X className="h-5 w-5 text-muted-foreground" />
                            </button>
                        </DialogClose>
                    </div>
                </DialogHeader>

                <div className="min-h-0 overflow-y-auto overscroll-contain px-4 py-4 sm:px-6 sm:py-6">
                    {children}
                </div>

                <DialogFooter className="sticky bottom-0 z-10 border-t border-border bg-background px-4 py-3 sm:px-6 sm:py-4 sm:justify-end shrink-0 safe-bottom">
                    <Button
                        variant="outline"
                        onClick={onClose}
                        className="min-h-11 px-6"
                        data-aipp-slot="form-dialog-cancel"
                    >
                        取消
                    </Button>
                    <Button
                        onClick={onSubmit}
                        className="min-h-11 px-6 bg-primary hover:bg-primary/90 text-primary-foreground shadow-md hover:shadow-lg transition-all"
                        data-aipp-slot="form-dialog-confirm"
                    >
                        确认
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
};

export default FormDialog;
