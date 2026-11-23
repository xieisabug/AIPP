import React from 'react';
import { Package } from 'lucide-react';
import { AlertDialog, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from './ui/alert-dialog';
import { Button } from './ui/button';

interface EnvironmentInstallDialogProps {
    tool: string;
    message: string;
    isOpen: boolean;
    onConfirm: () => void;
    onCancel: () => void;
}

const EnvironmentInstallDialog: React.FC<EnvironmentInstallDialogProps> = ({ 
    tool, 
    message, 
    isOpen, 
    onConfirm, 
    onCancel 
}) => {
    if (!isOpen) return null;

    const getToolDisplayName = (tool: string) => {
        switch (tool) {
            case 'bun':
                return 'Bun';
            case 'uv':
                return 'uv';
            default:
                return tool;
        }
    };

    const getToolDescription = (tool: string) => {
        switch (tool) {
            case 'bun':
                return 'Bun 是一个快速的 JavaScript 运行时和包管理器，用于运行 React 和 Vue 组件预览。';
            case 'uv':
                return 'uv 是一个极快的 Python 包管理器，用于管理 Python 项目依赖。';
            default:
                return `${tool} 是预览功能所需的环境工具。`;
        }
    };

    return (
        <AlertDialog open={isOpen} onOpenChange={onCancel}>
            <AlertDialogContent className="flex flex-col gap-0 overflow-hidden p-0 sm:max-w-md">
                <AlertDialogHeader className="border-b border-border px-4 py-3 text-left sm:px-5">
                    <AlertDialogTitle className="flex items-center gap-2 text-base font-semibold">
                        <Package className="h-4 w-4 text-muted-foreground" />
                        需要安装 {getToolDisplayName(tool)}
                    </AlertDialogTitle>
                </AlertDialogHeader>
                <div className="space-y-3 px-4 py-4 sm:px-5">
                    <AlertDialogDescription className="text-sm text-muted-foreground">
                        {message}
                    </AlertDialogDescription>
                    <div className="rounded-lg bg-muted/50 px-3 py-2.5 text-xs text-foreground">
                        {getToolDescription(tool)}
                    </div>
                    <p className="text-xs text-muted-foreground">
                        安装过程可能需要几分钟时间，请耐心等待。
                    </p>
                </div>
                <AlertDialogFooter className="flex-row items-center justify-end gap-2 border-t border-border bg-background px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
                    <Button
                        onClick={onCancel}
                        variant="outline"
                        className="min-h-11 px-4 sm:min-h-9"
                    >
                        取消预览
                    </Button>
                    <Button
                        onClick={onConfirm}
                        className="min-h-11 px-5 sm:min-h-9"
                    >
                        自动安装
                    </Button>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    );
};

export default EnvironmentInstallDialog;