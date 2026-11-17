import { useState, useMemo, useEffect, useId } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Button } from "@/components/ui/button";
import UnifiedMarkdown from "@/components/UnifiedMarkdown";
import { cn } from "@/utils/utils";
import { Check, ChevronDown, CircleCheck, FileText, Files, Globe, ImageIcon, MessageCircleQuestion } from "lucide-react";
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
    message_id?: number | null;
    call_id?: number | null;
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
    message_id?: number | null;
    call_id?: number | null;
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
    /** 已不再调用：卡片只在内部收起/展开，不会自行关闭。保留以兼容现有调用方。 */
    onOpenChange?: (open: boolean) => void;
}

const OTHER_OPTION_VALUE = "__other__";

/**
 * 交互卡片统一外观：细边框 + 紧凑标题行，与引用消息卡片（MessagePreviewToolCall）一致，只用主题语义色。
 * 卡片会渲染在消息气泡的 `.prose` 里，typography 与 App.css 按标签名给 p/pre/img/figure/dl 加外边距，
 * 所以卡片骨架只用 div/section 等不受影响的标签，正文 markdown 才交给 prose。
 */
const CARD_CLASS = "w-full max-w-3xl overflow-hidden rounded-lg border border-border bg-card text-card-foreground";
const CARD_HEADER_CLASS = "flex min-w-0 items-center gap-2 border-b border-border/60 px-3 py-2 text-xs text-muted-foreground";
const CARD_FOOTER_CLASS = "flex items-center gap-2 border-t border-border/60 px-3 py-2";
const OPTION_ROW_CLASS = "flex items-start gap-2.5 rounded-md px-2.5 py-2 text-sm transition-colors has-data-[state=checked]:bg-muted";
const PILL_CLASS = "inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-2 text-xs transition-colors";
const pillState = (active: boolean) =>
    active ? "bg-muted font-medium text-foreground" : "text-muted-foreground hover:text-foreground";

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

    const renderOptionText = (label: string, description: string) => (
        <div className="min-w-0 space-y-0.5">
            <div className="font-medium leading-5 text-foreground">{label}</div>
            {description && (
                <div className="text-xs leading-5 text-muted-foreground">{description}</div>
            )}
        </div>
    );

    const optionRowClass = cn(OPTION_ROW_CLASS, readOnly ? "cursor-default" : "cursor-pointer hover:bg-muted/60");

    const renderQuestionContent = (question: AskUserQuestionItem, index: number) => {
        const isFreeform = question.options.length === 0;
        const selected = selectedAnswers[index] ?? [];
        const includesOther = selected.includes(OTHER_OPTION_VALUE);
        const options = [
            ...question.options,
            { label: OTHER_OPTION_VALUE, description: "" },
        ];
        const displayLabel = (label: string) => (label === OTHER_OPTION_VALUE ? "其他" : label);

        return (
            <div className="space-y-2.5">
                <div className="flex items-baseline gap-2">
                    <div className="text-sm font-medium leading-6 text-foreground">{question.question}</div>
                    {question.required === false && (
                        <span className="shrink-0 text-xs text-muted-foreground">选填</span>
                    )}
                </div>
                <div className="-mx-1 max-h-[30vh] overflow-y-auto px-1">
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
                        <div className="space-y-0.5">
                            {options.map((option) => {
                                const optionValue = option.label;
                                return (
                                    <label key={optionValue} className={optionRowClass}>
                                        <Checkbox
                                            checked={selected.includes(optionValue)}
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
                                        {renderOptionText(displayLabel(optionValue), option.description)}
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
                            className="gap-0.5"
                        >
                            {options.map((option) => {
                                const optionValue = option.label;
                                return (
                                    <label key={optionValue} className={optionRowClass}>
                                        <RadioGroupItem
                                            value={optionValue}
                                            className="mt-0.5"
                                            disabled={readOnly}
                                        />
                                        {renderOptionText(displayLabel(optionValue), option.description)}
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
                        autoFocus={!readOnly}
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
                <section className={CARD_CLASS}>
                    <header className={CARD_HEADER_CLASS}>
                        <CircleCheck className="h-3.5 w-3.5 shrink-0" />
                        <span className="font-medium text-foreground">已回答</span>
                        {hasMultipleQuestions && (
                            <span className="ml-auto shrink-0 tabular-nums">{request.questions.length} 个问题</span>
                        )}
                    </header>
                    <div className="divide-y divide-border/60">
                        {request.questions.map((question, index) => (
                            <div key={`${question.header}-answer-${index}`} className="space-y-0.5 px-3 py-2">
                                <div className="text-xs leading-5 text-muted-foreground">{question.question}</div>
                                <div className="text-sm leading-6 text-foreground whitespace-pre-wrap break-words">
                                    {summaryAnswers[question.question] || (
                                        <span className="text-muted-foreground">（无回答）</span>
                                    )}
                                </div>
                            </div>
                        ))}
                    </div>
                </section>
            </div>
        );
    }

    const activeQuestion = request.questions[activeQuestionIndex];

    return (
        <div
            className="w-full flex justify-center"
            data-message-item
            data-message-type="ui_interaction_ask_user_question"
            data-request-id={request.request_id}
        >
            <section className={CARD_CLASS}>
                <header className={CARD_HEADER_CLASS}>
                    <MessageCircleQuestion className="h-3.5 w-3.5 shrink-0" />
                    {hasMultipleQuestions ? (
                        <nav className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto" aria-label="问题列表">
                            {request.questions.map((question, index) => {
                                const active = index === activeQuestionIndex;
                                return (
                                    <button
                                        key={`${question.header}-${index}`}
                                        type="button"
                                        className={cn(PILL_CLASS, pillState(active))}
                                        aria-current={active ? "step" : undefined}
                                        onClick={() => setActiveTab(`${index}`)}
                                    >
                                        {!active && hasAnswerValue(index) && <Check className="h-3 w-3" />}
                                        <span className="max-w-[140px] truncate">{question.header}</span>
                                    </button>
                                );
                            })}
                        </nav>
                    ) : (
                        <span className="min-w-0 truncate font-medium text-foreground">
                            {activeQuestion?.header || "需要你的确认"}
                        </span>
                    )}
                    {hasMultipleQuestions && (
                        <span className="ml-auto shrink-0 tabular-nums">
                            {activeQuestionIndex + 1} / {request.questions.length}
                        </span>
                    )}
                </header>
                <div className="px-3 py-3">
                    {activeQuestion
                        ? renderQuestionContent(activeQuestion, activeQuestionIndex)
                        : <div className="text-sm text-muted-foreground">暂无可回答的问题。</div>}
                    {errorMessage && (
                        <div className="mt-2 text-xs text-destructive">{errorMessage}</div>
                    )}
                </div>
                <footer className={CARD_FOOTER_CLASS}>
                    {readOnly ? (
                        <span className="ml-auto text-xs text-muted-foreground">已提交，执行中</span>
                    ) : (
                        <>
                            <Button
                                variant="ghost"
                                size="sm"
                                className="mr-auto text-muted-foreground"
                                onClick={() => onCancel(request.request_id)}
                            >
                                取消
                            </Button>
                            {hasMultipleQuestions && activeQuestionIndex > 0 && (
                                <Button variant="ghost" size="sm" onClick={handlePrevious}>
                                    上一步
                                </Button>
                            )}
                            {!isLastQuestion ? (
                                <Button size="sm" onClick={handleNext} disabled={!canGoNext}>
                                    下一步
                                </Button>
                            ) : (
                                <Button size="sm" onClick={handleSubmit} disabled={!canSubmit}>
                                    提交
                                </Button>
                            )}
                        </>
                    )}
                </footer>
            </section>
        </div>
    );
}

const fileTypeIcon = (type: string) => {
    switch (type) {
        case "image":
            return <ImageIcon className="h-3.5 w-3.5 shrink-0" />;
        default:
            return <FileText className="h-3.5 w-3.5 shrink-0" />;
    }
};

const resolveImageSrc = (file: PreviewFileItem): string | null => {
    const url = file.url?.trim();
    if (url) {
        // WebView2 serves custom protocols through a virtual localhost origin.
        const relay = /^aipp-preview:\/\/localhost\/([\w-]+)$/.exec(url);
        return relay ? convertFileSrc(relay[1], "aipp-preview") : url;
    }
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

const FRAME_CLASS = "block h-[60vh] w-full bg-background";

/** 文件内容贴边铺满卡片，不再套额外边框；文本类内容自带内边距。 */
const PreviewFileContent: React.FC<{ file: PreviewFileItem }> = ({ file }) => {
    if (file.type === "markdown") {
        return (
            <div className="prose prose-sm max-w-none px-3 py-2 dark:prose-invert [&>:first-child]:mt-0! [&>:last-child]:mb-0!">
                <UnifiedMarkdown noProseWrapper>{file.content ?? ""}</UnifiedMarkdown>
            </div>
        );
    }

    if (file.type === "text") {
        const text = file.content ?? file.url ?? "";
        if (file.language) {
            return (
                <div className="px-3 py-2">
                    <UnifiedMarkdown noProseWrapper>{`\`\`\`${file.language}\n${text}\n\`\`\``}</UnifiedMarkdown>
                </div>
            );
        }
        return (
            <div className="bg-muted/30 px-3 py-2 text-xs leading-5 font-mono whitespace-pre-wrap break-words">
                {text}
            </div>
        );
    }

    if (file.type === "image") {
        const src = resolveImageSrc(file);
        if (!src) {
            return <div className="px-3 py-2 text-sm text-muted-foreground">无法展示图片内容。</div>;
        }
        return (
            <div className="flex justify-center bg-muted/30 p-2">
                <img src={src} alt={file.title} className="my-0! max-h-[60vh] max-w-full rounded-md object-contain" />
            </div>
        );
    }

    if (file.type === "pdf") {
        const src = resolvePdfSrc(file);
        if (!src) {
            return <div className="px-3 py-2 text-sm text-muted-foreground">无法展示 PDF 内容。</div>;
        }
        return <iframe src={src} title={file.title} className={FRAME_CLASS} />;
    }

    if (file.type === "html") {
        if (file.url?.trim()) {
            return <iframe src={file.url} title={file.title} className={FRAME_CLASS} />;
        }
        return <iframe srcDoc={file.content ?? ""} title={file.title} className={FRAME_CLASS} />;
    }

    return <div className="px-3 py-2 text-sm text-muted-foreground">不支持的文件类型：{file.type}</div>;
};

const FileDescription: React.FC<{ text?: string }> = ({ text }) =>
    text ? <div className="px-3 pt-2 text-xs leading-5 text-muted-foreground">{text}</div> : null;

export function PreviewFileCard({
    request,
    isOpen,
}: PreviewFileCardProps) {
    const [activeTab, setActiveTab] = useState(0);
    const [collapsed, setCollapsed] = useState(false);
    const [currentRequest, setCurrentRequest] = useState<PreviewFileRequest | null>(request);
    const [isResourceDialogOpen, setIsResourceDialogOpen] = useState(false);
    const bodyId = useId();

    useEffect(() => {
        if (request?.files.length) {
            setActiveTab(0);
        }
        setCollapsed(false);
        setCurrentRequest(request);
    }, [request]);

    if (!currentRequest || !isOpen) return null;

    const viewMode = currentRequest.viewMode ?? "tabs";
    const files = currentRequest.files;
    const useTabs = viewMode === "tabs" && files.length > 1;
    const activeIndex = Math.min(activeTab, Math.max(files.length - 1, 0));
    const activeFile = useTabs ? files[activeIndex] : undefined;
    const showExternalResourceButton =
        hasActionablePreviewResources(currentRequest.externalResources);
    const origin = currentRequest.metadata?.origin;

    const renderHeading = () => {
        if (useTabs) {
            return (
                <nav className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto" aria-label="文件列表">
                    {files.map((file, index) => (
                        <button
                            key={`${file.title}-${index}`}
                            type="button"
                            className={cn(PILL_CLASS, pillState(index === activeIndex))}
                            aria-current={index === activeIndex ? "page" : undefined}
                            onClick={() => {
                                setActiveTab(index);
                                setCollapsed(false);
                            }}
                        >
                            {fileTypeIcon(file.type)}
                            <span className="max-w-[180px] truncate">{file.title}</span>
                        </button>
                    ))}
                </nav>
            );
        }
        if (files.length === 1) {
            return (
                <>
                    {fileTypeIcon(files[0].type)}
                    <span className="min-w-0 truncate font-medium text-foreground">{files[0].title}</span>
                </>
            );
        }
        return (
            <>
                <Files className="h-3.5 w-3.5 shrink-0" />
                <span className="font-medium text-foreground">
                    {files.length > 0 ? `${files.length} 个文件` : "文件预览"}
                </span>
            </>
        );
    };

    const renderBody = () => {
        if (activeFile) {
            return (
                <>
                    <FileDescription text={activeFile.description} />
                    <PreviewFileContent file={activeFile} />
                </>
            );
        }
        if (files.length === 0) {
            return <div className="px-3 py-3 text-sm text-muted-foreground">暂无可预览文件。</div>;
        }
        if (files.length === 1) {
            return (
                <>
                    <FileDescription text={files[0].description} />
                    <PreviewFileContent file={files[0]} />
                </>
            );
        }
        // list / grid：每个文件一个小节，用细分隔线区分，标题行与卡片标题同一层级感。
        return (
            <div
                className={cn(
                    "divide-y divide-border/60",
                    viewMode === "grid" && "lg:grid lg:grid-cols-2 lg:gap-px lg:divide-y-0 lg:bg-border/60",
                )}
            >
                {files.map((file, index) => (
                    <section key={`${file.title}-${index}`} className="min-w-0 bg-card">
                        <div className="flex min-w-0 items-center gap-1.5 px-3 pt-2 text-xs text-muted-foreground">
                            {fileTypeIcon(file.type)}
                            <span className="truncate font-medium text-foreground">{file.title}</span>
                        </div>
                        <FileDescription text={file.description} />
                        <div className="pt-1">
                            <PreviewFileContent file={file} />
                        </div>
                    </section>
                ))}
            </div>
        );
    };

    return (
        <div
            className="w-full flex justify-center"
            data-message-item
            data-message-type="ui_interaction_preview_file"
            data-request-id={currentRequest.request_id}
        >
            <div className={cn(CARD_CLASS, "max-w-5xl")}>
                <div className={cn(CARD_HEADER_CLASS, collapsed && "border-b-0")}>
                    {renderHeading()}
                    {origin && (
                        <span className="hidden min-w-0 max-w-[40%] truncate sm:inline" title={`来源：${origin}`}>
                            {origin}
                        </span>
                    )}
                    <div className="ml-auto flex shrink-0 items-center gap-0.5">
                        {showExternalResourceButton && (
                            <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                className="h-6 gap-1 px-2 text-xs has-[>svg]:px-2"
                                onClick={() => setIsResourceDialogOpen(true)}
                            >
                                <Globe className="size-3.5" />
                                加载外部资源
                            </Button>
                        )}
                        <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="size-6 text-muted-foreground"
                            aria-label={collapsed ? "展开预览" : "收起预览"}
                            title={collapsed ? "展开预览" : "收起预览"}
                            aria-expanded={!collapsed}
                            aria-controls={bodyId}
                            onClick={() => setCollapsed((value) => !value)}
                        >
                            <ChevronDown className={cn("size-3.5 transition-transform", !collapsed && "rotate-180")} />
                        </Button>
                    </div>
                </div>
                <div id={bodyId} hidden={collapsed}>
                    {renderBody()}
                </div>
            </div>
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
