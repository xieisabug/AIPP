/**
 * 应用内快捷键数据模型与动作注册表
 */

export type ShortcutWindow = "ask" | "chat";

export interface ShortcutAction {
    /** 唯一标识，格式: window.action */
    id: string;
    /** 显示名称 */
    label: string;
    /** 所属窗口 */
    window: ShortcutWindow;
    /** 默认快捷键（使用 Mod 表示 Ctrl/Cmd） */
    defaultShortcut: string;
}

/**
 * 所有可配置快捷键的动作注册表
 *
 * 快捷键格式说明:
 * - "Mod" 在 macOS 上映射为 Cmd，在 Windows/Linux 上映射为 Ctrl
 * - 组合键用 "+" 连接，如 "Mod+Shift+F"
 */
export const SHORTCUT_ACTIONS: ShortcutAction[] = [
    // Ask 窗口
    {
        id: "ask.new",
        label: "新建对话",
        window: "ask",
        defaultShortcut: "Mod+N",
    },
    {
        id: "ask.fullscreen",
        label: "全屏打开",
        window: "ask",
        defaultShortcut: "Mod+Shift+F",
    },
    {
        id: "ask.copy",
        label: "复制回复",
        window: "ask",
        defaultShortcut: "Mod+Shift+C",
    },
    {
        id: "ask.settings",
        label: "打开设置",
        window: "ask",
        defaultShortcut: "Mod+Comma",
    },
    // Chat 窗口
    {
        id: "chat.new",
        label: "新建对话",
        window: "chat",
        defaultShortcut: "Mod+N",
    },
    {
        id: "chat.search",
        label: "搜索",
        window: "chat",
        defaultShortcut: "Mod+F",
    },
    {
        id: "chat.stats",
        label: "查看消耗",
        window: "chat",
        defaultShortcut: "Mod+Shift+I",
    },
    {
        id: "chat.export",
        label: "导出",
        window: "chat",
        defaultShortcut: "Mod+E",
    },
    {
        id: "chat.settings",
        label: "打开设置",
        window: "chat",
        defaultShortcut: "Mod+Comma",
    },
    {
        id: "chat.toggle_sidebar",
        label: "切换侧边栏",
        window: "chat",
        defaultShortcut: "Mod+B",
    },
    {
        id: "chat.open_sidebar_window",
        label: "侧边详情窗口",
        window: "chat",
        defaultShortcut: "Mod+Shift+B",
    },
];

/** 按窗口分组获取动作列表 */
export function getActionsByWindow(
    window: ShortcutWindow
): ShortcutAction[] {
    return SHORTCUT_ACTIONS.filter((a) => a.window === window);
}

/** 获取动作的默认快捷键 Map: actionId → defaultShortcut */
export function getDefaultShortcutMap(): Record<string, string> {
    const map: Record<string, string> = {};
    for (const action of SHORTCUT_ACTIONS) {
        map[action.id] = action.defaultShortcut;
    }
    return map;
}

/** feature_config 中存储应用快捷键的 key 前缀 */
export const APP_SHORTCUT_KEY_PREFIX = "app.";

/** 将 actionId 转换为 feature_config key */
export function actionIdToConfigKey(actionId: string): string {
    return APP_SHORTCUT_KEY_PREFIX + actionId;
}

/** 将 feature_config key 转换为 actionId */
export function configKeyToActionId(configKey: string): string | null {
    if (configKey.startsWith(APP_SHORTCUT_KEY_PREFIX)) {
        return configKey.slice(APP_SHORTCUT_KEY_PREFIX.length);
    }
    return null;
}

/** 窗口显示名称 */
export const WINDOW_LABELS: Record<ShortcutWindow, string> = {
    ask: "Ask 窗口",
    chat: "Chat 窗口",
};
