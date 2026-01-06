import React, { useState, useEffect, useCallback } from "react";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Keyboard } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";

interface ShortcutRecorderProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    currentShortcut: string;
    onShortcutChange: (shortcut: string) => void;
}

export const ShortcutRecorder: React.FC<ShortcutRecorderProps> = ({
    open,
    onOpenChange,
    currentShortcut,
    onShortcutChange,
}) => {
    const [recording, setRecording] = useState(false);
    const [pressedKeys, setPressedKeys] = useState<Set<string>>(new Set());
    const [recordedShortcut, setRecordedShortcut] = useState("");
    const isMac = typeof navigator !== "undefined" && /mac/i.test(navigator.userAgent);

    useEffect(() => {
        if (open) {
            setRecordedShortcut(currentShortcut);
            setPressedKeys(new Set());
            setRecording(true);
            // 通知后端进入录入模式：忽略全局快捷键，并暂时卸载注册
            invoke("set_shortcut_recording", { active: true }).catch(() => {});
            invoke("suspend_global_shortcut").catch(() => {});
        } else {
            setRecording(false);
            invoke("set_shortcut_recording", { active: false }).catch(() => {});
            invoke("resume_global_shortcut").catch(() => {});
        }
    }, [open, currentShortcut]);

    const formatKey = (codeOrKey: string): string => {
        const k = codeOrKey;
        // Normalize common codes to readable tokens
        if (/^Key[A-Z]$/.test(k)) return k.replace(/^Key/, "");
        if (/^Digit[0-9]$/.test(k)) return k.replace(/^Digit/, "");
        if (k === "Space" || k === "Spacebar" || k === " ") return "Space";
        if (/^Control/.test(k) || k === "Control") return "Ctrl";
        if (/^Shift/.test(k) || k === "Shift") return "Shift";
        if (/^Alt/.test(k) || k === "Alt") return "Alt";
        if (/^Meta/.test(k) || k === "Meta") return "Cmd";
        return k.length === 1 ? k.toUpperCase() : k;
    };

    // 录入逻辑：必须包含修饰键 + 任意非修饰键

    const handleKeyDown = useCallback(
        (e: KeyboardEvent) => {
            if (!recording) return;

            e.preventDefault();
            e.stopPropagation();

            const key = e.key;
            const code = (e as KeyboardEvent).code || "";
            const stored = code || key;
            const newKeys = new Set(pressedKeys.size === 0 ? [] : Array.from(pressedKeys));
            newKeys.add(stored);
            setPressedKeys(newKeys);

            const hasModifier = e.altKey || e.ctrlKey || e.metaKey || e.shiftKey;
            // 忽略纯修饰键
            const isModifierOnly = key === "Shift" || key === "Control" || key === "Alt" || key === "Meta";
            if (!hasModifier || isModifierOnly) return;

            // 组装 accelerator 字符串
            const tokens: string[] = [];
            if (e.ctrlKey) tokens.push("Ctrl");
            if (e.shiftKey) tokens.push("Shift");
            if (e.altKey) tokens.push("Alt");
            if (e.metaKey) tokens.push(isMac ? "Command" : "Super");

            const normalizeKey = () => {
                if (code === "Space" || key === " ") return "Space";
                return code || key.toUpperCase();
            };
            const next = [...tokens, normalizeKey()].join("+");
            setRecordedShortcut(next);
        },
        [recording, pressedKeys, isMac]
    );

    const handleKeyUp = useCallback(
        (e: KeyboardEvent) => {
            if (!recording) return;

            e.preventDefault();
            e.stopPropagation();

            const key = e.key;
            const code = (e as KeyboardEvent).code || "";
            const stored = code || key;
            const newKeys = new Set(pressedKeys);
            newKeys.delete(stored);
            setPressedKeys(newKeys);
        },
        [recording, pressedKeys]
    );

    useEffect(() => {
        if (recording) {
            window.addEventListener("keydown", handleKeyDown);
            window.addEventListener("keyup", handleKeyUp);

            return () => {
                window.removeEventListener("keydown", handleKeyDown);
                window.removeEventListener("keyup", handleKeyUp);
            };
        }
    }, [recording, handleKeyDown, handleKeyUp]);

    const handleSave = () => {
        if (recordedShortcut) {
            onShortcutChange(recordedShortcut);
            // 恢复全局快捷键注册
            invoke("set_shortcut_recording", { active: false }).catch(() => {});
            invoke("resume_global_shortcut").catch(() => {});
            onOpenChange(false);
        }
    };

    const handleCancel = () => {
        setRecording(false);
        setPressedKeys(new Set());
        // 恢复全局快捷键注册
        invoke("set_shortcut_recording", { active: false }).catch(() => {});
        invoke("resume_global_shortcut").catch(() => {});
        onOpenChange(false);
    };

    const displayKeys = Array.from(pressedKeys).map(formatKey).join(" + ");

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-[425px]">
                <DialogHeader>
                    <DialogTitle>录入快捷键</DialogTitle>
                    <DialogDescription>在窗口内按下你想设置的快捷键组合，然后点击保存</DialogDescription>
                </DialogHeader>

                <div className="py-6">
                    <div className="flex flex-col items-center gap-4">
                        {/* 显示区域 */}
                        <div className="w-full min-h-[100px] border-2 border-dashed rounded-lg flex items-center justify-center bg-muted/50">
                            {recording ? (
                                <div className="flex flex-col items-center gap-2 text-center px-4">
                                    <Keyboard className="h-8 w-8 text-muted-foreground animate-pulse" />
                                    <p className="text-sm text-muted-foreground">当前捕获：{recordedShortcut || "未捕获"}</p>
                                    <p className="text-xs text-muted-foreground">按下中：{displayKeys || "无"}</p>
                                    <p className="text-xs text-muted-foreground">提示：请按下 修饰键 + 任意键，然后松开以确认本次组合</p>
                                </div>
                            ) : (
                                <div className="flex flex-col items-center gap-2">
                                    <Keyboard className="h-8 w-8 text-muted-foreground" />
                                    {recordedShortcut ? (
                                        <p className="text-lg font-medium">
                                            {recordedShortcut}
                                        </p>
                                    ) : (
                                        <p className="text-sm text-muted-foreground">未设置快捷键</p>
                                    )}
                                </div>
                            )}
                        </div>

                        {/* 提示信息 */}
                        <div className="w-full text-xs text-muted-foreground space-y-1">
                            <p>• 支持的修饰键：Alt/Option、Ctrl、Shift、Cmd</p>
                            <p>• 快捷键格式：修饰键 + 任意键（如 Ctrl+Shift+I 或 Alt+Space）</p>
                            <p>• macOS 推荐使用 Option+Space，Windows 推荐使用 Alt+Space（若被系统占用，请选择其他组合）</p>
                        </div>
                    </div>
                </div>

                <DialogFooter>
                    <Button variant="outline" onClick={handleCancel}>
                        取消
                    </Button>
                    <Button onClick={handleSave} disabled={!recordedShortcut}>
                        保存
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
};
