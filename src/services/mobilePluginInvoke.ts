import { invoke } from "@tauri-apps/api/core";

/** One bridge for typed and generic SDK commands. Platform lookup failures propagate. */
export function createPluginInvoke(pluginId: number) {
    let platform: Promise<string> | undefined;
    return async <T = unknown>(command: string, args?: Record<string, unknown>): Promise<T> => {
        platform ??= invoke<string>("get_platform");
        const current = await platform;
        if (current === "android" || current === "ios") {
            await invoke("validate_mobile_plugin_command", { pluginId, command });
            return invoke<T>(command, { ...args, pluginId });
        }
        if (!["windows", "macos", "linux"].includes(current)) throw new Error(`plugin_id=${pluginId}; stage=platform; 未识别运行平台 ${current}`);
        return invoke<T>(command, args ?? {});
    };
}
