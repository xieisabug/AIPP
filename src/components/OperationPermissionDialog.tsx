import {
    AlertDialog,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { FolderOpen, Shield, ShieldAlert, ShieldCheck } from "lucide-react";

export interface OperationPermissionRequest {
    request_id: string;
    operation: string;
    path: string;
    conversation_id?: number;
}

interface OperationPermissionDialogProps {
    request: OperationPermissionRequest | null;
    isOpen: boolean;
    onDecision: (requestId: string, decision: 'allow' | 'allow_and_save' | 'deny') => void;
}

const operationLabels: Record<string, string> = {
    read_file: "读取文件",
    write_file: "写入文件",
    edit_file: "编辑文件",
    list_directory: "列出目录",
};

export function OperationPermissionDialog({
    request,
    isOpen,
    onDecision,
}: OperationPermissionDialogProps) {
    if (!request) return null;

    const operationLabel = operationLabels[request.operation] || request.operation;

    const handleAllow = () => {
        onDecision(request.request_id, 'allow');
    };

    const handleAllowAndSave = () => {
        onDecision(request.request_id, 'allow_and_save');
    };

    const handleDeny = () => {
        onDecision(request.request_id, 'deny');
    };

    return (
        <AlertDialog open={isOpen}>
            <AlertDialogContent className="max-w-md">
                <AlertDialogHeader>
                    <AlertDialogTitle className="flex items-center gap-2">
                        <Shield className="h-5 w-5 text-yellow-500" />
                        操作权限请求
                    </AlertDialogTitle>
                    <AlertDialogDescription asChild>
                        <div className="space-y-3">
                            <p>AI 助手请求执行以下操作：</p>
                            <div className="rounded-md bg-muted p-3 space-y-2">
                                <div className="flex items-center gap-2 text-sm">
                                    <span className="font-medium text-foreground">操作:</span>
                                    <span className="text-muted-foreground">{operationLabel}</span>
                                </div>
                                <div className="flex items-start gap-2 text-sm">
                                    <FolderOpen className="h-4 w-4 mt-0.5 flex-shrink-0" />
                                    <span className="font-mono text-xs break-all text-foreground">
                                        {request.path}
                                    </span>
                                </div>
                            </div>
                            <p className="text-xs text-muted-foreground">
                                该路径不在允许访问的目录白名单中，请选择是否授权此操作。
                            </p>
                        </div>
                    </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter className="flex-col sm:flex-row gap-2">
                    <Button
                        variant="outline"
                        onClick={handleDeny}
                        className="flex items-center gap-2"
                    >
                        <ShieldAlert className="h-4 w-4" />
                        拒绝
                    </Button>
                    <Button
                        variant="outline"
                        onClick={handleAllow}
                        className="flex items-center gap-2"
                    >
                        <Shield className="h-4 w-4" />
                        仅本次允许
                    </Button>
                    <Button
                        onClick={handleAllowAndSave}
                        className="flex items-center gap-2"
                    >
                        <ShieldCheck className="h-4 w-4" />
                        允许并加入白名单
                    </Button>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    );
}
