import React, { useState, useCallback, useMemo } from "react";
import { UseFormReturn } from "react-hook-form";
import { toast } from "sonner";
import ConfigForm from "@/components/ConfigForm";
import { ShortcutRecorder } from "../ShortcutRecorder";

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

    // 录入：点击按钮弹出小窗录入
    const [recorderOpen, setRecorderOpen] = useState(false);

    const shortcutValue: string = form.watch("shortcut") || "";

    const displayText = useMemo(() => {
        if (!shortcutValue) return "未设置";
        const parts = shortcutValue.split("+");
        const last = parts[parts.length - 1] || "";
        let keyPart = last.replace(/^Key([A-Z])$/, "$1").replace(/^Digit([0-9])$/, "$1");
        if (/^Space$/i.test(keyPart)) keyPart = "Space";
        return [...parts.slice(0, -1), keyPart].join("+");
    }, [shortcutValue]);

    const onShortcutChange = useCallback((s: string) => {
        form.setValue("shortcut", s, { shouldDirty: true });
        // 兼容旧字段：保存第一个修饰符，便于后端回退
        const modifier = s.split("+").find((t) => ["Ctrl", "Shift", "Alt", "Super", "Command"].includes(t));
        if (modifier) {
            const map: Record<string, string> = { Ctrl: "ctrl", Shift: "shift", Alt: "alt", Super: "cmd", Command: "cmd" };
            form.setValue("modifier_key", map[modifier] || "alt");
        }
    }, [form]);

    const openRecorder = () => {
        setRecorderOpen(true);
    };

    const SHORTCUTS_FORM_CONFIG = [
        {
            key: "shortcut",
            config: {
                type: "custom" as const,
                label: "全局快捷键",
                customRender: () => (
                    <div className="space-y-2">
                        <div className="flex items-center gap-2">
                            <div className="flex-1 px-3 py-2 rounded-md border bg-muted/50 font-mono text-sm">
                                {displayText}
                            </div>
                            <button
                                type="button"
                                onClick={openRecorder}
                                className="px-3 py-2 text-sm rounded-md border hover:bg-accent"
                            >
                                录入
                            </button>
                        </div>
                        <p className="text-xs text-muted-foreground">点击“录入”按钮，在弹出的窗口按下组合键后确认</p>
                    </div>
                ),
            },
        },
    ];

    return (
        <>
            <ConfigForm
                title="快捷键"
                description="配置全局快捷键来快速唤起 Ask 窗口。如果有选中的文本，会自动捕获并填充到输入框。"
                config={SHORTCUTS_FORM_CONFIG}
                layout="default"
                classNames="bottom-space"
                useFormReturn={form}
                onSave={handleSaveShortcutsConfig}
            />
            <ShortcutRecorder
                open={recorderOpen}
                onOpenChange={setRecorderOpen}
                currentShortcut={shortcutValue}
                onShortcutChange={onShortcutChange}
            />
        </>
    );
};

export default ShortcutsConfigForm;
