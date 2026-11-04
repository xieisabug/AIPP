import React, { useState, useEffect, useRef, useMemo } from "react";
import { Control, Controller, SubmitHandler, UseFormReturn } from "react-hook-form";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { Copy, Trash2, CircleHelp, ChevronDown, ChevronRight, Edit3, MoreHorizontal } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./ui/card";
import { Form, FormControl, FormItem, FormLabel, FormMessage } from "./ui/form";
import { Button } from "./ui/button";
import { RadioGroup, RadioGroupItem } from "./ui/radio-group";
import { Input } from "./ui/input";
import { Checkbox } from "./ui/checkbox";
import { Switch } from "./ui/switch";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "./ui/tooltip";
import { useModels } from "../hooks/useModels";
import { useFilteredModels } from "../hooks/useFilteredModels";
import { useFilteredProviders } from "../hooks/useFilteredProviders";
import { useMcpServers } from "../hooks/useMcpServers";
import { FolderPicker } from "./config/FolderPicker";
import ExpandableTextareaField from "./config/ExpandableTextareaField";
import { PasswordInput } from "./ui/password-input";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./ui/dropdown-menu";
import { useIsMobilePlatform } from "../hooks/use-platform";

interface ConfigFieldButton {
    text: string;
    onClick: () => void;
    disabled?: boolean;
    variant?: "default" | "outline" | "ghost" | "destructive";
    className?: string;
}

interface ConfigField {
    type:
    | "select"
    | "textarea"
    | "input"
    | "folder-picker"
    | "password"
    | "checkbox"
    | "radio"
    | "static"
    | "custom"
    | "button"
    | "switch"
    | "model-select"
    | "provider-select"
    | "mcp-select"
    | "inline-buttons";
    label: string;
    className?: string;
    options?: { value: string; label: string; tooltip?: string }[];
    value?: string | boolean;
    tooltip?: string;
    placeholder?: string;
    onChange?: (value: string | boolean) => void;
    onBlur?: (value: string | boolean) => void;
    customRender?: (fieldRenderData: any) => React.ReactNode;
    onClick?: (assistantConfigApi?: AssistantConfigApi) => void;
    disabled?: boolean;
    hidden?: boolean;
    buttons?: ConfigFieldButton[];
    hint?: React.ReactNode;
    /** input / password 字段的虚拟键盘类型，例如 URL 字段用 "url"。 */
    inputMode?: React.HTMLAttributes<HTMLInputElement>["inputMode"];
}

interface ConfigFormProps {
    title: string;
    description?: string;
    config: Array<{ key: string; config: ConfigField }>;
    classNames?: string;
    enableExpand?: boolean;
    defaultExpanded?: boolean;
    useFormReturn: UseFormReturn<any, any, any>;
    /** Caller-computed dirty state; when omitted falls back to RHF formState.isDirty. */
    isDirty?: boolean;
    assistantConfigApi?: AssistantConfigApi;
    layout?: "default" | "prompt" | "provider";
    assistantType?: number | null;  // 用于过滤模型（ACP 助手 vs 普通助手）
    onSave?: SubmitHandler<any>;
    onCopy?: () => void;
    onDelete?: () => void;
    onEdit?: () => void;
    onTitleClick?: () => void;
    extraButtons?: React.ReactNode;
}

interface FieldOptionsData {
    models: any[] | undefined;
    modelsLoading: boolean;
    modelsError: unknown;
    providers: any[] | undefined;
    providersLoading: boolean;
    providersError: unknown;
    mcpServers: any[] | undefined;
    mcpServersLoading: boolean;
    mcpServersError: unknown;
}

interface ConfigFormFieldProps {
    field: ConfigField;
    name: string;
    control: Control<any, any, any>;
    data: FieldOptionsData;
    assistantConfigApi?: AssistantConfigApi;
}

/** One form row. Declared at module level so its identity is stable across renders. */
const ConfigFormField = ({ field, name, control, data, assistantConfigApi }: ConfigFormFieldProps) => {
    // 触屏上 Tooltip 无法打开：移动平台把说明文字直接显示为提示行
    const isMobilePlatform = useIsMobilePlatform();
    const {
        models, modelsLoading, modelsError,
        providers, providersLoading, providersError,
        mcpServers, mcpServersLoading, mcpServersError,
    } = data;
    // 生成模型选项用于 model-select 类型
    const modelOptions = useMemo(() => {
        if (field.type === "model-select" && models) {
            return models.map((model) => ({
                value: `${model.code}%%${model.llm_provider_id}`,
                label: model.name,
            }));
        }
        return [];
    }, [field.type, models]);

    // 生成提供商选项用于 provider-select 类型
    const providerOptions = useMemo(() => {
        if (field.type === "provider-select" && providers) {
            return providers.map((provider) => ({
                value: provider.id.toString(),
                label: provider.name,
            }));
        }
        return [];
    }, [field.type, providers]);

    // 生成MCP服务器选项用于 mcp-select 类型
    const mcpOptions = useMemo(() => {
        if (field.type === "mcp-select" && mcpServers) {
            return mcpServers.map((server) => ({
                value: server.id,
                label: server.name,
            }));
        }
        return [];
    }, [field.type, mcpServers]);

    const renderField = (fieldRenderData: any) => {
        switch (field.type) {
            case "select":
                return (
                    <Select
                        disabled={field.disabled}
                        value={fieldRenderData.value}
                        onValueChange={(value) => {
                            fieldRenderData.onChange(value);
                            field.onChange?.(value);
                        }}
                    >
                        <SelectTrigger className="w-full max-w-full focus:ring-ring/20 focus:border-ring overflow-hidden">
                            <SelectValue placeholder={field.label} />
                        </SelectTrigger>
                        <SelectContent>
                            {field.options?.map((option) => (
                                <SelectItem key={option.value} value={option.value}>
                                    {option.label}
                                </SelectItem>
                            ))}
                        </SelectContent>
                    </Select>
                );
            case "model-select":
                return (
                    <Select
                        disabled={field.disabled || modelsLoading}
                        value={fieldRenderData.value}
                        onValueChange={fieldRenderData.onChange}
                    >
                        <SelectTrigger className="w-full max-w-full focus:ring-ring/20 focus:border-ring overflow-hidden">
                            <SelectValue
                                placeholder={modelsLoading ? "加载中..." : modelsError ? "加载失败" : "选择模型"}
                            />
                        </SelectTrigger>
                        <SelectContent>
                            {modelOptions.map((option) => (
                                <SelectItem key={option.value} value={option.value}>
                                    {option.label}
                                </SelectItem>
                            ))}
                        </SelectContent>
                    </Select>
                );
            case "provider-select":
                return (
                    <Select
                        disabled={field.disabled || providersLoading}
                        value={fieldRenderData.value}
                        onValueChange={(value) => {
                            fieldRenderData.onChange(value);
                            field.onChange?.(value);
                        }}
                    >
                        <SelectTrigger className="w-full max-w-full focus:ring-ring/20 focus:border-ring overflow-hidden">
                            <SelectValue
                                placeholder={providersLoading ? "加载中..." : providersError ? "加载失败" : "选择提供商"}
                            />
                        </SelectTrigger>
                        <SelectContent>
                            {providerOptions.map((option) => (
                                <SelectItem key={option.value} value={option.value}>
                                    {option.label}
                                </SelectItem>
                            ))}
                        </SelectContent>
                    </Select>
                );
            case "mcp-select":
                return (
                    <Select
                        disabled={field.disabled || mcpServersLoading}
                        value={fieldRenderData.value}
                        onValueChange={fieldRenderData.onChange}
                    >
                        <SelectTrigger className="w-full max-w-full focus:ring-ring/20 focus:border-ring overflow-hidden">
                            <SelectValue
                                placeholder={mcpServersLoading ? "加载中..." : mcpServersError ? "加载失败" : "选择MCP服务器"}
                            />
                        </SelectTrigger>
                        <SelectContent>
                            {mcpOptions.map((option) => (
                                <SelectItem key={option.value} value={option.value ? option.value.toString() : ""}>
                                    {option.label}
                                </SelectItem>
                            ))}
                        </SelectContent>
                    </Select>
                );
            case "textarea":
                return (
                    <ExpandableTextareaField
                        label={field.label}
                        className={field.className}
                        placeholder={field.placeholder}
                        disabled={field.disabled}
                        fieldRenderData={fieldRenderData}
                    />
                );
            case "input":
            case "password":
                if (field.type === "password" && isMobilePlatform) {
                    return (
                        <PasswordInput
                            className={`focus:ring-ring/20 focus:border-ring ${field.className || ""}`}
                            disabled={field.disabled}
                            placeholder={field.placeholder}
                            enterKeyHint="done"
                            inputMode={field.inputMode}
                            {...fieldRenderData}
                        />
                    );
                }
                return (
                    <Input
                        className={`focus:ring-ring/20 focus:border-ring ${field.className || ""}`}
                        type={field.type === "password" ? "password" : "text"}
                        disabled={field.disabled}
                        placeholder={field.placeholder}
                        enterKeyHint="done"
                        inputMode={field.inputMode}
                        {...fieldRenderData}
                    />
                );
            case "folder-picker":
                return (
                    <FolderPicker
                        value={(fieldRenderData.value as string) || ""}
                        onChange={(value) => {
                            fieldRenderData.onChange(value);
                            field.onChange?.(value);
                        }}
                        placeholder={field.placeholder || "选择文件夹"}
                        disabled={field.disabled}
                        className={field.className}
                    />
                );
            case "checkbox":
                return (
                    <Checkbox
                        className={`focus:ring-ring/20 ${field.className || ""}`}
                        disabled={field.disabled}
                        checked={fieldRenderData.value}
                        onCheckedChange={(value) => {
                            fieldRenderData.onChange(value);
                            if (field.onChange) {
                                field.onChange(value);
                            }
                        }}
                    />
                );
            case "switch":
                const switchValue = fieldRenderData.value ?? field.value;
                return (
                    <Switch
                        className={field.className}
                        disabled={field.disabled}
                        checked={
                            switchValue === true ||
                            switchValue === "true" ||
                            switchValue === 1 ||
                            switchValue === "1"
                        }
                        onCheckedChange={(value) => {
                            // 先调用表单的 onChange
                            fieldRenderData.onChange(value);
                            // 再调用自定义的 onChange（如果有）
                            if (field.onChange) {
                                field.onChange(value);
                            }
                        }}
                    />
                );
            case "radio":
                return (
                    <RadioGroup
                        className={field.className}
                        value={fieldRenderData.value}
                        onValueChange={fieldRenderData.onChange}
                        disabled={field.disabled}
                    >
                        {field.options?.map((option) => (
                            <FormItem className={isMobilePlatform ? "flex flex-wrap items-center gap-x-2" : "flex items-center space-x-2"} key={option.value}>
                                <RadioGroupItem
                                    value={option.value}
                                    id={option.value}
                                    className="focus:ring-ring/20"
                                />
                                <label
                                    htmlFor={option.value}
                                    className={`text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70 text-foreground${isMobilePlatform ? " min-h-11 flex items-center" : ""}`}
                                >
                                    {option.label}
                                </label>
                                {option.tooltip && isMobilePlatform && (
                                    <p className="basis-full pl-6 text-xs leading-5 text-muted-foreground">{option.tooltip}</p>
                                )}
                                {option.tooltip && !isMobilePlatform && (
                                    <TooltipProvider>
                                        <Tooltip>
                                            <TooltipTrigger>
                                                <CircleHelp size={16} className="text-muted-foreground" />
                                            </TooltipTrigger>
                                            <TooltipContent>{option.tooltip}</TooltipContent>
                                        </Tooltip>
                                    </TooltipProvider>
                                )}
                            </FormItem>
                        ))}
                    </RadioGroup>
                );
            case "static":
                return (
                    <div
                        className={`text-sm text-muted-foreground px-3 py-2 bg-muted rounded-md break-words whitespace-pre-wrap ${field.className || ""
                            }`}
                    >
                        {field.value}
                    </div>
                );
            case "custom":
                return field.customRender ? field.customRender(fieldRenderData) : null;
            case "button":
                return (
                    <Button
                        type="button"
                        variant="outline"
                        className={`hover:bg-muted hover:border-border ${field.className || ""}`}
                        disabled={field.disabled}
                        onClick={() => {
                            field.onClick && field.onClick(assistantConfigApi);
                        }}
                    >
                        {field.value as string}
                    </Button>
                );
            case "inline-buttons":
                return (
                    <div className="flex items-center gap-2 flex-1">
                        <div className={`flex-1 text-sm text-muted-foreground px-3 py-2 bg-muted rounded-md ${field.className || ""}`}>
                            {field.value}
                        </div>
                        {field.buttons?.map((btn, i) => (
                            <Button
                                key={i}
                                type="button"
                                variant={btn.variant || "outline"}
                                size="sm"
                                disabled={btn.disabled}
                                className={btn.className}
                                onClick={btn.onClick}
                            >
                                {btn.text}
                            </Button>
                        ))}
                    </div>
                );
            default:
                return null;
        }
    };

    return (
        <Controller
            control={control}
            name={name}
            render={({ field: fieldRenderData }: { field: any }) => (
                <FormItem className={`mb-6 ${field.hidden ? "hidden" : ""}`}>
                    <FormLabel className="flex items-center font-semibold text-sm text-foreground">
                        {field.label}
                        {field.tooltip && !isMobilePlatform && (
                            <TooltipProvider>
                                <Tooltip>
                                    <TooltipTrigger className="ml-2">
                                        <CircleHelp
                                            size={16}
                                            className="text-muted-foreground hover:text-foreground transition-colors"
                                        />
                                    </TooltipTrigger>
                                    <TooltipContent className="max-w-sm text-left leading-relaxed">
                                        {field.tooltip}
                                    </TooltipContent>
                                </Tooltip>
                            </TooltipProvider>
                        )}
                    </FormLabel>
                    {field.tooltip && isMobilePlatform ? (
                        <p className="text-xs leading-5 text-muted-foreground">{field.tooltip}</p>
                    ) : null}
                    <FormControl>{renderField(fieldRenderData)}</FormControl>
                    {field.hint ? (
                        <p className="text-xs leading-5 text-muted-foreground">{field.hint}</p>
                    ) : null}
                    <FormMessage />
                </FormItem>
            )}
        />
    );
};

const ConfigForm: React.FC<ConfigFormProps> = ({
    title,
    description,
    config,
    classNames,
    enableExpand = false,
    defaultExpanded = true,
    layout = "default",
    useFormReturn,
    isDirty: isDirtyProp,
    assistantConfigApi,
    assistantType,
    onSave,
    onCopy,
    onDelete,
    onEdit,
    onTitleClick,
    extraButtons,
}) => {
    const [isExpanded, setIsExpanded] = useState<boolean>(defaultExpanded);
    const isMobilePlatform = useIsMobilePlatform();
    const contentRef = useRef<HTMLDivElement>(null);
    // Only read RHF's proxy when no explicit value is given (reading it subscribes).
    const isDirty = isDirtyProp ?? useFormReturn.formState.isDirty;

    // 检查是否需要获取模型数据
    const hasModelSelect = useMemo(() => {
        return config.some(item => item.config.type === "model-select");
    }, [config]);

    // 检查是否需要获取提供商数据
    const hasProviderSelect = useMemo(() => {
        return config.some(item => item.config.type === "provider-select");
    }, [config]);

    // 检查是否需要获取MCP服务器数据
    const hasMcpSelect = useMemo(() => {
        return config.some(item => item.config.type === "mcp-select");
    }, [config]);

    // 条件获取模型数据：如果有 assistantType，使用过滤后的模型；否则使用全部模型
    const { models, loading: modelsLoading, error: modelsError } = assistantType !== null && assistantType !== undefined
        ? useFilteredModels(assistantType, hasModelSelect)
        : useModels(hasModelSelect);

    // 条件获取提供商数据
    const { providers, loading: providersLoading, error: providersError } = assistantType !== null && assistantType !== undefined
        ? useFilteredProviders(assistantType, hasProviderSelect)
        : { providers: [], loading: false, error: null };

    // 条件获取MCP服务器数据用于 mcp-select 类型
    const { mcpServers, loading: mcpServersLoading, error: mcpServersError } = useMcpServers(hasMcpSelect);

    const toggleExpand = () => {
        if (enableExpand) {
            setIsExpanded(!isExpanded);
        }
    };

    useEffect(() => {
        const content = contentRef.current;
        if (content) {
            const handleTransitionEnd = () => {
                if (isExpanded) {
                    content.style.overflow = "visible";
                }
            };
            const handleTransitionStart = () => {
                if (!isExpanded) {
                    content.style.overflow = "hidden";
                }
            };
            content.addEventListener("transitionend", handleTransitionEnd);
            content.addEventListener("transitionstart", handleTransitionStart);

            return () => {
                content.removeEventListener("transitionend", handleTransitionEnd);
                content.removeEventListener("transitionstart", handleTransitionStart);
            };
        }
    }, [isExpanded]);

    useEffect(() => {
        const content = contentRef.current;

        if (content) {
            if (isExpanded) {
                content.style.overflow = "visible";
            } else {
                content.style.overflow = "hidden";
            }
        }
    }, []);

    const fieldData = useMemo<FieldOptionsData>(() => ({
        models, modelsLoading, modelsError,
        providers, providersLoading, providersError,
        mcpServers, mcpServersLoading, mcpServersError,
    }), [models, modelsLoading, modelsError, providers, providersLoading, providersError, mcpServers, mcpServersLoading, mcpServersError]);

    // Stable component identity: ConfigFormField is a module-level component, so a
    // parent re-render only updates props and never remounts inputs (keeps focus).
    const renderFormField = (name: string, field: ConfigField) => (
        <ConfigFormField
            key={name}
            field={field}
            name={name}
            control={useFormReturn.control}
            data={fieldData}
            assistantConfigApi={assistantConfigApi}
        />
    );

    const renderContent = () => {
        switch (layout) {
            case "prompt":
                return (
                    <div className="grid grid-cols-1 lg:grid-cols-2 gap-8">
                        <div className="space-y-6">
                            {config
                                .filter((item) => item.key !== "prompt")
                                .map((item) => renderFormField(item.key, item.config))}
                        </div>
                        {config.find((item) => item.key === "prompt") && (
                            <div className="space-y-6">
                                {renderFormField("prompt", config.find((item) => item.key === "prompt")!.config)}
                            </div>
                        )}
                    </div>
                );
            case "provider":
                return (
                    <div className="grid grid-cols-1 lg:grid-cols-2 gap-8">
                        <div className="space-y-6">
                            {config.map((item) => renderFormField(item.key, item.config))}
                        </div>
                        {config.find((item) => item.key === "modelList") && (
                            <div className="space-y-6">
                                {renderFormField("model_list", config.find((item) => item.key === "modelList")!.config)}
                            </div>
                        )}
                    </div>
                );
            default:
                return (
                    <div className="space-y-6">
                        {config.map((item) => renderFormField(item.key, item.config))}
                    </div>
                );
        }
    };

    return (
        <Card className={`shadow-none border-l-4 border-l-primary ${classNames || ""}`} data-aipp-config-section>
            <CardHeader
                onClick={toggleExpand}
                className={`flex flex-row items-center ${enableExpand ? "cursor-pointer hover:bg-muted" : ""
                    } transition-colors rounded-t-lg`}
            >
                <div className="flex items-center flex-1 min-w-0">
                    {enableExpand && (
                        <div className="mr-3 text-muted-foreground">
                            {isExpanded ? <ChevronDown className="h-5 w-5" /> : <ChevronRight className="h-5 w-5" />}
                        </div>
                    )}
                    <div className="flex-1 min-w-0">
                        <CardTitle
                            className={`text-lg font-semibold text-foreground truncate ${onTitleClick ? 'cursor-pointer hover:text-primary transition-colors' : ''}`}
                            onClick={(e) => {
                                if (onTitleClick) {
                                    e.stopPropagation();
                                    onTitleClick();
                                }
                            }}
                        >
                            {title}
                        </CardTitle>
                        {description && (
                            <CardDescription className="text-sm text-muted-foreground mt-1 truncate">
                                {description}
                            </CardDescription>
                        )}
                    </div>
                </div>
                <div className="flex items-center gap-2 ml-4 flex-shrink-0">
                    {onSave && isDirty ? (
                        <span
                            className="rounded-full border border-foreground/30 bg-muted px-2 py-0.5 text-xs font-medium text-foreground"
                            data-aipp-slot="config-form-dirty-badge"
                        >
                            未保存
                        </span>
                    ) : null}
                    {/* 移动端展开时使用底部固定保存栏，头部只保留状态与更多菜单 */}
                    {onSave && !(isMobilePlatform && isExpanded) && (
                        <Button
                            type="button"
                            size="sm"
                            onClick={(e) => {
                                e.stopPropagation();
                                void onSave(useFormReturn.getValues());
                            }}
                            className="bg-primary hover:bg-primary/90 text-primary-foreground min-h-9"
                            data-aipp-slot="config-form-save-header"
                        >
                            保存配置
                        </Button>
                    )}
                    {isMobilePlatform && (onCopy || onEdit || onDelete) ? (
                        <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                                <Button
                                    type="button"
                                    variant="ghost"
                                    size="icon"
                                    aria-label="更多操作"
                                    onClick={(e) => e.stopPropagation()}
                                >
                                    <MoreHorizontal className="h-4 w-4" />
                                </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end" onClick={(e) => e.stopPropagation()}>
                                {onCopy && (
                                    <DropdownMenuItem onSelect={onCopy}>
                                        <Copy className="h-4 w-4" />
                                        复制
                                    </DropdownMenuItem>
                                )}
                                {onEdit && (
                                    <DropdownMenuItem onSelect={onEdit}>
                                        <Edit3 className="h-4 w-4" />
                                        编辑
                                    </DropdownMenuItem>
                                )}
                                {onDelete && (
                                    <DropdownMenuItem variant="destructive" onSelect={onDelete}>
                                        <Trash2 className="h-4 w-4" />
                                        删除
                                    </DropdownMenuItem>
                                )}
                            </DropdownMenuContent>
                        </DropdownMenu>
                    ) : null}
                    {!isMobilePlatform && onCopy && (
                        <Button
                            variant="ghost"
                            size="sm"
                            onClick={onCopy}
                            className="hover:bg-muted hover:border-border hover:text-foreground"
                        >
                            <Copy className="h-4 w-4 mr-1" />
                        </Button>
                    )}
                    {!isMobilePlatform && onEdit && (
                        <Button
                            variant="ghost"
                            size="sm"
                            onClick={onEdit}
                            className="hover:bg-muted hover:border-border hover:text-foreground"
                        >
                            <Edit3 className="h-4 w-4 mr-1" />
                        </Button>
                    )}
                    {!isMobilePlatform && onDelete && (
                        <Button
                            variant="ghost"
                            size="sm"
                            onClick={onDelete}
                            className="hover:bg-destructive/10 hover:border-destructive/30 hover:text-destructive"
                        >
                            <Trash2 className="h-4 w-4 mr-1" />
                        </Button>
                    )}
                    {extraButtons}
                </div>
            </CardHeader>

            <CardContent
                ref={contentRef}
                className={`transition-all duration-300 ease-in-out ${isExpanded ? "max-h-none opacity-100" : "max-h-0 opacity-0 overflow-hidden"
                    }`}
            >
                <Form {...useFormReturn}>
                    {renderContent()}
                    {onSave && (
                        <div
                            className="sticky bottom-0 z-20 mt-8 border-t border-border bg-card/95 pt-4 pb-[max(1rem,env(safe-area-inset-bottom,0px))] backdrop-blur supports-[backdrop-filter]:bg-card/80"
                            style={{ bottom: "var(--aipp-keyboard-inset-bottom, 0px)" }}
                            data-aipp-slot="config-form-save-footer"
                        >
                            <div className="flex items-center gap-3">
                                <Button
                                    type="button"
                                    onClick={() => void onSave(useFormReturn.getValues())}
                                    className="bg-primary hover:bg-primary/90 text-primary-foreground min-h-11"
                                >
                                    保存配置
                                </Button>
                                {isDirty ? (
                                    <span className="text-xs font-medium text-foreground">有未保存的更改</span>
                                ) : null}
                            </div>
                        </div>
                    )}
                </Form>
            </CardContent>
        </Card>
    );
};

export default React.memo(ConfigForm);
