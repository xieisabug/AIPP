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
            <AlertDialogContent data-aipp-slot="confirm-dialog">
                <AlertDialogHeader>
                    <AlertDialogTitle>{title}</AlertDialogTitle>
                    <AlertDialogDescription>
                        <p>{confirmText}</p>
                    </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                    <Button onClick={onCancel} variant="outline" className="min-h-11 min-w-11" data-aipp-slot="confirm-dialog-cancel">取消</Button>
                    <Button onClick={onConfirm} variant="destructive" className="min-h-11 min-w-11" data-aipp-slot="confirm-dialog-confirm">确认</Button>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    );
};

export default ConfirmDialog;
