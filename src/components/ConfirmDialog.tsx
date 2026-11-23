// ConfirmDialog.tsx
import React from 'react';
import { AlertDialog, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from './ui/alert-dialog';
import { Button } from './ui/button';

interface ConfirmDialogProps {
    title: string;
    confirmText: string;
    onConfirm: () => void;
    onCancel: () => void;
    isOpen: boolean;
}

const ConfirmDialog: React.FC<ConfirmDialogProps> = ({ title, confirmText, onConfirm, onCancel, isOpen }) => {
    if (!isOpen) return null;

    return (
        <AlertDialog open={isOpen} onOpenChange={onCancel}>
            <AlertDialogContent
                className="flex flex-col gap-0 overflow-hidden p-0 sm:max-w-md"
                data-aipp-slot="confirm-dialog"
            >
                <AlertDialogHeader className="border-b border-border px-4 py-3 text-left sm:px-5">
                    <AlertDialogTitle className="text-base font-semibold">{title}</AlertDialogTitle>
                </AlertDialogHeader>
                <div className="px-4 py-4 sm:px-5">
                    <AlertDialogDescription className="text-sm leading-relaxed text-muted-foreground">
                        {confirmText}
                    </AlertDialogDescription>
                </div>
                <AlertDialogFooter className="flex-row items-center justify-end gap-2 border-t border-border bg-background px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
                    <Button onClick={onCancel} variant="outline" className="min-h-11 px-4 sm:min-h-9" data-aipp-slot="confirm-dialog-cancel">取消</Button>
                    <Button onClick={onConfirm} variant="destructive" className="min-h-11 px-5 sm:min-h-9" data-aipp-slot="confirm-dialog-confirm">确认</Button>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    );
};

export default ConfirmDialog;
