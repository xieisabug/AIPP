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

const FormDialog: React.FC<FormDialogProps> = ({ title, onSubmit, onClose, isOpen, children }) => {
    return (
        <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
            <DialogContent showCloseButton={false} className="max-w-md gap-0 overflow-hidden p-0">
                <DialogHeader className="border-b border-border px-6 py-6">
                    <div className="flex items-center justify-between gap-4">
                        <DialogTitle className="truncate pr-4 text-xl">{title}</DialogTitle>
                        <DialogClose asChild>
                            <button className="flex-shrink-0 rounded-lg p-2 transition-colors duration-200 hover:bg-muted">
                                <X className="h-5 w-5 text-muted-foreground" />
                            </button>
                        </DialogClose>
                    </div>
                </DialogHeader>

                <div className="px-6 py-6">
                    {children}
                </div>

                <DialogFooter className="border-t border-border px-6 py-4 sm:justify-end">
                    <Button
                        variant="outline"
                        onClick={onClose}
                        className="px-6"
                    >
                        取消
                    </Button>
                    <Button
                        onClick={onSubmit}
                        className="px-6 bg-primary hover:bg-primary/90 text-primary-foreground shadow-md hover:shadow-lg transition-all"
                    >
                        确认
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
};

export default FormDialog;
