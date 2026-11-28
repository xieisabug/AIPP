import { ReviewModelHint } from "./mcp-tool-components/ReviewModelHint";
import {
    AlertDialog,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { type ReactNode, useLayoutEffect } from "react";
import { createPortal } from "react-dom";
import { usePermissionHost } from "./conversation/ComposerPermissionSlot";
import { Button } from "@/components/ui/button";
import { Shield, ShieldAlert, ShieldCheck } from "lucide-react";
import { RawRequestDetails, RawText, ToolCallDetails } from "./permission/ToolCallDetails";
import { describeToolCall, isRecord, parseJson } from "./permission/toolCallView";

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
    agent_kind?: "acp" | "codex_app_server" | string;
    tool_call_id: string;
    title?: string;
    kind?: string;
    parameters?: string;
    options: AcpPermissionOption[];
    /** Present when AIPP's model review ran first and handed the decision to the user. */
    review?: { verdict: string; reason: string };
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
    superadmin_execute: "管理操作（仅本次批准）",
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

function PermissionSurface({ conversationId, open, width, title, actions, children }: {
    conversationId?: number;
    open: boolean;
    width: string;
    title: ReactNode;
    actions: ReactNode;
    children: ReactNode;
}) {
    const host = usePermissionHost(conversationId);
    useLayoutEffect(() => {
        if (!host || !open) return;
        const composer = host.parentElement;
        const input = composer?.querySelector<HTMLElement>(".input-area-textarea-container");
        if (!composer || !input) return;
        const align = () => {
            const box = input.getBoundingClientRect();
            const root = composer.getBoundingClientRect();
            host.style.left = (box.left - root.left) + "px";
            host.style.width = box.width + "px";
            host.style.top = (host.dataset.placement === "top" ? box.bottom - root.top + 6 : box.top - root.top - 6) + "px";
        };
        align();
        const observer = new ResizeObserver(align);
        observer.observe(input);
        observer.observe(composer);
        return () => observer.disconnect();
    }, [host, open]);
    if (!open) return null;
    if (host) return createPortal(
        <details open className="rounded-xl border bg-background px-3 py-2 text-sm shadow-lg">
            <summary className="cursor-pointer font-semibold"><span className="inline-flex items-center gap-2 align-middle">{title}</span></summary>
            <div className="mt-2 grid gap-2">{children}{actions}</div>
        </details>, host);
    return <AlertDialog open>
        <AlertDialogContent className={`${permissionDialogClassName} ${width}`}>
            <AlertDialogHeader className={permissionDialogHeaderClassName}>
                <AlertDialogTitle className="flex items-center gap-2">{title}</AlertDialogTitle>
                <AlertDialogDescription asChild>{children}</AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter className="min-w-0 overflow-hidden">{actions}</AlertDialogFooter>
        </AlertDialogContent>
    </AlertDialog>;
}

export function OperationPermissionDialog({
    request,
    isOpen,
    isSubmitting = false,
    onDecision,
    errorMessage,
}: OperationPermissionDialogProps) {
    if (!request) return null;

    const operationLabel = operationLabels[request.operation] || request.operation;
    const oneTimeOnly = request.operation === "superadmin_execute";

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
        <PermissionSurface key={request.request_id} conversationId={request.conversation_id} open={isOpen}
            width="!max-w-lg"
            title={<>
                <Shield className="h-5 w-5 text-muted-foreground" />
                操作权限请求
            </>}
            actions={
                <div className="grid w-full grid-cols-1 gap-2 sm:grid-cols-2">
                    <Button
                        variant="outline"
                        onClick={handleDeny}
                        disabled={isSubmitting}
                        className={neutralActionClassName}
                    >
                        <ShieldAlert className="h-4 w-4 shrink-0" />
                        拒绝
                    </Button>
                    <Button
                        variant="outline"
                        onClick={handleAllow}
                        disabled={isSubmitting}
                        className={neutralActionClassName}
                    >
                        <Shield className="h-4 w-4 shrink-0" />
                        仅本次允许
                    </Button>
                    {!oneTimeOnly && <>
                    <Button
                        variant="outline"
                        onClick={handleAllowForConversation}
                        disabled={isSubmitting}
                        className={neutralActionClassName}
                    >
                        <Shield className="h-4 w-4 shrink-0" />
                        对话期间信任
                    </Button>
                    <Button
                        variant="outline"
                        onClick={handleAllowForAssistant}
                        disabled={isSubmitting}
                        className={neutralActionClassName}
                    >
                        <ShieldCheck className="h-4 w-4 shrink-0" />
                        添加到助手工作区
                    </Button>
                    <Button
                        variant="outline"
                        onClick={handleAllowAndSave}
                        disabled={isSubmitting}
                        className={`${neutralActionClassName} sm:col-span-2`}
                    >
                        <ShieldCheck className="h-4 w-4 shrink-0" />
                        允许并加入全局白名单
                    </Button>
                    </>}
                </div>
            }>
            <div className={permissionDialogBodyClassName}>
                <p>AI 助手请求执行以下操作：</p>
                <div className={permissionDetailPanelClassName}>
                    <div className="flex items-start gap-2 text-sm">
                        <span className="shrink-0 font-medium text-foreground">操作:</span>
                        <span className="min-w-0 text-muted-foreground">{operationLabel}</span>
                    </div>
                    <div className="flex items-start gap-2 text-sm">
                        <span className="shrink-0 font-medium text-foreground">{oneTimeOnly ? "操作详情:" : "路径:"}</span>
                        <span className="min-w-0 break-all font-mono text-xs text-foreground">
                            {request.path}
                        </span>
                    </div>
                </div>
                <p className="text-xs text-muted-foreground">
                    {oneTimeOnly ? "批准只对上述具体参数和当前工作有效，5分钟后失效；目标或参数变化需重新批准。" : "该路径不在允许访问的目录白名单中，请选择是否授权此操作。"}
                </p>
                {errorMessage ? (
                    <p className="text-xs text-destructive">{errorMessage}</p>
                ) : null}
            </div>
        </PermissionSurface>
    );
}

// The native ACP bridge maps this reserved choice to ACP's cancelled outcome.
const bridgeCancelOption = "__aipp_cancel_permission__";

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

function McpApprovalDetails({ server, meta, children }: { server: string; meta: Record<string, unknown>; children?: ReactNode }) {
    const name = typeof meta.tool_name === "string" ? meta.tool_name : "";
    const labels = new Map<string, string>();
    const displayValues = new Map<string, unknown>();
    if (Array.isArray(meta.tool_params_display)) {
        for (const field of meta.tool_params_display) {
            if (!isRecord(field) || typeof field.name !== "string") continue;
            // 只采用真正翻译过的显示名；和参数名相同时让内置译名生效。
            if (typeof field.display_name === "string" && field.display_name !== field.name) {
                labels.set(field.name, field.display_name);
            }
            if ("value" in field) displayValues.set(field.name, field.value);
        }
    }
    // Use the actual invocation values even when a display hint abbreviates or omits them.
    const input = isRecord(meta.tool_params) ? meta.tool_params : Object.fromEntries(displayValues);
    const view = describeToolCall(name, input, server === "aipp" ? undefined : server);
    return <ToolCallDetails view={view} labels={labels}>
        {typeof meta.tool_description === "string" && <RawRequestDetails summary="工具说明" open={!view.label}>
            <p className="whitespace-pre-wrap break-words">{meta.tool_description}</p>
        </RawRequestDetails>}
        {children}
    </ToolCallDetails>;
}

/** Antigravity/Kiro/Pi 等原生通道：标题是工具名，参数是工具入参；Claude Code 的参数是整条 can_use_tool 请求。 */
function NativeToolPermissionDetails({ request }: { request: AcpPermissionRequest }) {
    const parsed = parseJson(request.parameters);
    const claude = request.agent_kind === "claude_sdk" && isRecord(parsed) && typeof parsed.tool_name === "string";
    const view = claude
        ? describeToolCall(parsed.tool_name as string, parsed.input)
        : describeToolCall(request.title || "", parsed);
    // 参数不是对象（纯文本或其他 JSON）时整体作为一项参数展示。
    if (!claude && !isRecord(parsed) && parsed != null && parsed !== "") view.args = [["parameters", parsed]];
    return <ToolCallDetails view={view} labels={new Map([["parameters", "参数"]])}>
        <RawRequestDetails>
            {request.title && <p className="break-all">{request.title}</p>}
            <p className="break-all">请求编号 {request.tool_call_id}</p>
            {request.parameters && typeof parsed !== "string" && <RawText text={request.parameters} />}
        </RawRequestDetails>
    </ToolCallDetails>;
}

function PermissionReviewNotice({ review }: { review: NonNullable<AcpPermissionRequest["review"]> }) {
    if (review.verdict === "manual") return <ReviewModelHint />;
    const timeout = review.reason.match(/自动审核模型超时（(\d+)秒）$/);
    return <div role="status" className="rounded-md border border-border bg-muted/50 px-3 py-2 text-sm text-foreground">
        {timeout ? <>
            <p>自动审核在 {timeout[1]} 秒内没有返回结果，请你决定是否允许。</p>
            <details className="mt-1 text-xs text-muted-foreground">
                <summary className="cursor-pointer">审核详情</summary>
                <p className="break-words">{review.reason}</p>
            </details>
        </> : review.reason}
    </div>;
}

function CodexPermissionDetails({ request }: { request: AcpPermissionRequest }) {
    let params: Record<string, unknown> = {};
    try {
        const parsed = JSON.parse(request.parameters ?? "{}");
        if (parsed && typeof parsed === "object") params = parsed;
    } catch { /* Keep the original payload accessible below. */ }
    const meta = isRecord(params._meta) && params._meta.codex_approval_kind === "mcp_tool_call" ? params._meta : undefined;
    const reason = params.reason || params.message || request.title;
    const fields = [
        ["命令", params.command],
        ["工作目录", params.cwd],
        ["服务", params.serverName],
        ["写入范围", params.grantRoot],
        ["申请权限", params.permissions],
        ["网络访问", params.networkApprovalContext],
        ["附加权限", params.additionalPermissions],
    ] as const;
    const raw = request.parameters && <RawRequestDetails summary="原始请求"><RawText text={request.parameters} /></RawRequestDetails>;
    if (meta) return <McpApprovalDetails server={typeof params.serverName === "string" ? params.serverName : "MCP"} meta={meta}>{raw}</McpApprovalDetails>;
    return (
        <div className={permissionDetailPanelClassName}>
            <p className="break-words font-medium">{typeof reason === "string" ? reason : "Codex 请求授权"}</p>
            {fields.map(([label, value]) => value != null && (
                <div key={label}>
                    <span className="text-xs text-muted-foreground">{label}</span>
                    <pre className={permissionCodeBlockClassName}>{typeof value === "string" ? value : JSON.stringify(value, null, 2)}</pre>
                </div>
            ))}
            {raw}
        </div>
    );
}

export function AcpPermissionDialog({
    request,
    isOpen,
    isSubmitting = false,
    onDecision,
    errorMessage,
}: AcpPermissionDialogProps) {
    if (!request) return null;

    const nativeAcp = request.agent_kind === "antigravity" || request.agent_kind === "kiro";
    const isBridgeCancel = (option: AcpPermissionOption) => nativeAcp && option.option_id === bridgeCancelOption;
    const hasCancel = request.options.some(option => option.option_id === "cancel" || isBridgeCancel(option));
    function optionLabel(option: AcpPermissionOption) {
        if (isBridgeCancel(option)) return "取消请求";
        if (request?.agent_kind === "codex_app_server") return option.name;
        return acpOptionLabel(option);
    }
    return (
        <PermissionSurface key={request.request_id} conversationId={request.conversation_id} open={isOpen}
            width="sm:!max-w-xl"
            title={<>
                <Shield className="h-5 w-5 text-muted-foreground" />
                {request.agent_kind === "codex_app_server"
                    ? "Codex 执行权限请求"
                    : `${({ pi: "Pi", cursor: "Cursor", opencode: "OpenCode", kiro: "Kiro", antigravity: "Antigravity", claude_sdk: "Claude Code" } as Record<string, string>)[request.agent_kind ?? ""] ?? "ACP"} 工具权限请求`}
            </>}
            actions={
                <div className={`grid min-w-0 w-full grid-cols-1 gap-2 ${nativeAcp && hasCancel && request.options.length === 3 ? "sm:grid-cols-3" : "sm:grid-cols-2"}`}>
                    {!hasCancel && <Button
                        variant="outline"
                        onClick={() => onDecision(request.request_id, undefined, true)}
                        disabled={isSubmitting}
                        className={neutralActionClassName}
                    >
                        <ShieldAlert className="h-4 w-4 shrink-0" />
                        取消请求
                    </Button>}
                    {request.options.map((option) => (
                        <Button
                            key={option.option_id}
                            variant="outline"
                            onClick={() => onDecision(request.request_id, option.option_id, false)}
                            disabled={isSubmitting}
                            className={neutralActionClassName}
                        >
                            {option.kind.startsWith("allow") ? (
                                <ShieldCheck className="h-4 w-4 shrink-0" />
                            ) : (
                                <ShieldAlert className="h-4 w-4 shrink-0" />
                            )}
                            {optionLabel(option)}
                        </Button>
                    ))}
                </div>
            }>
            <div className={permissionDialogBodyClassName}>
                {request.agent_kind === "codex_app_server"
                    ? <CodexPermissionDetails request={request} />
                    : <NativeToolPermissionDetails request={request} />}
                {request.review && request.review.verdict !== "manual" && <PermissionReviewNotice review={request.review} />}
                {errorMessage ? (
                    <p className="text-xs text-destructive">{errorMessage}</p>
                ) : null}
                {request.review?.verdict === "manual" && <ReviewModelHint />}
            </div>
        </PermissionSurface>
    );
}
