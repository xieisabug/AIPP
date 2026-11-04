import { useState, useMemo, useEffect } from "react";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import UnifiedMarkdown from "@/components/UnifiedMarkdown";
import { FileText, ImageIcon, Shield, ShieldCheck } from "lucide-react";
import PreviewExternalResourcesDialog from "@/components/PreviewExternalResourcesDialog";
import {
    hasActionablePreviewResources,
    PreviewExternalResourcesPayload,
    PreviewResourceAuthorizationResult,
} from "@/utils/previewExternalResources";

export interface AskUserQuestionOption {
    label: string;
    description: string;
}

export interface AskUserQuestionItem {
    question: string;
    header: string;
    options: AskUserQuestionOption[];
    multiSelect: boolean;
    isSecret?: boolean;
    /**
     * 结构化来源（如 ACP elicitation）的稳定答案键。
     * 缺省时答案以 question 文本为键，保持 ask_user_question 既有行为。
     */
    answerKey?: string;
    /** 结构化值类型，缺省为 string；仅用于提交方的类型化转换，卡片本身仍按文本采集 */
    valueType?: "string" | "number" | "integer" | "boolean" | "stringArray";
    /** option label → 原始值的映射（如 oneOf 的 title → const 值），缺省时 label 即值 */
    optionValues?: Record<string, string>;
    /** 是否必填，缺省 true（保持既有行为：所有问题都必须回答） */
    required?: boolean;
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
    externalResources?: PreviewExternalResourcesPayload;
}

interface AskUserQuestionCardProps {
    request: AskUserQuestionRequest | null;
    isOpen: boolean;
    viewMode?: "questionnaire" | "summary";
    completedAnswers?: Record<string, string> | null;
    readOnly?: boolean;
    /** 提交方（如 ACP elicitation 类型化转换）反馈的错误信息，展示在操作行上方 */
    errorMessage?: string | null;
    onSubmit: (requestId: string, answers: Record<string, string>) => void;
    onCancel: (requestId: string) => void;
}

interface PreviewFileCardProps {
    request: PreviewFileRequest | null;
    isOpen: boolean;
    onOpenChange: (open: boolean) => void;
}

const OTHER_OPTION_VALUE = "__other__";

export function AskUserQuestionCard({
    request,
    isOpen,
    viewMode = "questionnaire",
    completedAnswers = null,
    readOnly = false,
    errorMessage: errorMessageProp = null,
    onSubmit,
    onCancel,
}: AskUserQuestionCardProps) {
    const [selectedAnswers, setSelectedAnswers] = useState<Record<number, string[]>>({});
    const [otherInputs, setOtherInputs] = useState<Record<number, string>>({});
    const [activeTab, setActiveTab] = useState("0");
    const errorMessage = errorMessageProp ?? null;

    useEffect(() => {
        if (!request || !isOpen) {
            setSelectedAnswers({});
            setOtherInputs({});
            setActiveTab("0");
            return;
        }
        const next: Record<number, string[]> = {};
        request.questions.forEach((_, index) => {
            next[index] = [];
        });
        setSelectedAnswers(next);
        setOtherInputs({});
        setActiveTab("0");
    }, [request, isOpen]);

    const hasAnswerValue = (index: number) => {
        if (request?.questions[index]?.options.length === 0) {
            return Boolean((otherInputs[index] ?? "").trim());
        }
        const values = selectedAnswers[index] ?? [];
        if (values.length === 0) return false;
        if (values.includes(OTHER_OPTION_VALUE)) {
            return Boolean((otherInputs[index] ?? "").trim());
        }
        return true;
    };

    const isQuestionAnswered = (index: number) => {
        // 选填问题（required === false）允许留空，其余问题维持原有必填行为
        if (request?.questions[index]?.required === false) return true;
        return hasAnswerValue(index);
    };

    const canSubmit = useMemo(() => {
        if (!request) return false;
        return request.questions.every((_, index) => isQuestionAnswered(index));
    }, [request, selectedAnswers, otherInputs]);

    const canGoNext = useMemo(() => {
        if (!request) return false;
        const currentIndex = Number.parseInt(activeTab, 10) || 0;
        return isQuestionAnswered(currentIndex);
    }, [request, activeTab, selectedAnswers, otherInputs]);

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

    const handleNext = () => {
        if (!request) return;
        const currentIndex = Number.parseInt(activeTab, 10) || 0;
        if (currentIndex < request.questions.length - 1) {
            setActiveTab(`${currentIndex + 1}`);
        }
    };

    const handlePrevious = () => {
        const currentIndex = Number.parseInt(activeTab, 10) || 0;
        if (currentIndex > 0) {
            setActiveTab(`${currentIndex - 1}`);
        }
    };

    const handleSubmit = () => {
        if (!request) return;
        const answers: Record<string, string> = {};

        request.questions.forEach((question, index) => {
            // 选填且未作答的问题不上报，由提交方决定语义（如 ACP elicitation 直接省略该字段）
            if (question.required === false && !hasAnswerValue(index)) return;
            const answerKey = question.answerKey ?? question.question;
            if (question.options.length === 0) {
                answers[answerKey] = (otherInputs[index] ?? "").trim();
                return;
            }
            const values = selectedAnswers[index] ?? [];
            const resolved = values
                .map((value) => {
                    if (value === OTHER_OPTION_VALUE) {
                        return (otherInputs[index] ?? "").trim();
                    }
                    return value;
                })
                .filter((value) => value.length > 0);
            answers[answerKey] =
                question.multiSelect ? resolved.join(", ") : (resolved[0] ?? "");
        });

        onSubmit(request.request_id, answers);
    };

    if (!request || !isOpen) return null;

    const activeQuestionIndex = Number.parseInt(activeTab, 10) || 0;
    const isLastQuestion = activeQuestionIndex >= request.questions.length - 1;
    const summaryAnswers = completedAnswers ?? {};
    const hasMultipleQuestions = request.questions.length > 1;

    const renderQuestionContent = (question: AskUserQuestionItem, index: number) => {
        const isFreeform = question.options.length === 0;
        const selected = selectedAnswers[index] ?? [];
        const includesOther = selected.includes(OTHER_OPTION_VALUE);
        const options = [
            ...question.options,
            { label: OTHER_OPTION_VALUE, description: "自定义输入" },
        ];

        return (
            <div className="rounded-md border border-border p-3 space-y-3">
                <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2">
                        <Badge variant="outline">{question.header}</Badge>
                        {question.required === false && (
                            <span className="text-xs text-muted-foreground">选填</span>
                        )}
                    </div>
                    {hasMultipleQuestions && (
                        <span className="text-xs text-muted-foreground">
                            {index + 1} / {request.questions.length}
                        </span>
                    )}
                </div>
                <div className="text-sm leading-6 font-medium">
                    {question.question}
                </div>
                <div className="max-h-[30vh] pr-2 overflow-y-auto">
                    {isFreeform ? (
                        <Input
                            type={question.isSecret ? "password" : "text"}
                            value={otherInputs[index] ?? ""}
                            disabled={readOnly}
                            onChange={(event) =>
                                setOtherInputs((prev) => ({
                                    ...prev,
                                    [index]: event.target.value,
                                }))
                            }
                            placeholder="请输入答案"
                        />
                    ) : question.multiSelect ? (
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
                                            disabled={readOnly}
                                            onCheckedChange={(value) =>
                                                !readOnly &&
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
                                !readOnly && updateSingleSelect(index, value)
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
                                            disabled={readOnly}
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
                </div>
                {includesOther && (
                    <Input
                        value={otherInputs[index] ?? ""}
                        disabled={readOnly}
                        onChange={(event) =>
                            setOtherInputs((prev) => ({
                                ...prev,
                                [index]: event.target.value,
                            }))
                        }
                        placeholder="请输入自定义答案"
                    />
                )}
            </div>
        );
    };

    if (viewMode === "summary") {
        return (
            <div
                className="w-full flex justify-center"
                data-message-item
                data-message-type="ui_interaction_ask_user_question"
                data-request-id={request.request_id}
            >
                <Card className="w-full max-w-3xl py-4 gap-3">
                    <CardHeader className="px-4 pb-0 gap-2">
                        <CardTitle className="flex items-center gap-2 text-base">
                            <ShieldCheck className="h-5 w-5" />
                            问题回答结果
                        </CardTitle>
                        <div className="text-sm text-muted-foreground">
                            以下是本次 ask_user_question 的问题与回答。
                        </div>
                    </CardHeader>
                    <CardContent className="px-4 space-y-3">
                        {request.questions.map((question, index) => (
                            <div
                                key={`${question.header}-answer-${index}`}
                                className="rounded-md border border-border p-3 space-y-2"
                            >
                                <div className="flex items-center gap-2">
                                    <Badge variant="outline">{question.header}</Badge>
                                </div>
                                <div className="text-sm font-medium">{question.question}</div>
                                <div className="text-sm text-muted-foreground whitespace-pre-wrap break-words">
                                    {summaryAnswers[question.question] || "（无回答）"}
                                </div>
                            </div>
                        ))}
                    </CardContent>
                </Card>
            </div>
        );
    }

    return (
        <div
            className="w-full flex justify-center"
            data-message-item
            data-message-type="ui_interaction_ask_user_question"
            data-request-id={request.request_id}
        >
            <Card className="w-full max-w-3xl py-4 gap-3">
                <CardHeader className="px-4 pb-0 gap-2">
                    <CardTitle className="flex items-center gap-2 text-base">
                        <Shield className="h-5 w-5" />
                        用户问题确认
                    </CardTitle>
                    <div className="text-sm text-muted-foreground">
                        AI 助手需要你的选择来继续执行，请完成以下问题。
                    </div>
                </CardHeader>
                <CardContent className="px-4 space-y-4">
                    {hasMultipleQuestions ? (
                        <Tabs
                            value={activeTab}
                            onValueChange={(value) => {
                                if (!readOnly) {
                                    setActiveTab(value);
                                }
                            }}
                            className="w-full space-y-3"
                        >
                            <TabsList className="w-full justify-start overflow-x-auto h-auto min-h-9">
                                {request.questions.map((question, index) => (
                                    <TabsTrigger
                                        key={`${question.header}-${index}`}
                                        value={`${index}`}
                                        className="min-w-[96px]"
                                        disabled={readOnly}
                                    >
                                        {question.header}
                                    </TabsTrigger>
                                ))}
                            </TabsList>
                            {request.questions.map((question, index) => (
                                <TabsContent
                                    key={`${question.header}-content-${index}`}
                                    value={`${index}`}
                                    className="mt-0"
                                >
                                    {renderQuestionContent(question, index)}
                                </TabsContent>
                            ))}
                        </Tabs>
                    ) : (
                        request.questions[0]
                            ? renderQuestionContent(request.questions[0], 0)
                            : (
                                <div className="rounded-md border border-border p-3 text-sm text-muted-foreground">
                                    暂无可回答的问题。
                                </div>
                            )
                    )}
                    {errorMessage && (
                        <div className="text-sm text-destructive">{errorMessage}</div>
                    )}
                    <div className="flex items-center justify-between border-t border-border pt-3">
                        {readOnly ? <div /> : (
                            <Button variant="outline" onClick={() => onCancel(request.request_id)}>
                                取消
                            </Button>
                        )}
                        <div className="flex items-center gap-2">
                            {!readOnly && request.questions.length > 1 && activeQuestionIndex > 0 && (
                                <Button variant="outline" onClick={handlePrevious}>
                                    上一步
                                </Button>
                            )}
                            {readOnly ? (
                                <Badge variant="outline">执行中</Badge>
                            ) : (
                                !isLastQuestion ? (
                                    <Button onClick={handleNext} disabled={!canGoNext}>
                                        下一步
                                    </Button>
                                ) : (
                                    <Button onClick={handleSubmit} disabled={!canSubmit}>
                                        提交
                                    </Button>
                                )
                            )}
                        </div>
                    </div>
                </CardContent>
            </Card>
        </div>
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

export function PreviewFileCard({
    request,
    isOpen,
    onOpenChange,
}: PreviewFileCardProps) {
    const [activeTab, setActiveTab] = useState("0");
    const [currentRequest, setCurrentRequest] = useState<PreviewFileRequest | null>(request);
    const [isResourceDialogOpen, setIsResourceDialogOpen] = useState(false);

    useEffect(() => {
        if (request?.files.length) {
            setActiveTab("0");
        }
        setCurrentRequest(request);
    }, [request]);

    if (!currentRequest || !isOpen) return null;

    const viewMode = currentRequest.viewMode ?? "tabs";
    const files = currentRequest.files;
    const useTabs = viewMode === "tabs" && files.length > 1;
    const showExternalResourceButton =
        hasActionablePreviewResources(currentRequest.externalResources);

    const renderFileContent = (file: PreviewFileItem, index: number) => (
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
    );

    return (
        <div
            className="w-full flex justify-center"
            data-message-item
            data-message-type="ui_interaction_preview_file"
            data-request-id={currentRequest.request_id}
        >
            <Card className="w-full max-w-5xl py-4 gap-3">
                <CardHeader className="px-4 pb-0 gap-2">
                    <div className="flex items-start justify-between gap-3">
                        <div className="space-y-2">
                            <CardTitle className="flex items-center gap-2 text-base">
                                <FileText className="h-5 w-5" />
                                文件预览
                            </CardTitle>
                            <div className="text-sm text-muted-foreground">
                                由 PreviewFile 工具生成的只读文件展示组件。
                            </div>
                        </div>
                        {showExternalResourceButton && (
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                onClick={() => setIsResourceDialogOpen(true)}
                            >
                                需要加载外部资源
                            </Button>
                        )}
                    </div>
                </CardHeader>
                <CardContent className="px-4 space-y-3">
                    {useTabs ? (
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
                                    {renderFileContent(file, index)}
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
                            {files.length > 0 ? files.map((file, index) => renderFileContent(file, index)) : (
                                <div className="rounded-md border border-border p-3 text-sm text-muted-foreground">
                                    暂无可预览文件。
                                </div>
                            )}
                        </div>
                    )}
                    {currentRequest.metadata?.origin && (
                        <div className="text-xs text-muted-foreground flex items-center gap-1">
                            <ShieldCheck className="h-3 w-3" />
                            来源: {currentRequest.metadata.origin}
                        </div>
                    )}
                    <div className="flex justify-end">
                        <Button variant="outline" onClick={() => onOpenChange(false)}>
                            关闭
                        </Button>
                    </div>
                </CardContent>
            </Card>
            <PreviewExternalResourcesDialog<unknown, PreviewFileRequest>
                externalResources={currentRequest.externalResources}
                open={isResourceDialogOpen}
                onOpenChange={setIsResourceDialogOpen}
                onAuthorized={(result: PreviewResourceAuthorizationResult<unknown, PreviewFileRequest>) => {
                    if (result.previewFile) {
                        setCurrentRequest(result.previewFile);
                    }
                }}
            />
        </div>
    );
}
