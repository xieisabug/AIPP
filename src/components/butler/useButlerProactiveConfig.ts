import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getErrorMessage } from "@/utils/error";
import type { ProactiveConfig } from "./proactiveApi";

/**
 * 主动整理设置的读取与保存。设置弹窗共用一个「保存配置」按钮，
 * 这里只在内容相对上次读取/保存有变化时才写入：开启状态下保存会做设备登记，
 * 不应因为用户只改了别的总管家设置就触发（登记失败还会拦住整次保存）。
 */
export function useButlerProactiveConfig() {
    const [config, setConfig] = useState<ProactiveConfig>();
    const [saved, setSaved] = useState<ProactiveConfig>();
    const [error, setError] = useState<string>();
    const latest = useRef<{ config?: ProactiveConfig; saved?: ProactiveConfig }>({});
    latest.current = { config, saved };

    const reload = useCallback(async () => {
        try {
            const next = await invoke<ProactiveConfig>("get_butler_proactive_config");
            setConfig(next);
            setSaved(next);
            setError(undefined);
        } catch (e) {
            setError(`读取主动整理设置失败：${getErrorMessage(e)}`);
        }
    }, []);

    useEffect(() => {
        void reload();
    }, [reload]);

    const save = useCallback(async () => {
        const { config: next, saved: previous } = latest.current;
        if (!next || JSON.stringify(next) === JSON.stringify(previous)) return;
        await invoke<void>("save_butler_proactive_config", { settings: next });
        setSaved(next);
    }, []);

    return { config, setConfig, saved, error, reload, save };
}
