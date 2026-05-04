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
    isSubmitting?: boolean;
    onDecision: (
        requestId: string,
        decision: 'allow' | 'allow_for_conversation' | 'allow_for_assistant' | 'allow_and_save' | 'deny'
    ) => void;
    errorMessage?: string | null;
}

interface AcpPermissionDialogProps {
    request: AcpPermissionRequest | null;
    isOpen: boolean;
    isSubmitting?: boolean;
    onDecision: (requestId: string, optionId?: string, cancelled?: boolean) => void;
    errorMessage?: string | null;
}

const operationLabels: Record<string, string> = {
    read_file: "读取文件",
    write_file: "写入文件",
    edit_file: "编辑文件",
    list_directory: "列出目录",
};

const permissionDialogClassName =
    "grid !max-h-[calc(100dvh-2rem)] !w-[calc(100vw-2rem)] grid-rows-[minmax(0,1fr)_auto] overflow-hidden";
const permissionDialogHeaderClassName = "min-h-0 min-w-0 overflow-hidden";
const permissionDialogBodyClassName = "min-h-0 min-w-0 space-y-3 overflow-y-auto overflow-x-hidden pr-1";
const permissionDetailPanelClassName = "min-w-0 space-y-2 overflow-hidden rounded-md bg-muted p-3";
const permissionCodeBlockClassName =
    "mt-2 max-h-56 w-full max-w-full overflow-auto rounded-md bg-background p-2 font-mono text-xs whitespace-pre-wrap break-all";
const neutralActionClassName =
    "flex !h-auto min-h-9 w-full items-center !justify-start gap-2 !whitespace-normal !border-border !bg-background py-2 text-left !text-foreground hover:!bg-muted hover:!text-foreground";
const denyActionClassName =
    "flex !h-auto min-h-9 w-full items-center !justify-start gap-2 !whitespace-normal !border-red-500/60 !bg-red-50 py-2 text-left !text-red-700 hover:!bg-red-100 hover:!text-red-800 dark:!border-red-500/40 dark:!bg-red-950/30 dark:!text-red-300 dark:hover:!bg-red-950/50";
const denyStrongActionClassName =
    "flex !h-auto min-h-9 w-full items-center !justify-start gap-2 !whitespace-normal !bg-red-600 py-2 text-left !text-white hover:!bg-red-700 dark:!bg-red-700 dark:hover:!bg-red-800";
const allowActionClassName =
    "flex !h-auto min-h-9 w-full items-center !justify-start gap-2 !whitespace-normal !border-emerald-500/60 !bg-emerald-50 py-2 text-left !text-emerald-800 hover:!bg-emerald-100 hover:!text-emerald-900 dark:!border-emerald-500/40 dark:!bg-emerald-950/30 dark:!text-emerald-300 dark:hover:!bg-emerald-950/50";
const allowStrongActionClassName =
    "flex !h-auto min-h-9 w-full items-center !justify-start gap-2 !whitespace-normal !bg-emerald-600 py-2 text-left !text-white hover:!bg-emerald-700 dark:!bg-emerald-700 dark:hover:!bg-emerald-800";

export function OperationPermissionDialog({
    request,
    isOpen,
    isSubmitting = false,
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
            <AlertDialogContent className={`${permissionDialogClassName} !max-w-lg`}>
                <AlertDialogHeader className={permissionDialogHeaderClassName}>
                    <AlertDialogTitle className="flex items-center gap-2">
                        <Shield className="h-5 w-5 text-yellow-500" />
                        操作权限请求
                    </AlertDialogTitle>
                    <AlertDialogDescription asChild>
                        <div className={permissionDialogBodyClassName}>
                            <p>AI 助手请求执行以下操作：</p>
                            <div className={permissionDetailPanelClassName}>
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
                            disabled={isSubmitting}
                            className={denyActionClassName}
                        >
                            <ShieldAlert className="h-4 w-4 shrink-0" />
                            拒绝
                        </Button>
                        <Button
                            variant="outline"
                            onClick={handleAllow}
                            disabled={isSubmitting}
                            className={allowActionClassName}
                        >
                            <Shield className="h-4 w-4 shrink-0" />
                            仅本次允许
                        </Button>
                        <Button
                            variant="outline"
                            onClick={handleAllowForConversation}
                            disabled={isSubmitting}
                            className={allowActionClassName}
                        >
                            <Shield className="h-4 w-4 shrink-0" />
                            对话期间信任
                        </Button>
                        <Button
                            variant="outline"
                            onClick={handleAllowForAssistant}
                            disabled={isSubmitting}
                            className={allowActionClassName}
                        >
                            <ShieldCheck className="h-4 w-4 shrink-0" />
                            添加到助手工作区
                        </Button>
                        <Button
                            onClick={handleAllowAndSave}
                            disabled={isSubmitting}
                            className={`${allowStrongActionClassName} sm:col-span-2`}
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

const acpOptionLabel = (option: AcpPermissionOption) => {
    switch (option.kind) {
        case "allow_once":
            return "本次允许";
        case "allow_always":
            return "始终允许";
        case "reject_once":
            return "本次拒绝";
        case "reject_always":
            return "始终拒绝";
        default:
            return option.name || "未知选项";
    }
};

const acpOptionClassName = (kind: string) => {
    switch (kind) {
        case "allow_always":
            return allowStrongActionClassName;
        case "allow_once":
            return allowActionClassName;
        case "reject_always":
            return denyStrongActionClassName;
        case "reject_once":
            return denyActionClassName;
        default:
            return neutralActionClassName;
    }
};

export function AcpPermissionDialog({
    request,
    isOpen,
    isSubmitting = false,
    onDecision,
    errorMessage,
}: AcpPermissionDialogProps) {
    if (!request) return null;

    return (
        <AlertDialog open={isOpen}>
            <AlertDialogContent className={`${permissionDialogClassName} sm:!max-w-xl`}>
                <AlertDialogHeader className={permissionDialogHeaderClassName}>
                    <AlertDialogTitle className="flex items-center gap-2">
                        <Shield className="h-5 w-5 text-yellow-500" />
                        ACP 工具权限请求
                    </AlertDialogTitle>
                    <AlertDialogDescription asChild>
                        <div className={permissionDialogBodyClassName}>
                            <p>AI 助手请求执行以下工具调用：</p>
                            <div className={permissionDetailPanelClassName}>
                                <div className="flex items-start gap-2 text-sm">
                                    <span className="shrink-0 font-medium text-foreground">标题:</span>
                                    <span className="min-w-0 break-words text-muted-foreground">
                                        {request.title || "未命名"}
                                    </span>
                                </div>
                                <div className="flex items-start gap-2 text-sm">
                                    <span className="shrink-0 font-medium text-foreground">类型:</span>
                                    <span className="min-w-0 break-words text-muted-foreground">
                                        {request.kind || "未知"}
                                    </span>
                                </div>
                                <div className="flex items-start gap-2 text-sm">
                                    <span className="shrink-0 font-medium text-foreground">工具调用 ID:</span>
                                    <span className="min-w-0 break-all font-mono text-xs text-foreground">
                                        {request.tool_call_id}
                                    </span>
                                </div>
                                {request.parameters && (
                                    <div className="min-w-0 text-sm">
                                        <span className="font-medium text-foreground">参数:</span>
                                        <pre className={permissionCodeBlockClassName}>
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
                <AlertDialogFooter className="min-w-0 overflow-hidden">
                    <div className="grid min-w-0 w-full grid-cols-1 gap-2 sm:grid-cols-2">
                        <Button
                            variant="outline"
                            onClick={() => onDecision(request.request_id, undefined, true)}
                            disabled={isSubmitting}
                            className={neutralActionClassName}
                        >
                            <ShieldAlert className="h-4 w-4 shrink-0" />
                            取消请求
                        </Button>
                        {request.options.map((option) => (
                            <Button
                                key={option.option_id}
                                variant={option.kind === "allow_always" || option.kind === "reject_always" ? "default" : "outline"}
                                onClick={() => onDecision(request.request_id, option.option_id, false)}
                                disabled={isSubmitting}
                                className={acpOptionClassName(option.kind)}
                            >
                                {option.kind.startsWith("allow") ? (
                                    <ShieldCheck className="h-4 w-4 shrink-0" />
                                ) : (
                                    <ShieldAlert className="h-4 w-4 shrink-0" />
                                )}
                                {acpOptionLabel(option)}
                            </Button>
                        ))}
                    </div>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    );
}
