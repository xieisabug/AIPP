import { useEffect, useMemo, useState } from "react";
import {
    AlertDialog,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import UnifiedMarkdown from "@/components/UnifiedMarkdown";
import { FileText, FolderOpen, ImageIcon, Shield, ShieldAlert, ShieldCheck } from "lucide-react";

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

export interface AskUserQuestionOption {
    label: string;
    description: string;
}

export interface AskUserQuestionItem {
    question: string;
    header: string;
    options: AskUserQuestionOption[];
    multiSelect: boolean;
}

export interface AskUserQuestionMetadata {
    source?: string;
}

export interface AskUserQuestionRequest {
    request_id: string;
    conversation_id?: number;
    questions: AskUserQuestionItem[];
    metadata?: AskUserQuestionMetadata;
}

export interface PreviewFileItem {
    title: string;
    type: "markdown" | "text" | "image" | "pdf" | "html" | string;
    content?: string;
    url?: string;
    language?: string;
    description?: string;
}

export interface PreviewFileMetadata {
    origin?: string;
}

export interface PreviewFileRequest {
    request_id: string;
    conversation_id?: number;
    files: PreviewFileItem[];
    viewMode?: "tabs" | "list" | "grid";
    metadata?: PreviewFileMetadata;
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

interface AskUserQuestionDialogProps {
    request: AskUserQuestionRequest | null;
    isOpen: boolean;
    onSubmit: (requestId: string, answers: Record<string, string>) => void;
    onCancel: (requestId: string) => void;
}

interface PreviewFileDialogProps {
    request: PreviewFileRequest | null;
    isOpen: boolean;
    onOpenChange: (open: boolean) => void;
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

const OTHER_OPTION_VALUE = "__other__";

export function AskUserQuestionDialog({
    request,
    isOpen,
    onSubmit,
    onCancel,
}: AskUserQuestionDialogProps) {
    const [selectedAnswers, setSelectedAnswers] = useState<Record<number, string[]>>({});
    const [otherInputs, setOtherInputs] = useState<Record<number, string>>({});

    useEffect(() => {
        if (!request) {
            setSelectedAnswers({});
            setOtherInputs({});
            return;
        }
        const next: Record<number, string[]> = {};
        request.questions.forEach((_, index) => {
            next[index] = [];
        });
        setSelectedAnswers(next);
        setOtherInputs({});
    }, [request]);

    const canSubmit = useMemo(() => {
        if (!request) return false;
        return request.questions.every((_, index) => {
            const values = selectedAnswers[index] ?? [];
            if (values.length === 0) return false;
            if (values.includes(OTHER_OPTION_VALUE)) {
                return Boolean((otherInputs[index] ?? "").trim());
            }
            return true;
        });
    }, [request, selectedAnswers, otherInputs]);

    const updateSingleSelect = (index: number, value: string) => {
        setSelectedAnswers((prev) => ({
            ...prev,
            [index]: [value],
        }));
    };

    const updateMultiSelect = (index: number, value: string, checked: boolean) => {
        setSelectedAnswers((prev) => {
            const current = prev[index] ?? [];
            const next = checked
                ? Array.from(new Set([...current, value]))
                : current.filter((item) => item !== value);
            return { ...prev, [index]: next };
        });
    };

    const handleSubmit = () => {
        if (!request) return;
        const answers: Record<string, string> = {};

        request.questions.forEach((question, index) => {
            const values = selectedAnswers[index] ?? [];
            const resolved = values
                .map((value) => {
                    if (value === OTHER_OPTION_VALUE) {
                        return (otherInputs[index] ?? "").trim();
                    }
                    return value;
                })
                .filter((value) => value.length > 0);
            answers[question.question] =
                question.multiSelect ? resolved.join(", ") : (resolved[0] ?? "");
        });

        onSubmit(request.request_id, answers);
    };

    if (!request) return null;

    return (
        <Dialog open={isOpen}>
            <DialogContent className="max-w-2xl" showCloseButton={false}>
                <DialogHeader>
                    <DialogTitle className="flex items-center gap-2">
                        <Shield className="h-5 w-5" />
                        用户问题确认
                    </DialogTitle>
                    <DialogDescription>
                        AI 助手需要你的选择来继续执行，请完成以下问题。
                    </DialogDescription>
                </DialogHeader>
                <ScrollArea className="max-h-[60vh] pr-3">
                    <div className="space-y-3">
                        {request.questions.map((question, index) => {
                            const selected = selectedAnswers[index] ?? [];
                            const includesOther = selected.includes(OTHER_OPTION_VALUE);
                            const options = [
                                ...question.options,
                                { label: OTHER_OPTION_VALUE, description: "自定义输入" },
                            ];
                            return (
                                <Card key={`${question.header}-${index}`} className="py-4 gap-3">
                                    <CardHeader className="px-4 gap-2">
                                        <Badge variant="outline">{question.header}</Badge>
                                        <CardTitle className="text-sm leading-6">
                                            {question.question}
                                        </CardTitle>
                                    </CardHeader>
                                    <CardContent className="px-4 space-y-3">
                                        {question.multiSelect ? (
                                            <div className="space-y-2">
                                                {options.map((option) => {
                                                    const optionValue = option.label;
                                                    const checked = selected.includes(optionValue);
                                                    const displayLabel =
                                                        optionValue === OTHER_OPTION_VALUE
                                                            ? "Other"
                                                            : option.label;
                                                    return (
                                                        <label
                                                            key={optionValue}
                                                            className="flex items-start gap-2 rounded-md border border-border px-3 py-2 cursor-pointer"
                                                        >
                                                            <Checkbox
                                                                checked={checked}
                                                                onCheckedChange={(value) =>
                                                                    updateMultiSelect(
                                                                        index,
                                                                        optionValue,
                                                                        value === true
                                                                    )
                                                                }
                                                                className="mt-0.5"
                                                            />
                                                            <div className="space-y-1 text-sm">
                                                                <div className="font-medium">{displayLabel}</div>
                                                                <div className="text-muted-foreground text-xs">
                                                                    {option.description}
                                                                </div>
                                                            </div>
                                                        </label>
                                                    );
                                                })}
                                            </div>
                                        ) : (
                                            <RadioGroup
                                                value={selected[0] ?? ""}
                                                onValueChange={(value) =>
                                                    updateSingleSelect(index, value)
                                                }
                                                className="space-y-2"
                                            >
                                                {options.map((option) => {
                                                    const optionValue = option.label;
                                                    const displayLabel =
                                                        optionValue === OTHER_OPTION_VALUE
                                                            ? "Other"
                                                            : option.label;
                                                    return (
                                                        <label
                                                            key={optionValue}
                                                            className="flex items-start gap-2 rounded-md border border-border px-3 py-2 cursor-pointer"
                                                        >
                                                            <RadioGroupItem
                                                                value={optionValue}
                                                                className="mt-1"
                                                            />
                                                            <div className="space-y-1 text-sm">
                                                                <div className="font-medium">{displayLabel}</div>
                                                                <div className="text-muted-foreground text-xs">
                                                                    {option.description}
                                                                </div>
                                                            </div>
                                                        </label>
                                                    );
                                                })}
                                            </RadioGroup>
                                        )}
                                        {includesOther && (
                                            <Input
                                                value={otherInputs[index] ?? ""}
                                                onChange={(event) =>
                                                    setOtherInputs((prev) => ({
                                                        ...prev,
                                                        [index]: event.target.value,
                                                    }))
                                                }
                                                placeholder="请输入自定义答案"
                                            />
                                        )}
                                    </CardContent>
                                </Card>
                            );
                        })}
                    </div>
                </ScrollArea>
                <DialogFooter>
                    <Button
                        variant="outline"
                        onClick={() => onCancel(request.request_id)}
                    >
                        取消
                    </Button>
                    <Button onClick={handleSubmit} disabled={!canSubmit}>
                        提交
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}

const fileTypeIcon = (type: string) => {
    switch (type) {
        case "image":
            return <ImageIcon className="h-4 w-4" />;
        default:
            return <FileText className="h-4 w-4" />;
    }
};

const resolveImageSrc = (file: PreviewFileItem): string | null => {
    if (file.url?.trim()) return file.url.trim();
    if (!file.content?.trim()) return null;
    if (file.content.startsWith("data:")) return file.content;
    return `data:image/png;base64,${file.content}`;
};

const resolvePdfSrc = (file: PreviewFileItem): string | null => {
    if (file.url?.trim()) return file.url.trim();
    if (!file.content?.trim()) return null;
    if (file.content.startsWith("data:")) return file.content;
    return `data:application/pdf;base64,${file.content}`;
};

const PreviewFileContent: React.FC<{ file: PreviewFileItem }> = ({ file }) => {
    if (file.type === "markdown") {
        return (
            <div className="prose prose-sm max-w-none dark:prose-invert">
                <UnifiedMarkdown noProseWrapper>{file.content ?? ""}</UnifiedMarkdown>
            </div>
        );
    }

    if (file.type === "text") {
        const text = file.content ?? file.url ?? "";
        if (file.language) {
            return (
                <UnifiedMarkdown noProseWrapper>{`\`\`\`${file.language}\n${text}\n\`\`\``}</UnifiedMarkdown>
            );
        }
        return (
            <pre className="text-xs font-mono whitespace-pre-wrap break-words rounded-md border border-border bg-muted p-3">
                {text}
            </pre>
        );
    }

    if (file.type === "image") {
        const src = resolveImageSrc(file);
        if (!src) {
            return <div className="text-sm text-muted-foreground">无法展示图片内容。</div>;
        }
        return (
            <div className="rounded-md border border-border bg-muted p-2">
                <img src={src} alt={file.title} className="max-h-[60vh] w-full object-contain" />
            </div>
        );
    }

    if (file.type === "pdf") {
        const src = resolvePdfSrc(file);
        if (!src) {
            return <div className="text-sm text-muted-foreground">无法展示 PDF 内容。</div>;
        }
        return (
            <iframe
                src={src}
                title={file.title}
                className="h-[60vh] w-full rounded-md border border-border bg-background"
            />
        );
    }

    if (file.type === "html") {
        if (file.url?.trim()) {
            return (
                <iframe
                    src={file.url}
                    title={file.title}
                    className="h-[60vh] w-full rounded-md border border-border bg-background"
                />
            );
        }
        return (
            <iframe
                srcDoc={file.content ?? ""}
                title={file.title}
                className="h-[60vh] w-full rounded-md border border-border bg-background"
            />
        );
    }

    return <div className="text-sm text-muted-foreground">不支持的文件类型：{file.type}</div>;
};

export function PreviewFileDialog({
    request,
    isOpen,
    onOpenChange,
}: PreviewFileDialogProps) {
    const [activeTab, setActiveTab] = useState("0");

    useEffect(() => {
        if (request?.files.length) {
            setActiveTab("0");
        }
    }, [request]);

    if (!request) return null;

    const viewMode = request.viewMode ?? "tabs";
    const files = request.files;

    return (
        <Dialog open={isOpen} onOpenChange={onOpenChange}>
            <DialogContent className="max-w-5xl">
                <DialogHeader>
                    <DialogTitle className="flex items-center gap-2">
                        <FileText className="h-5 w-5" />
                        文件预览
                    </DialogTitle>
                    <DialogDescription>
                        由 PreviewFile 工具生成的只读文件展示组件。
                    </DialogDescription>
                </DialogHeader>
                <div className="space-y-3">
                    {viewMode === "tabs" ? (
                        <Tabs value={activeTab} onValueChange={setActiveTab} className="w-full">
                            <TabsList className="w-full justify-start overflow-x-auto h-auto min-h-9">
                                {files.map((file, index) => (
                                    <TabsTrigger
                                        key={`${file.title}-${index}`}
                                        value={`${index}`}
                                        className="flex items-center gap-1"
                                    >
                                        {fileTypeIcon(file.type)}
                                        <span className="max-w-[180px] truncate">{file.title}</span>
                                    </TabsTrigger>
                                ))}
                            </TabsList>
                            {files.map((file, index) => (
                                <TabsContent key={`${file.title}-content-${index}`} value={`${index}`}>
                                    <div className="rounded-md border border-border p-3 space-y-2">
                                        <div className="flex items-center gap-2">
                                            <Badge variant="outline">{file.type}</Badge>
                                            <span className="text-sm font-medium">{file.title}</span>
                                        </div>
                                        {file.description && (
                                            <div className="text-xs text-muted-foreground">
                                                {file.description}
                                            </div>
                                        )}
                                        <PreviewFileContent file={file} />
                                    </div>
                                </TabsContent>
                            ))}
                        </Tabs>
                    ) : (
                        <div
                            className={
                                viewMode === "grid"
                                    ? "grid grid-cols-1 lg:grid-cols-2 gap-3"
                                    : "space-y-3"
                            }
                        >
                            {files.map((file, index) => (
                                <div
                                    key={`${file.title}-${index}`}
                                    className="rounded-md border border-border p-3 space-y-2"
                                >
                                    <div className="flex items-center gap-2">
                                        <Badge variant="outline">{file.type}</Badge>
                                        <span className="text-sm font-medium">{file.title}</span>
                                    </div>
                                    {file.description && (
                                        <div className="text-xs text-muted-foreground">
                                            {file.description}
                                        </div>
                                    )}
                                    <PreviewFileContent file={file} />
                                </div>
                            ))}
                        </div>
                    )}
                    {request.metadata?.origin && (
                        <div className="text-xs text-muted-foreground flex items-center gap-1">
                            <ShieldCheck className="h-3 w-3" />
                            来源: {request.metadata.origin}
                        </div>
                    )}
                </div>
                <DialogFooter>
                    <Button variant="outline" onClick={() => onOpenChange(false)}>
                        关闭
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
