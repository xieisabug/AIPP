import React, { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Label } from "../ui/label";
import { Button } from "../ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";
import { Switch } from "../ui/switch";
import { Card, CardContent } from "../ui/card";

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
    const [envValues, setEnvValues] = useState<Record<string, string>>({});
    const [busy, setBusy] = useState(false);
    const [initialized, setInitialized] = useState(false);
    const [editedName, setEditedName] = useState<string>("");
    const [timeout, setTimeout] = useState<number | null>(null);

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

        switch (env.field_type) {
            case "select":
                return (
                    <div key={env.key} className="space-y-2">
                        <Label htmlFor={fieldId} className="text-sm font-medium">
                            {env.label}
                            {env.required && <span className="text-red-500 ml-1">*</span>}
                        </Label>
                        <Select value={value} onValueChange={(val) => handleEnvValueChange(env.key, val)}>
                            <SelectTrigger>
                                <SelectValue placeholder={env.placeholder || `选择${env.label}`} />
                            </SelectTrigger>
                            <SelectContent>
                                {env.options?.map((option) => (
                                    <SelectItem key={option.value} value={option.value}>
                                        {option.label}
                                    </SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                        {env.tip && <p className="text-xs text-muted-foreground">{env.tip}</p>}
                    </div>
                );

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
                            type="text"
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

    return (
        <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
            <DialogContent className="max-w-4xl min-w-xl w-1/2 sm:max-w-none max-h-[80vh] flex flex-col">
                <DialogHeader>
                    <DialogTitle>{editing ? "编辑内置工具" : "添加内置工具"}</DialogTitle>
                </DialogHeader>
                <div className="space-y-4 overflow-y-auto flex-1 min-h-0 px-4">
                    {/* Template selector; hidden in edit mode */}
                    {!editing && (
                        <div className="space-y-2">
                            <Label>选择内置工具</Label>
                            <Select value={selectedId} onValueChange={setSelectedId}>
                                <SelectTrigger className="w-full">
                                    <SelectValue placeholder="选择一个内置工具" />
                                </SelectTrigger>
                                <SelectContent>
                                    {templates.map((t) => (
                                        <SelectItem key={t.id} value={t.id}>
                                            {t.name}
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                        </div>
                    )}

                    {/* Readonly basics as plain text, name editable in edit mode */}
                    <div className="grid grid-cols-2 gap-4 text-sm">
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
                        <div className="col-span-2">
                            <div className="text-muted-foreground">描述</div>
                            <div className="text-foreground whitespace-pre-wrap">
                                {editing ? initialDescription || "" : selected?.description ?? ""}
                            </div>
                        </div>
                        <div className="col-span-2">
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
                        <div className="flex items-center justify-between">
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
                            <CardContent className="p-6">
                                {templateForFields?.required_envs?.length ? (
                                    <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                                        {templateForFields.required_envs.map(renderEnvField)}
                                    </div>
                                ) : (
                                    <p className="text-muted-foreground text-center py-4">
                                        {templateForFields ? "该工具无需配置环境变量" : "加载环境变量配置中..."}
                                    </p>
                                )}
                            </CardContent>
                        </Card>
                    </div>
                </div>
                <DialogFooter className="flex-shrink-0">
                    <Button variant="ghost" onClick={onClose}>
                        取消
                    </Button>
                    <Button onClick={handleSubmit} disabled={busy}>
                        {editing ? "保存" : "添加"}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
};

export default BuiltinToolDialog;
