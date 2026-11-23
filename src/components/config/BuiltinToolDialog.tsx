import AppIcon from './apps/AppIcon';
import React, { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from "../ui/dialog";
import { Label } from "../ui/label";
import { Button } from "../ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";
import { Switch } from "../ui/switch";
import { Card, CardContent } from "../ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../ui/collapsible";
import { ChevronDown } from "lucide-react";
import { useFeatureAvailableOnPlatform, BROWSER_SEARCH_ENV_KEYS } from "@/lib/mobileUnsupported";
import { useVaultItems } from "@/hooks/useVaultItems";
import VaultRefField from "./vault/VaultRefField";
import {
    SEARCH_PROVIDER_KEY,
    browserSettingsOpenByDefault,
    displayedSearchProvider,
    searchFieldPlacement,
    searchProviderOptions,
} from "./searchToolFields";

/** 内置搜索里唯一可以引用保险箱的设置（后端 vault::config_ref 只为它解析引用）。 */
const SEARCH_VAULT_KEY = "HTTP_SEARCH_API_KEY";
const HTTP_SEARCH_DEFAULT_ORIGINS: Record<string, string> = {
    deepseek: "https://api.deepseek.com",
    tavily: "https://api.tavily.com",
    brave: "https://api.search.brave.com",
};

type EnvVarOption = {
    label: string;
    value: string;
};

type BuiltinTemplateEnvVar = {
    key: string;
    label: string;
    required: boolean;
    tip?: string;
    field_type: string; // "text", "select", "boolean", "number"
    default_value?: string;
    placeholder?: string;
    options?: EnvVarOption[];
};

type BuiltinTemplate = {
    id: string;
    name: string;
    description: string;
    command: string;
    transport_type: string;
    required_envs: BuiltinTemplateEnvVar[];
    default_timeout?: number;
};

interface BuiltinToolDialogProps {
    initialTemplateId?: string;
    isOpen: boolean;
    onClose: () => void;
    onSubmit: () => void;
    editing?: boolean;
    initialName?: string;
    initialDescription?: string;
    initialCommand?: string;
    initialEnvText?: string;
    initialTimeout?: number | null;
    onEnvChange?: (v: string) => void;
    onNameChange?: (v: string) => void;
    onTimeoutChange?: (v: number | null) => void;
}

const BuiltinToolDialog: React.FC<BuiltinToolDialogProps> = ({
    initialTemplateId,
    isOpen,
    onClose,
    onSubmit,
    editing = false,
    initialName,
    initialDescription,
    initialCommand,
    initialEnvText,
    initialTimeout,
    onEnvChange,
    onNameChange,
    onTimeoutChange,
}) => {
    const [templates, setTemplates] = useState<BuiltinTemplate[]>([]);
    const [selectedId, setSelectedId] = useState<string>("search");
    useEffect(() => { if (isOpen && initialTemplateId && !editing) setSelectedId(initialTemplateId); }, [isOpen, initialTemplateId, editing]);
    const [envValues, setEnvValues] = useState<Record<string, string>>({});
    const [busy, setBusy] = useState(false);
    const [initialized, setInitialized] = useState(false);
    const [editedName, setEditedName] = useState<string>("");
    const [timeout, setTimeout] = useState<number | null>(null);

    // 浏览器模式搜索依赖仅桌面编译的 chromiumoxide，移动端隐藏其专属环境变量字段
    const browserSearchAvailable = useFeatureAvailableOnPlatform("browser_search");
    const vault = useVaultItems();

    const selected = useMemo(() => templates.find((t) => t.id === selectedId), [templates, selectedId]);

    // Parse initial envText to envValues, and merge with default values from template
    useEffect(() => {
        // Only run once when dialog opens and template is loaded
        if (!isOpen || initialized) return;
        
        // Get the template for current mode
        let currentTemplate = selected;
        if (editing && initialCommand) {
            const templateId = initialCommand.replace("aipp:", "");
            currentTemplate = templates.find((t) => t.id === templateId);
        }
        
        if (editing && !currentTemplate) return; // Wait for template to load in editing mode

        // Parse the initial env text
        const parsedEnvs: Record<string, string> = {};
        if (initialEnvText) {
            initialEnvText
                .split("\n")
                .map((l) => l.trim())
                .filter(Boolean)
                .forEach((line) => {
                    const idx = line.indexOf("=");
                    if (idx > 0) {
                        const k = line.slice(0, idx).trim();
                        const v = line.slice(idx + 1).trim();
                        if (k) parsedEnvs[k] = v;
                    }
                });
        }

        // In editing mode, merge with default values for fields that don't have a saved value
        if (editing && currentTemplate) {
            const defaultValues: Record<string, string> = {};
            currentTemplate.required_envs.forEach((env) => {
                if (env.default_value && parsedEnvs[env.key] === undefined) {
                    defaultValues[env.key] = env.default_value;
                }
            });
            setEnvValues({ ...defaultValues, ...parsedEnvs });
            setInitialized(true);
        } else if (!editing) {
            setEnvValues(parsedEnvs);
            // Don't set initialized here for non-editing mode, let the other effect handle defaults
        }
    }, [isOpen, initialEnvText, editing, selected, initialized, templates, initialCommand]);

    // Set default values when template changes (non-editing mode only)
    useEffect(() => {
        if (editing || !selected) return;

        const defaultValues: Record<string, string> = {};
        selected.required_envs.forEach((env) => {
            if (env.default_value) {
                defaultValues[env.key] = env.default_value;
            }
        });
        setEnvValues(defaultValues);
        setInitialized(true);
    }, [selected, editing]);

    // Reset state when dialog closes
    useEffect(() => {
        if (!isOpen) {
            setInitialized(false);
            setEnvValues({});
            setEditedName("");
            setTimeout(null);
        }
    }, [isOpen]);

    // Initialize editedName when dialog opens in editing mode
    useEffect(() => {
        if (isOpen && editing && initialName) {
            setEditedName(initialName);
        }
    }, [isOpen, editing, initialName]);

    // Initialize timeout when dialog opens
    useEffect(() => {
        if (isOpen && editing && initialTimeout !== undefined) {
            setTimeout(initialTimeout);
        } else if (isOpen && !editing && selected?.default_timeout) {
            setTimeout(selected.default_timeout);
        }
    }, [isOpen, editing, initialTimeout, selected]);

    // Notify parent when name changes
    useEffect(() => {
        if (editing && onNameChange && editedName) {
            onNameChange(editedName);
        }
    }, [editing, editedName, onNameChange]);

    // Notify parent when timeout changes
    useEffect(() => {
        if (onTimeoutChange && timeout !== null) {
            onTimeoutChange(timeout);
        }
    }, [timeout, onTimeoutChange]);

    useEffect(() => {
        if (!isOpen) return;
        // Always fetch templates, even in editing mode for field definitions
        invoke<BuiltinTemplate[]>("list_aipp_builtin_templates")
            .then(setTemplates)
            .catch(() => { });
    }, [isOpen]);

    // Convert envValues to string format for onEnvChange callback (only after initialization)
    useEffect(() => {
        if (!initialized || !onEnvChange) return;

        const envText = Object.entries(envValues)
            .filter(([_, value]) => value !== "")
            .map(([key, value]) => `${key}=${value}`)
            .join("\n");
        onEnvChange(envText);
    }, [envValues, onEnvChange, initialized]);

    const handleEnvValueChange = (key: string, value: string) => {
        setEnvValues((prev) => ({
            ...prev,
            [key]: value,
        }));
    };

    const handleSubmit = async () => {
        // In editing mode, we don't need selected template
        if (!editing && !selected) return;

        setBusy(true);
        // Convert envValues to the expected format
        const envs = Object.fromEntries(Object.entries(envValues).filter(([_, value]) => value !== ""));

        try {
            if (!editing) {
                await invoke<number>("add_or_update_aipp_builtin_server", {
                    templateId: selected!.id,
                    name: selected!.name,
                    description: selected!.description,
                    envs,
                    timeout: timeout ?? selected!.default_timeout ?? null,
                });
            } else {
                // In editing mode, just call onSubmit with the parsed environment variables
                // The parent component (MCPConfig) will handle the actual update
            }
            onSubmit();
        } catch (e) {
            // noop; outer page toasts
            console.error(e);
        } finally {
            setBusy(false);
        }
    };

    const renderEnvField = (env: BuiltinTemplateEnvVar) => {
        const value = envValues[env.key] || "";

        const fieldId = `env-${env.key}`;

        if (env.field_type === "password" && templateForFields?.id === "search" && env.key === SEARCH_VAULT_KEY) {
            // 保存时确认：凭据只发往当前配置的搜索 API 地址
            let target: string | null = HTTP_SEARCH_DEFAULT_ORIGINS[envValues.HTTP_SEARCH_PROVIDER ?? ""] ?? null;
            try {
                if (envValues.HTTP_SEARCH_ENDPOINT?.trim()) target = new URL(envValues.HTTP_SEARCH_ENDPOINT.trim()).origin;
            } catch {
                target = null;
            }
            return (
                <div key={env.key} className="space-y-2">
                    <Label htmlFor={fieldId} className="text-sm font-medium">
                        {env.label}
                        {env.required && <span className="text-red-500 ml-1">*</span>}
                    </Label>
                    <VaultRefField
                        vault={vault}
                        id={fieldId}
                        ariaLabel={env.label}
                        value={value}
                        onChange={(next) => handleEnvValueChange(env.key, next)}
                        suggestedName="搜索 API Key"
                        targetHint={target}
                    />
                    {env.tip && <p className="text-xs text-muted-foreground">{env.tip}</p>}
                </div>
            );
        }

        switch (env.field_type) {
            case "select": {
                const isSearchProvider = isSearchTemplate && env.key === SEARCH_PROVIDER_KEY;
                const selectValue = isSearchProvider ? displayedSearchProvider(value, browserSearchAvailable) : value;
                const options = isSearchProvider ? searchProviderOptions(env.options ?? [], browserSearchAvailable) : env.options;
                return (
                    <div key={env.key} className="space-y-2">
                        <Label htmlFor={fieldId} className="text-sm font-medium">
                            {env.label}
                            {env.required && <span className="text-red-500 ml-1">*</span>}
                        </Label>
                        <Select value={selectValue} onValueChange={(val) => handleEnvValueChange(env.key, val)}>
                            <SelectTrigger id={fieldId} aria-label={env.label}>
                                <SelectValue placeholder={env.placeholder || `选择${env.label}`} />
                            </SelectTrigger>
                            <SelectContent>
                                {options?.map((option) => (
                                    <SelectItem key={option.value} value={option.value}>
                                        {option.label}
                                    </SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                        {env.tip && <p className="text-xs text-muted-foreground">{env.tip}</p>}
                    </div>
                );
            }

            case "boolean":
                return (
                    <div key={env.key} className="space-y-2">
                        <div className="flex items-center justify-between">
                            <Label htmlFor={fieldId} className="text-sm font-medium">
                                {env.label}
                                {env.required && <span className="text-red-500 ml-1">*</span>}
                            </Label>
                            <Switch
                                id={fieldId}
                                checked={value === "true"}
                                onCheckedChange={(checked) => handleEnvValueChange(env.key, checked ? "true" : "false")}
                            />
                        </div>
                        {env.tip && <p className="text-xs text-muted-foreground">{env.tip}</p>}
                    </div>
                );

            case "number":
                return (
                    <div key={env.key} className="space-y-2">
                        <Label htmlFor={fieldId} className="text-sm font-medium">
                            {env.label}
                            {env.required && <span className="text-red-500 ml-1">*</span>}
                        </Label>
                        <Input
                            id={fieldId}
                            type="number"
                            value={value}
                            placeholder={env.placeholder}
                            onChange={(e) => handleEnvValueChange(env.key, e.target.value)}
                        />
                        {env.tip && <p className="text-xs text-muted-foreground">{env.tip}</p>}
                    </div>
                );

            case "textarea":
                return (
                    <div key={env.key} className="space-y-2 col-span-2">
                        <Label htmlFor={fieldId} className="text-sm font-medium">
                            {env.label}
                            {env.required && <span className="text-red-500 ml-1">*</span>}
                        </Label>
                        <Textarea
                            id={fieldId}
                            value={value}
                            placeholder={env.placeholder}
                            onChange={(e) => handleEnvValueChange(env.key, e.target.value)}
                            rows={4}
                            className="font-mono text-xs resize-y"
                        />
                        {env.tip && <p className="text-xs text-muted-foreground">{env.tip}</p>}
                    </div>
                );

            case "password":
            case "text":
            default:
                return (
                    <div key={env.key} className="space-y-2">
                        <Label htmlFor={fieldId} className="text-sm font-medium">
                            {env.label}
                            {env.required && <span className="text-red-500 ml-1">*</span>}
                        </Label>
                        <Input
                            id={fieldId}
                            type={env.field_type === "password" ? "password" : "text"}
                            value={value}
                            placeholder={env.placeholder}
                            onChange={(e) => handleEnvValueChange(env.key, e.target.value)}
                        />
                        {env.tip && <p className="text-xs text-muted-foreground">{env.tip}</p>}
                    </div>
                );
        }
    };

    // Get the template to use for rendering fields
    const templateForFields = useMemo(() => {
        if (editing && initialCommand) {
            // Extract template ID from command (e.g., "aipp:search" -> "search")
            const templateId = initialCommand.replace("aipp:", "");
            return templates.find((t) => t.id === templateId);
        }
        return selected;
    }, [editing, initialCommand, templates, selected]);

    // 搜索工具：设置项随「搜索服务」联动（searchToolFields.ts）
    const isSearchTemplate = templateForFields?.id === "search";
    const browserFields = isSearchTemplate
        ? (templateForFields?.required_envs ?? []).filter((env) => searchFieldPlacement(env.key, envValues, browserSearchAvailable) === "browser")
        : [];
    const browserOpenDefault = browserSettingsOpenByDefault(envValues);
    const [browserOpen, setBrowserOpen] = useState(browserOpenDefault);
    // 切换搜索服务时按新服务重置展开状态；用户手动展开/收起后不打扰
    const lastProviderRef = useRef(envValues[SEARCH_PROVIDER_KEY]);
    useEffect(() => {
        if (lastProviderRef.current === envValues[SEARCH_PROVIDER_KEY]) return;
        lastProviderRef.current = envValues[SEARCH_PROVIDER_KEY];
        setBrowserOpen(browserOpenDefault);
    }, [envValues, browserOpenDefault]);

    return (
        <Dialog open={isOpen} onOpenChange={(open) => !open && !busy && onClose()}>
            <CompactDialogContent className="sm:max-w-3xl">
                <CompactDialogHeader
                    title={editing ? "编辑内置工具" : "添加内置工具"}
                    subtitle={editing ? initialCommand : selected?.command}
                    description="配置内置工具的超时时间与环境变量"
                    closeDisabled={busy}
                />
                <CompactDialogBody className="space-y-4">
                    {/* Template selector; hidden in edit mode */}
                    {!editing && (
                        <div className="space-y-2">
                            <Label>选择内置工具</Label>
                            <Select value={selectedId} onValueChange={setSelectedId}>
                                <SelectTrigger className="h-auto w-full">
                                    <SelectValue placeholder="选择一个内置工具" />
                                </SelectTrigger>
                                <SelectContent>
                                    {templates.map((t) => (
                                        <SelectItem key={t.id} value={t.id}>
                                            <span className="flex items-center gap-2"><AppIcon size="sm" builtinCommand={t.command} />{t.name}</span>
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                        </div>
                    )}

                    {/* Readonly basics as plain text, name editable in edit mode */}
                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 text-sm">
                        <div>
                            <div className="text-muted-foreground">名称</div>
                            {editing ? (
                                <Input
                                    value={editedName}
                                    onChange={(e) => setEditedName(e.target.value)}
                                    placeholder="输入名称"
                                    className="mt-1"
                                />
                            ) : (
                                <div className="text-foreground break-all">
                                    {selected?.id ?? ""}
                                </div>
                            )}
                        </div>
                        <div>
                            <div className="text-muted-foreground">类型</div>
                            <div className="text-foreground">{editing ? "stdio" : selected?.transport_type ?? ""}</div>
                        </div>
                        <div className="sm:col-span-2">
                            <div className="text-muted-foreground">描述</div>
                            <div className="text-foreground whitespace-pre-wrap">
                                {editing ? initialDescription || "" : selected?.description ?? ""}
                            </div>
                        </div>
                        <div className="sm:col-span-2">
                            <div className="text-muted-foreground">命令</div>
                            <div className="text-foreground break-all font-mono">
                                {editing ? initialCommand || "" : selected?.command ?? ""}
                            </div>
                        </div>
                    </div>

                    {/* 超时时间配置 */}
                    <div className="space-y-2">
                        <Label htmlFor="timeout" className="text-base font-semibold">工具超时时间</Label>
                        <div className="flex items-center gap-2">
                            <Input
                                id="timeout"
                                type="number"
                                value={timeout ?? (templateForFields?.default_timeout ?? 30000)}
                                placeholder="30000"
                                onChange={(e) => setTimeout(e.target.value ? parseInt(e.target.value, 10) : null)}
                                className="w-40"
                            />
                            <span className="text-sm text-muted-foreground">毫秒</span>
                        </div>
                        <p className="text-xs text-muted-foreground">整个工具调用的最大超时时间，超时后工具执行将被中止</p>
                    </div>

                    {/* Environment Variables */}
                    <div className="space-y-4">
                        <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
                            <Label className="text-base font-semibold">环境变量配置</Label>
                            {!editing && selected?.required_envs?.some((e) => e.required) && (
                                <div className="text-xs text-muted-foreground">
                                    必填字段:{" "}
                                    {selected.required_envs
                                        .filter((e) => e.required)
                                        .map((e) => e.label)
                                        .join(", ")}
                                </div>
                            )}
                        </div>

                        <Card className="shadow-none">
                            <CardContent className="p-4 sm:p-6">
                                {templateForFields?.required_envs?.length ? (
                                    isSearchTemplate ? (
                                        <div className="space-y-6">
                                            <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                                                {templateForFields.required_envs
                                                    .filter((env) => searchFieldPlacement(env.key, envValues, browserSearchAvailable) === "main")
                                                    .map(renderEnvField)}
                                            </div>
                                            {browserFields.length > 0 && (
                                                <Collapsible open={browserOpen} onOpenChange={setBrowserOpen} className="border-t border-border pt-4">
                                                    <CollapsibleTrigger asChild>
                                                        <button type="button" className="flex w-full items-center justify-between gap-3 text-left">
                                                            <span>
                                                                <span className="block text-sm font-medium">浏览器设置</span>
                                                                <span className="block text-xs text-muted-foreground">
                                                                    {browserOpenDefault ? "本地搜索与本机网页抓取共用" : "搜索不使用浏览器；fetch_url 本机抓取网页时使用"}
                                                                </span>
                                                            </span>
                                                            <ChevronDown className={`h-4 w-4 shrink-0 text-muted-foreground transition-transform ${browserOpen ? "rotate-180" : ""}`} />
                                                        </button>
                                                    </CollapsibleTrigger>
                                                    <CollapsibleContent className="pt-4">
                                                        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                                                            {browserFields.map(renderEnvField)}
                                                        </div>
                                                    </CollapsibleContent>
                                                </Collapsible>
                                            )}
                                        </div>
                                    ) : (
                                        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                                            {templateForFields.required_envs
                                                .filter((env) => browserSearchAvailable || !BROWSER_SEARCH_ENV_KEYS.has(env.key))
                                                .map(renderEnvField)}
                                        </div>
                                    )
                                ) : (
                                    <p className="text-muted-foreground text-center py-4">
                                        {templateForFields ? "该工具无需配置环境变量" : "加载环境变量配置中..."}
                                    </p>
                                )}
                            </CardContent>
                        </Card>
                    </div>
                </CompactDialogBody>
                <CompactDialogFooter>
                    <Button type="button" variant="outline" className="min-h-11 px-4 sm:min-h-9" disabled={busy} onClick={onClose}>
                        取消
                    </Button>
                    <Button type="button" className="min-h-11 px-5 sm:min-h-9" onClick={handleSubmit} disabled={busy}>
                        {editing ? "保存" : "添加"}
                    </Button>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
};

export default BuiltinToolDialog;
