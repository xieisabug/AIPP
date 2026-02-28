import { useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
    SHORTCUT_ACTIONS,
    APP_SHORTCUT_KEY_PREFIX,
    type ShortcutWindow,
} from "@/data/Shortcuts";

interface FeatureConfigListItem {
    id: number;
    feature_code: string;
    key: string;
    value: string;
}

const isMac =
    typeof navigator !== "undefined" && /mac/i.test(navigator.userAgent);

/**
 * 将存储格式的快捷键字符串解析为匹配用的标准化 key set
 * 例如 "Mod+Shift+F" → { ctrl/meta: true, shift: true, key: "f" }
 */
interface ParsedShortcut {
    ctrl: boolean;
    shift: boolean;
    alt: boolean;
    meta: boolean;
    key: string; // 小写的普通键
}

function parseShortcut(shortcut: string): ParsedShortcut | null {
    if (!shortcut) return null;
    const parts = shortcut.split("+");
    const result: ParsedShortcut = {
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
        key: "",
    };
    for (const part of parts) {
        const p = part.trim();
        switch (p) {
            case "Mod":
                if (isMac) result.meta = true;
                else result.ctrl = true;
                break;
            case "Ctrl":
                result.ctrl = true;
                break;
            case "Shift":
                result.shift = true;
                break;
            case "Alt":
                result.alt = true;
                break;
            case "Meta":
            case "Cmd":
            case "Command":
                result.meta = true;
                break;
            case "Super":
                if (isMac) result.meta = true;
                else result.ctrl = true;
                break;
            case "Comma":
                result.key = ",";
                break;
            case "Period":
                result.key = ".";
                break;
            case "Space":
                result.key = " ";
                break;
            default:
                // 处理 KeyA → a, Digit1 → 1 等格式
                if (/^Key[A-Z]$/.test(p)) {
                    result.key = p.slice(3).toLowerCase();
                } else if (/^Digit[0-9]$/.test(p)) {
                    result.key = p.slice(5);
                } else {
                    result.key = p.toLowerCase();
                }
                break;
        }
    }
    return result.key ? result : null;
}

function matchesEvent(parsed: ParsedShortcut, e: KeyboardEvent): boolean {
    if (e.ctrlKey !== parsed.ctrl) return false;
    if (e.shiftKey !== parsed.shift) return false;
    if (e.altKey !== parsed.alt) return false;
    if (e.metaKey !== parsed.meta) return false;

    // 匹配按键
    const eventKey = e.key.toLowerCase();
    if (eventKey === parsed.key) return true;

    // 也尝试用 code 匹配（适配不同键盘布局）
    const code = e.code.toLowerCase();
    if (parsed.key.length === 1) {
        const letter = parsed.key;
        if (code === `key${letter}` || code === `digit${letter}`) return true;
    }

    return false;
}

type ShortcutHandlers = Partial<Record<string, () => void>>;

/**
 * 应用内快捷键 Hook
 *
 * @param windowType 当前窗口类型
 * @param handlers 动作 ID → 回调函数的映射，key 为不带窗口前缀的动作名
 *   例如: { new: () => {}, search: () => {} }
 *
 * @example
 * ```tsx
 * useAppShortcuts("chat", {
 *   new: handleNewConversation,
 *   search: () => setSearchOpen(true),
 *   settings: openConfig,
 * });
 * ```
 */
export function useAppShortcuts(
    windowType: ShortcutWindow,
    handlers: ShortcutHandlers
) {
    const handlersRef = useRef(handlers);
    handlersRef.current = handlers;

    // 从 feature_config 加载用户自定义快捷键
    const shortcutMapRef = useRef<Map<string, ParsedShortcut>>(new Map());

    const windowActions = useMemo(
        () => SHORTCUT_ACTIONS.filter((a) => a.window === windowType),
        [windowType]
    );

    // 构建快捷键映射
    const buildShortcutMap = useCallback(
        (configMap?: Map<string, string>) => {
            const map = new Map<string, ParsedShortcut>();
            for (const action of windowActions) {
                // actionId 如 "chat.search"，handler key 为 "search"
                const actionSuffix = action.id.split(".").slice(1).join(".");
                const configKey = APP_SHORTCUT_KEY_PREFIX + action.id;
                const customShortcut = configMap?.get(configKey);
                const shortcutStr = customShortcut || action.defaultShortcut;
                const parsed = parseShortcut(shortcutStr);
                if (parsed) {
                    map.set(actionSuffix, parsed);
                }
            }
            shortcutMapRef.current = map;
        },
        [windowActions]
    );

    // 加载配置
    useEffect(() => {
        invoke<FeatureConfigListItem[]>("get_all_feature_config")
            .then((list) => {
                const configMap = new Map<string, string>();
                for (const item of list) {
                    if (item.feature_code === "shortcuts") {
                        configMap.set(item.key, item.value);
                    }
                }
                buildShortcutMap(configMap);
            })
            .catch(() => {
                // 加载失败时使用默认快捷键
                buildShortcutMap();
            });
    }, [buildShortcutMap]);

    // 监听配置变更事件
    useEffect(() => {
        const unlisten = listen("feature_config_changed", () => {
            invoke<FeatureConfigListItem[]>("get_all_feature_config")
                .then((list) => {
                    const configMap = new Map<string, string>();
                    for (const item of list) {
                        if (item.feature_code === "shortcuts") {
                            configMap.set(item.key, item.value);
                        }
                    }
                    buildShortcutMap(configMap);
                })
                .catch(() => {});
        });
        return () => {
            unlisten.then((fn) => fn());
        };
    }, [buildShortcutMap]);

    // 键盘事件监听
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // 忽略输入框中的纯字符按键（不带修饰键的情况）
            const target = e.target as HTMLElement;
            const isInput =
                target.tagName === "INPUT" ||
                target.tagName === "TEXTAREA" ||
                target.isContentEditable;

            const map = shortcutMapRef.current;
            for (const [actionSuffix, parsed] of map) {
                if (matchesEvent(parsed, e)) {
                    // 输入框中只响应带修饰键的快捷键
                    if (isInput && !e.ctrlKey && !e.metaKey && !e.altKey) {
                        continue;
                    }
                    const handler = handlersRef.current[actionSuffix];
                    if (handler) {
                        e.preventDefault();
                        e.stopPropagation();
                        handler();
                        return;
                    }
                }
            }
        };

        window.addEventListener("keydown", handleKeyDown);
        return () => window.removeEventListener("keydown", handleKeyDown);
    }, []);
}

/**
 * 获取快捷键的显示文本
 * 将 "Mod+Shift+F" 转换为 "⌘⇧F" (macOS) 或 "Ctrl+Shift+F" (Windows)
 */
export function formatShortcutDisplay(shortcut: string): string {
    if (!shortcut) return "";
    const parts = shortcut.split("+");
    const display: string[] = [];
    for (const part of parts) {
        const p = part.trim();
        switch (p) {
            case "Mod":
                display.push(isMac ? "⌘" : "Ctrl");
                break;
            case "Ctrl":
                display.push(isMac ? "⌃" : "Ctrl");
                break;
            case "Shift":
                display.push(isMac ? "⇧" : "Shift");
                break;
            case "Alt":
                display.push(isMac ? "⌥" : "Alt");
                break;
            case "Meta":
            case "Cmd":
            case "Command":
                display.push(isMac ? "⌘" : "Win");
                break;
            case "Comma":
                display.push(",");
                break;
            case "Period":
                display.push(".");
                break;
            case "Space":
                display.push("Space");
                break;
            default:
                if (/^Key[A-Z]$/.test(p)) {
                    display.push(p.slice(3));
                } else if (/^Digit[0-9]$/.test(p)) {
                    display.push(p.slice(5));
                } else {
                    display.push(p.toUpperCase());
                }
                break;
        }
    }
    return isMac ? display.join(" ") : display.join(" + ");
}
