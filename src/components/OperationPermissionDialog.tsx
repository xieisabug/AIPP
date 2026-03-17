import {
    AlertDialog,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Shield, ShieldAlert, ShieldCheck } from "lucide-react";

// Re-export types from InlineInteractionCards for backward compatibility
export type {
    AskUserQuestionOption,
    AskUserQuestionItem,
    AskUserQuestionMetadata,
    AskUserQuestionRequest,
    PreviewFileItem,
    PreviewFileMetadata,
    PreviewFileRequest,
} from "./InlineInteractionCards";

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
    onDecision: (
        requestId: string,
        decision: 'allow' | 'allow_for_conversation' | 'allow_for_assistant' | 'allow_and_save' | 'deny'
    ) => void;
    errorMessage?: string | null;
}

interface AcpPermissionDialogProps {
    request: AcpPermissionRequest | null;
    isOpen: boolean;
    onDecision: (requestId: string, optionId?: string, cancelled?: boolean) => void;
    errorMessage?: string | null;
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
    errorMessage,
}: OperationPermissionDialogProps) {
    if (!request) return null;

    const operationLabel = operationLabels[request.operation] || request.operation;

    const handleDeny = () => {
        onDecision(request.request_id, 'deny');
    };

    const handleAllow = () => {
        onDecision(request.request_id, 'allow');
    };

    const handleAllowForConversation = () => {
        onDecision(request.request_id, 'allow_for_conversation');
    };

    const handleAllowForAssistant = () => {
        onDecision(request.request_id, 'allow_for_assistant');
    };

    const handleAllowAndSave = () => {
        onDecision(request.request_id, 'allow_and_save');
    };

    return (
        <AlertDialog open={isOpen}>
            <AlertDialogContent className="max-h-[85vh] max-w-lg overflow-hidden">
                <AlertDialogHeader>
                    <AlertDialogTitle className="flex items-center gap-2">
                        <Shield className="h-5 w-5 text-yellow-500" />
                        操作权限请求
                    </AlertDialogTitle>
                    <AlertDialogDescription asChild>
                        <div className="max-h-[50vh] space-y-3 overflow-y-auto pr-1">
                            <p>AI 助手请求执行以下操作：</p>
                            <div className="space-y-2 rounded-md bg-muted p-3">
                                <div className="flex items-start gap-2 text-sm">
                                    <span className="shrink-0 font-medium text-foreground">操作:</span>
                                    <span className="min-w-0 text-muted-foreground">{operationLabel}</span>
                                </div>
                                <div className="flex items-start gap-2 text-sm">
                                    <span className="shrink-0 font-medium text-foreground">路径:</span>
                                    <span className="min-w-0 break-all font-mono text-xs text-foreground">
                                        {request.path}
                                    </span>
                                </div>
                            </div>
                            <p className="text-xs text-muted-foreground">
                                该路径不在允许访问的目录白名单中，请选择是否授权此操作。
                            </p>
                            {errorMessage ? (
                                <p className="text-xs text-destructive">{errorMessage}</p>
                            ) : null}
                        </div>
                    </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                    <div className="grid w-full grid-cols-1 gap-2 sm:grid-cols-2">
                        <Button
                            variant="outline"
                            onClick={handleDeny}
                            className="flex w-full items-center justify-start gap-2 whitespace-normal text-left"
                        >
                            <ShieldAlert className="h-4 w-4 shrink-0" />
                            拒绝
                        </Button>
                        <Button
                            variant="outline"
                            onClick={handleAllow}
                            className="flex w-full items-center justify-start gap-2 whitespace-normal text-left"
                        >
                            <Shield className="h-4 w-4 shrink-0" />
                            仅本次允许
                        </Button>
                        <Button
                            variant="outline"
                            onClick={handleAllowForConversation}
                            className="flex w-full items-center justify-start gap-2 whitespace-normal text-left"
                        >
                            <Shield className="h-4 w-4 shrink-0" />
                            对话期间信任
                        </Button>
                        <Button
                            variant="outline"
                            onClick={handleAllowForAssistant}
                            className="flex w-full items-center justify-start gap-2 whitespace-normal text-left"
                        >
                            <ShieldCheck className="h-4 w-4 shrink-0" />
                            添加到助手工作区
                        </Button>
                        <Button
                            onClick={handleAllowAndSave}
                            className="flex w-full items-center justify-start gap-2 whitespace-normal text-left sm:col-span-2"
                        >
                            <ShieldCheck className="h-4 w-4 shrink-0" />
                            允许并加入全局白名单
                        </Button>
                    </div>
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
    errorMessage,
}: AcpPermissionDialogProps) {
    if (!request) return null;

    return (
        <AlertDialog open={isOpen}>
            <AlertDialogContent className="max-h-[85vh] max-w-2xl overflow-hidden">
                <AlertDialogHeader>
                    <AlertDialogTitle className="flex items-center gap-2">
                        <Shield className="h-5 w-5 text-yellow-500" />
                        ACP 工具权限请求
                    </AlertDialogTitle>
                    <AlertDialogDescription asChild>
                        <div className="max-h-[50vh] space-y-3 overflow-y-auto pr-1">
                            <p>AI 助手请求执行以下工具调用：</p>
                            <div className="space-y-2 rounded-md bg-muted p-3">
                                <div className="flex items-start gap-2 text-sm">
                                    <span className="shrink-0 font-medium text-foreground">标题:</span>
                                    <span className="min-w-0 break-words text-muted-foreground">
                                        {request.title || "未命名"}
                                    </span>
                                </div>
                                <div className="flex items-start gap-2 text-sm">
                                    <span className="shrink-0 font-medium text-foreground">类型:</span>
                                    <span className="min-w-0 break-words text-muted-foreground">
                                        {request.kind || "unknown"}
                                    </span>
                                </div>
                                <div className="flex items-start gap-2 text-sm">
                                    <span className="shrink-0 font-medium text-foreground">ToolCallId:</span>
                                    <span className="min-w-0 break-all font-mono text-xs text-foreground">
                                        {request.tool_call_id}
                                    </span>
                                </div>
                                {request.parameters && (
                                    <div className="text-sm">
                                        <span className="font-medium text-foreground">参数:</span>
                                        <pre className="mt-2 max-h-60 overflow-auto rounded-md bg-background p-2 font-mono text-xs whitespace-pre-wrap break-words">
                                            {request.parameters}
                                        </pre>
                                    </div>
                                )}
                            </div>
                            {errorMessage ? (
                                <p className="text-xs text-destructive">{errorMessage}</p>
                            ) : null}
                        </div>
                    </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                    <div className="grid w-full grid-cols-1 gap-2 sm:grid-cols-2">
                        <Button
                            variant="outline"
                            onClick={() => onDecision(request.request_id, undefined, true)}
                            className="flex w-full items-center justify-start gap-2 whitespace-normal text-left"
                        >
                            <ShieldAlert className="h-4 w-4 shrink-0" />
                            取消
                        </Button>
                        {request.options.map((option) => (
                            <Button
                                key={option.option_id}
                                variant={acpOptionStyle(option.kind)}
                                onClick={() => onDecision(request.request_id, option.option_id, false)}
                                className="flex w-full items-center justify-start gap-2 whitespace-normal text-left"
                            >
                                {option.kind.startsWith("allow") ? (
                                    <ShieldCheck className="h-4 w-4 shrink-0" />
                                ) : (
                                    <ShieldAlert className="h-4 w-4 shrink-0" />
                                )}
                                {option.name}
                            </Button>
                        ))}
                    </div>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    );
}
