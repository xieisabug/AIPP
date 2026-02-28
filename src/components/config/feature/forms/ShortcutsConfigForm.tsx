import React, { useState, useCallback, useMemo } from "react";
import { UseFormReturn } from "react-hook-form";
import { toast } from "sonner";
import ConfigForm from "@/components/ConfigForm";
import { ShortcutRecorder } from "../ShortcutRecorder";
import {
    SHORTCUT_ACTIONS,
    WINDOW_LABELS,
    APP_SHORTCUT_KEY_PREFIX,
    type ShortcutWindow,
} from "@/data/Shortcuts";
import { formatShortcutDisplay } from "@/hooks/useAppShortcuts";
import { Button } from "@/components/ui/button";
import { RotateCcw } from "lucide-react";

interface ShortcutsConfigFormProps {
    form: UseFormReturn<any>;
    onSave: () => Promise<void>;
}

export const ShortcutsConfigForm: React.FC<ShortcutsConfigFormProps> = ({ form, onSave }) => {
    const handleSaveShortcutsConfig = useCallback(async () => {
        try {
            await onSave();
            toast.success("快捷键配置保存成功");
        } catch (e) {
            toast.error("保存快捷键配置失败: " + e);
        }
    }, [onSave]);

    // 录入器状态（全局和应用共用一个录入器）
    const [recorderOpen, setRecorderOpen] = useState(false);
    const [editingKey, setEditingKey] = useState<string | null>(null); // form key being edited
    const [editingCurrentValue, setEditingCurrentValue] = useState("");

    // 打开录入器（通用）
    const openRecorder = useCallback((formKey: string, currentValue: string) => {
        setEditingKey(formKey);
        setEditingCurrentValue(currentValue);
        setRecorderOpen(true);
    }, []);

    // 录入完成回调
    const handleShortcutRecorded = useCallback((shortcut: string) => {
        if (!editingKey) return;
        form.setValue(editingKey, shortcut, { shouldDirty: true });
        // 全局快捷键还需要同步 modifier_key
        if (editingKey === "shortcut") {
            const modifier = shortcut.split("+").find((t) =>
                ["Ctrl", "Shift", "Alt", "Super", "Command"].includes(t)
            );
            if (modifier) {
                const map: Record<string, string> = { Ctrl: "ctrl", Shift: "shift", Alt: "alt", Super: "cmd", Command: "cmd" };
                form.setValue("modifier_key", map[modifier] || "alt");
            }
        }
    }, [editingKey, form]);

    // 重置单个应用快捷键
    const handleResetAppShortcut = useCallback((actionId: string) => {
        const action = SHORTCUT_ACTIONS.find((a) => a.id === actionId);
        if (action) {
            const configKey = APP_SHORTCUT_KEY_PREFIX + actionId;
            form.setValue(configKey, action.defaultShortcut, { shouldDirty: true });
        }
    }, [form]);

    // 重置全部应用快捷键
    const handleResetAllAppShortcuts = useCallback(() => {
        for (const action of SHORTCUT_ACTIONS) {
            const configKey = APP_SHORTCUT_KEY_PREFIX + action.id;
            form.setValue(configKey, action.defaultShortcut, { shouldDirty: true });
        }
    }, [form]);

    // 冲突检测：同一窗口内的快捷键不能重复
    const conflictMap = useMemo(() => {
        const map: Record<string, string[]> = {};
        const windowGroups: Record<string, { actionId: string; shortcut: string }[]> = {};

        for (const action of SHORTCUT_ACTIONS) {
            const configKey = APP_SHORTCUT_KEY_PREFIX + action.id;
            const value = form.watch(configKey) || action.defaultShortcut;
            if (!windowGroups[action.window]) {
                windowGroups[action.window] = [];
            }
            windowGroups[action.window].push({ actionId: action.id, shortcut: value });
        }

        for (const entries of Object.values(windowGroups)) {
            const seen: Record<string, string[]> = {};
            for (const { actionId, shortcut } of entries) {
                if (!shortcut) continue;
                const normalized = shortcut.toLowerCase();
                if (!seen[normalized]) seen[normalized] = [];
                seen[normalized].push(actionId);
            }
            for (const ids of Object.values(seen)) {
                if (ids.length > 1) {
                    for (const id of ids) {
                        map[id] = ids.filter((i) => i !== id);
                    }
                }
            }
        }
        return map;
    }, [form, form.watch]);

    // 按窗口分组
    const actionsByWindow = useMemo(() => {
        const groups: Record<ShortcutWindow, typeof SHORTCUT_ACTIONS> = { ask: [], chat: [] };
        for (const action of SHORTCUT_ACTIONS) {
            groups[action.window].push(action);
        }
        return groups;
    }, []);

    // 快捷键行组件
    const ShortcutRow = ({ label, formKey, defaultValue, actionId }: {
        label: string;
        formKey: string;
        defaultValue?: string;
        actionId?: string;
    }) => {
        const currentValue = form.watch(formKey) || defaultValue || "";
        const isDefault = defaultValue ? currentValue === defaultValue : false;
        const conflicts = actionId ? conflictMap[actionId] : undefined;

        return (
            <div className="flex items-center justify-between py-2 px-3 rounded-md hover:bg-muted/50">
                <span className="text-sm">{label}</span>
                <div className="flex items-center gap-2">
                    {conflicts && (
                        <span className="text-xs text-destructive">冲突</span>
                    )}
                    <div
                        className={`w-40 px-3 py-1 rounded border font-mono text-[13px] text-center cursor-pointer hover:bg-accent transition-colors ${
                            conflicts ? "border-destructive" : ""
                        }`}
                        onClick={() => openRecorder(formKey, currentValue)}
                    >
                        {formatShortcutDisplay(currentValue) || "未设置"}
                    </div>
                    <div className="flex h-7 w-7 items-center justify-center">
                        {actionId && !isDefault ? (
                            <button
                                type="button"
                                onClick={() => handleResetAppShortcut(actionId)}
                                className="text-muted-foreground hover:text-foreground"
                                title="恢复默认"
                            >
                                <RotateCcw className="h-3.5 w-3.5" />
                            </button>
                        ) : (
                            <RotateCcw className="h-3.5 w-3.5 opacity-0 pointer-events-none" />
                        )}
                    </div>
                </div>
            </div>
        );
    };

    // 全部内容渲染
    const renderAllShortcuts = () => (
        <div className="space-y-6">
            {/* 全局快捷键 */}
            <div>
                <h4 className="text-sm font-medium mb-3 text-muted-foreground">全局快捷键</h4>
                <div className="space-y-1">
                    <ShortcutRow
                        label="唤起 Ask 窗口"
                        formKey="shortcut"
                    />
                </div>
                <p className="text-xs text-muted-foreground mt-2 px-3">
                    如果有选中的文本，会自动捕获并填充到输入框
                </p>
            </div>

            {/* 应用内快捷键 */}
            {(Object.keys(actionsByWindow) as ShortcutWindow[]).map((windowKey) => {
                const actions = actionsByWindow[windowKey];
                if (actions.length === 0) return null;
                return (
                    <div key={windowKey}>
                        <h4 className="text-sm font-medium mb-3 text-muted-foreground">
                            {WINDOW_LABELS[windowKey]}
                        </h4>
                        <div className="space-y-1">
                            {actions.map((action) => (
                                <ShortcutRow
                                    key={action.id}
                                    label={action.label}
                                    formKey={APP_SHORTCUT_KEY_PREFIX + action.id}
                                    defaultValue={action.defaultShortcut}
                                    actionId={action.id}
                                />
                            ))}
                        </div>
                    </div>
                );
            })}
        </div>
    );

    const FORM_CONFIG = [
        {
            key: "_all_shortcuts",
            config: {
                type: "custom" as const,
                label: "",
                customRender: renderAllShortcuts,
            },
        },
    ];

    const resetButton = (
        <Button
            variant="ghost"
            size="sm"
            onClick={handleResetAllAppShortcuts}
            className="hover:bg-muted hover:border-border hover:text-foreground"
        >
            <RotateCcw className="h-4 w-4 mr-1" />
            恢复默认
        </Button>
    );

    return (
        <>
            <ConfigForm
                title="快捷键"
                description="配置全局和应用内的键盘快捷键，点击快捷键可重新录入。"
                config={FORM_CONFIG}
                layout="default"
                useFormReturn={form}
                onSave={handleSaveShortcutsConfig}
                extraButtons={resetButton}
            />

            <ShortcutRecorder
                open={recorderOpen}
                onOpenChange={setRecorderOpen}
                currentShortcut={editingCurrentValue}
                onShortcutChange={handleShortcutRecorded}
            />
        </>
    );
};

export default ShortcutsConfigForm;
