import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { ConfigPageLoading } from "@/components/common";
import { ShortcutsConfigForm } from "@/components/config/feature/forms/ShortcutsConfigForm";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { useUnsavedLeaveGuard } from "@/hooks/useUnsavedLeaveGuard";
import { APP_SHORTCUT_KEY_PREFIX, SHORTCUT_ACTIONS } from "@/data/Shortcuts";
import SettingsPageShell from "./SettingsPageShell";

export const ShortcutsPageConfig: React.FC = () => {
    const { featureConfig, saveFeatureConfig, loading } = useFeatureConfig();
    const [initialLoaded, setInitialLoaded] = useState(false);
    const baselineRef = useRef<Record<string, string>>({});

    const isMac =
        typeof navigator !== "undefined" &&
        navigator.userAgent.toLowerCase().includes("mac");

    const defaultAppShortcutValues = useMemo(() => {
        const values: Record<string, string> = {};
        for (const action of SHORTCUT_ACTIONS) {
            values[APP_SHORTCUT_KEY_PREFIX + action.id] = action.defaultShortcut;
        }
        return values;
    }, []);

    const form = useForm<Record<string, string>>({
        defaultValues: {
            shortcut: isMac ? "Option+Space" : "Alt+Space",
            modifier_key: isMac ? "option" : "alt",
            ...defaultAppShortcutValues,
        },
    });

    useEffect(() => {
        if (loading) return;
        const shortcutsConfig = featureConfig.get("shortcuts") ?? new Map<string, string>();
        const shortcut = shortcutsConfig.get("shortcut");
        const modifier_key = shortcutsConfig.get("modifier_key") || (isMac ? "option" : "alt");
        const fallbackShortcut = (() => {
            const mk = modifier_key.toLowerCase();
            if (mk === "ctrl" || mk === "control") return "Ctrl+Space";
            if (mk === "shift") return "Shift+Space";
            if (mk === "cmd" || mk === "command" || mk === "super") {
                return isMac ? "Command+Space" : "Super+Space";
            }
            return isMac ? "Option+Space" : "Alt+Space";
        })();
        const nextValues: Record<string, string> = {
            shortcut: shortcut || fallbackShortcut,
            modifier_key,
            ...defaultAppShortcutValues,
        };
        for (const action of SHORTCUT_ACTIONS) {
            const key = APP_SHORTCUT_KEY_PREFIX + action.id;
            nextValues[key] = shortcutsConfig.get(key) || action.defaultShortcut;
        }
        baselineRef.current = { ...nextValues };
        form.reset(nextValues);
        setInitialLoaded(true);
    }, [loading, featureConfig, defaultAppShortcutValues, form, isMac]);

    const watched = form.watch();
    const isDirty = initialLoaded && JSON.stringify(watched) !== JSON.stringify(baselineRef.current);

    const handleSave = useCallback(async () => {
        const v = form.getValues();
        const appShortcutConfig = SHORTCUT_ACTIONS.reduce<Record<string, string>>((acc, action) => {
            const key = APP_SHORTCUT_KEY_PREFIX + action.id;
            acc[key] = v[key] || action.defaultShortcut;
            return acc;
        }, {});
        const payload = {
            shortcut: v.shortcut,
            modifier_key: v.modifier_key,
            ...appShortcutConfig,
        };
        await saveFeatureConfig("shortcuts", payload);
        baselineRef.current = { ...payload };
    }, [form, saveFeatureConfig]);

    const handleDiscard = useCallback(() => {
        form.reset({ ...baselineRef.current });
    }, [form]);

    const { dialog: leaveDialog } = useUnsavedLeaveGuard({
        isDirty,
        onSave: handleSave,
        onDiscard: handleDiscard,
    });

    if (!initialLoaded) {
        return <ConfigPageLoading />;
    }

    const showButlerShortcuts =
        featureConfig.get("experimental")?.get("butler_experiment_enabled") === "true";

    return (
        <>
            <SettingsPageShell
                title="快捷键"
                description="配置全局唤起快捷键及各窗口内的键盘快捷操作，点击快捷键格即可重新录入。"
                headerExtra={
                    isDirty ? (
                        <span className="rounded-full border border-foreground/30 bg-muted px-2.5 py-0.5 text-xs font-medium">
                            未保存
                        </span>
                    ) : null
                }
            >
                <ShortcutsConfigForm
                    form={form}
                    onSave={handleSave}
                    showButlerShortcuts={showButlerShortcuts}
                />
            </SettingsPageShell>
            {leaveDialog}
        </>
    );
};

export default ShortcutsPageConfig;
