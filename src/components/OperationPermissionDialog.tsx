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

export interface AcpPermissionOption {
    option_id: string;
    name: string;
    kind: string;
}

export interface AcpPermissionRequest {
    request_id: string;
    conversation_id?: number;
    tool_call_id: string;
    title?: string;
    kind?: string;
    parameters?: string;
    options: AcpPermissionOption[];
}

interface OperationPermissionDialogProps {
    request: OperationPermissionRequest | null;
    isOpen: boolean;
    onDecision: (requestId: string, decision: 'allow' | 'allow_and_save' | 'deny') => void;
}

interface AcpPermissionDialogProps {
    request: AcpPermissionRequest | null;
    isOpen: boolean;
    onDecision: (requestId: string, optionId?: string, cancelled?: boolean) => void;
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

const acpOptionStyle = (kind: string) => {
    switch (kind) {
        case "allow_always":
            return "default" as const;
        case "allow_once":
            return "outline" as const;
        case "reject_once":
        case "reject_always":
            return "destructive" as const;
        default:
            return "outline" as const;
    }
};

export function AcpPermissionDialog({
    request,
    isOpen,
    onDecision,
}: AcpPermissionDialogProps) {
    if (!request) return null;

    return (
        <AlertDialog open={isOpen}>
            <AlertDialogContent className="max-w-lg">
                <AlertDialogHeader>
                    <AlertDialogTitle className="flex items-center gap-2">
                        <Shield className="h-5 w-5 text-yellow-500" />
                        ACP 工具权限请求
                    </AlertDialogTitle>
                    <AlertDialogDescription asChild>
                        <div className="space-y-3">
                            <p>AI 助手请求执行以下工具调用：</p>
                            <div className="rounded-md bg-muted p-3 space-y-2">
                                <div className="flex items-center gap-2 text-sm">
                                    <span className="font-medium text-foreground">标题:</span>
                                    <span className="text-muted-foreground">
                                        {request.title || "未命名"}
                                    </span>
                                </div>
                                <div className="flex items-center gap-2 text-sm">
                                    <span className="font-medium text-foreground">类型:</span>
                                    <span className="text-muted-foreground">{request.kind || "unknown"}</span>
                                </div>
                                <div className="flex items-start gap-2 text-sm">
                                    <span className="font-medium text-foreground">ToolCallId:</span>
                                    <span className="font-mono text-xs break-all text-foreground">
                                        {request.tool_call_id}
                                    </span>
                                </div>
                                {request.parameters && (
                                    <div className="text-sm">
                                        <span className="font-medium text-foreground">参数:</span>
                                        <pre className="text-xs font-mono p-2 mt-2 whitespace-pre-wrap break-words bg-background rounded-md">
                                            {request.parameters}
                                        </pre>
                                    </div>
                                )}
                            </div>
                        </div>
                    </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter className="flex-col sm:flex-row gap-2">
                    <Button
                        variant="outline"
                        onClick={() => onDecision(request.request_id, undefined, true)}
                        className="flex items-center gap-2"
                    >
                        <ShieldAlert className="h-4 w-4" />
                        取消
                    </Button>
                    {request.options.map((option) => (
                        <Button
                            key={option.option_id}
                            variant={acpOptionStyle(option.kind)}
                            onClick={() => onDecision(request.request_id, option.option_id, false)}
                            className="flex items-center gap-2"
                        >
                            {option.kind.startsWith("allow") ? (
                                <ShieldCheck className="h-4 w-4" />
                            ) : (
                                <ShieldAlert className="h-4 w-4" />
                            )}
                            {option.name}
                        </Button>
                    ))}
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    );
}
