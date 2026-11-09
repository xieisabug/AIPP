import { useIsMobilePlatform, usePlatform } from "@/hooks/use-platform"

/**
 * 移动端（android / ios）不支持的功能中央清单。
 *
 * 用途：前端按平台隐藏对应入口，而不是等用户点了才在运行期报错。
 * 桌面平台一律视为可用，这里只列出移动端的限制。
 *
 * 与 PRD §3.1 A 类 / MOB-02 对齐：能力判断不得依赖窗口宽度或 MCP transport 字符串一刀切；
 * aipp:* 进程内工具即使登记为 stdio 也必须单独识别，不能与外部 stdio 一起误禁。
 */
export type MobileUnsupportedFeature =
    // 开机自启动：Rust 侧 get_autostart_state / set_autostart 在移动端直接返回 Err
    | "autostart"
    // 全局快捷键：Rust 侧仅桌面平台编译注册（global shortcut 插件无移动端实现）
    | "global_shortcut"
    // 应用内更新：Rust 侧 check_update / download_and_install_update 在移动端返回 Err
    | "app_updater"
    // 脚本/代码执行：Android 无 shell 环境，代码块"运行"按钮与 aipp:operation execute_bash 运行期必败
    | "script_execution"
    // 浏览器模式搜索：chromiumoxide 仅桌面编译，搜索工具中浏览器相关的配置项在移动端无意义
    | "browser_search"
    // 外部 stdio MCP：需要本机进程；不含 aipp:* 内置
    | "external_stdio_mcp"
    // 本地 CLI Agent：Codex app-server / Claude Code / ACP CLI
    | "local_cli_agent"
    // 桌面多窗口：Schedule / Artifact / Butler / Plugin / Sidebar 等独立 WebView
    | "desktop_multi_window"
    // 桌面 React/Vue 预览运行时（Bun/Vite）。移动端改用 src/mobile/previewRuntime.ts 的隔离 iframe
    // 运行时（MobileArtifactPreview），不经过此标记；当前无前端调用点检查此项。
    | "react_vue_preview"
    // 桌面 PDF 导出（export_api 在移动端返回 MOBILE_UNSUPPORTED）。Android 由
    // conversationExportService.printMobilePdf 走系统打印；当前无前端调用点检查此项。
    | "pdf_export"
    // 桌面剪贴板图片命令（copy_image_to_clipboard 在移动端返回 MOBILE_UNSUPPORTED）。Android 由
    // LazyImage 经 mobileFileService 的 copyImage 原生桥接实现；当前无前端调用点检查此项。
    | "clipboard_image"
    // Skills 管理：清单内默认不可用，但 Android 已由 MobileSkillsManager 提供应用内安装/查看/绑定，
    // useFeatureAvailableOnPlatform 对 android 单独放行（iOS 仍隐藏）。
    | "skills_manager"
    // 进程型插件（M2）；当前无前端调用点检查此项，插件能力由 plugin_api 按插件逐项返回 MOBILE_UNSUPPORTED
    | "process_plugin"

export const MOBILE_UNSUPPORTED_FEATURES: ReadonlySet<MobileUnsupportedFeature> = new Set([
    "autostart",
    "global_shortcut",
    "app_updater",
    "script_execution",
    "browser_search",
    "external_stdio_mcp",
    "local_cli_agent",
    "desktop_multi_window",
    "react_vue_preview",
    "pdf_export",
    "clipboard_image",
    "skills_manager",
    "process_plugin",
])

/**
 * 内置 search 工具中依赖浏览器（chromiumoxide，仅桌面编译）的环境变量。
 * 移动端配置界面隐藏这些字段，保留搜索引擎等普通配置。
 */
export const BROWSER_SEARCH_ENV_KEYS: ReadonlySet<string> = new Set([
    "SEARCH_ENGINE",
    "PROXY_SERVER",
    "KAGI_SESSION_URL",
    "USER_DATA_DIR",
    "HEADLESS",
    "WAIT_SELECTORS",
    "WAIT_TIMEOUT_MS",
    "WAIT_POLL_MS",
    "MAX_CONCURRENT_PAGES",
    "POOL_ACQUIRE_TIMEOUT_MS",
    "DEBUG_CAPTURE_EMPTY_ARTIFACTS",
    "DEBUG_ARTIFACT_DIR",
])

/**
 * 判断某功能在当前平台是否可用。
 * 桌面平台一律返回 true；移动平台对清单内功能返回 false。
 */
export function useFeatureAvailableOnPlatform(feature: MobileUnsupportedFeature): boolean {
    const isMobile = useIsMobilePlatform()
    const platform = usePlatform()
    if (!isMobile) return true
    // Android 有应用内 Skills 管理（MobileSkillsManager），见上方 skills_manager 注释
    if (platform === "android" && feature === "skills_manager") return true
    return !MOBILE_UNSUPPORTED_FEATURES.has(feature)
}

/**
 * 与 Rust 侧 `system_api::MOBILE_UNSUPPORTED_PREFIX` 保持一致。
 * 后端对移动端不支持的功能统一返回此前缀开头的错误。
 */
export const MOBILE_UNSUPPORTED_ERROR_PREFIX = "MOBILE_UNSUPPORTED"

/**
 * 判断一个后端错误是否为"移动端不支持"错误，便于前端统一提示。
 * 兼容 AppError 包装后的文案（前缀不一定在字符串最开头）。
 */
export function isMobileUnsupportedError(error: unknown): boolean {
    const message =
        typeof error === "string" ? error : error instanceof Error ? error.message : String(error)
    return message.includes(MOBILE_UNSUPPORTED_ERROR_PREFIX)
}

/** 是否 aipp:* 进程内内置命令（即使 transport=stdio 也不走外部进程）。 */
export function isAippBuiltinCommand(command: string | null | undefined): boolean {
    if (!command) return false
    return command.trim().startsWith("aipp:")
}

/**
 * 移动端是否允许执行该 MCP 服务器。
 * - HTTP：允许
 * - aipp:* 内置（不论 transport）：允许走进程内实现
 * - 外部 stdio / 其它：不允许
 */
export function isMobileExecutableMcpServer(server: {
    transport_type?: string | null
    command?: string | null
    is_builtin?: boolean | null
}): boolean {
    const transport = server.transport_type || ""
    if (transport === "http" || transport === "sse") return true
    return (transport === "stdio" || transport === "builtin") &&
        MOBILE_BUILTIN_COMMANDS.has(server.command?.trim() ?? "")
}

const MOBILE_BUILTIN_COMMANDS: ReadonlySet<string> = new Set([
    "aipp:search", "aipp:operation", "aipp:artifact", "aipp:dynamic_mcp",
    "aipp:ui_interaction", "aipp:agent", "aipp:superadmin",
])

export interface AssistantExecutionCapability {
    available: boolean
    issues: Array<{ code: string; capability: string; reason: string }>
}

/** 移动端 MCP 不可执行标记文案（侧栏列表与窄屏下拉共用）。 */
export const MOBILE_UNEXECUTABLE_LABEL = "此设备不可执行"

/**
 * MCP 服务器在列表/下拉中的显示名：移动平台且不可执行时追加「（此设备不可执行）」。
 * 桌面平台原样返回名称。
 */
export function getMcpServerDisplayLabel(
    server: { name: string } & Parameters<typeof isMobileExecutableMcpServer>[0],
    isMobilePlatform: boolean,
): string {
    if (isMobilePlatform && !isMobileExecutableMcpServer(server)) {
        return `${server.name}（${MOBILE_UNEXECUTABLE_LABEL}）`
    }
    return server.name
}

/** 普通 API 对话助手（非本地 CLI Agent、非插件类型）。assistant_type 0 或未设置。 */
export function isMobileExecutableAssistantType(assistantType: number | null | undefined): boolean {
    // 0 = 普通对话；4 = Codex/Claude/ACP 本地 Agent；其它为插件类型（M2）
    return assistantType === 0 || assistantType == null
}

/**
 * aipp:* 内置服务器中，移动端仍不可执行的单个工具（需要浏览器或 shell）。
 * 其余内置工具（含 aipp:search 的 HTTP fetch_url）在移动端可用。
 */
const MOBILE_UNEXECUTABLE_BUILTIN_TOOLS: Readonly<Record<string, ReadonlySet<string>>> = {
    "aipp:search": new Set(["search_web"]),
    "aipp:operation": new Set(["execute_bash", "get_bash_output", "kill_bash"]),
    "aipp:artifact": new Set(["capture_artifact_screenshot"]),
}

export function isMobileUnexecutableBuiltinTool(
    command: string | null | undefined,
    toolName: string,
): boolean {
    if (!command) return false
    const key = command.trim().split(/\s+/)[0]
    return MOBILE_UNEXECUTABLE_BUILTIN_TOOLS[key]?.has(toolName) ?? false
}


/** Presentation-only filtering; never changes persisted desktop tool settings. */
export function visibleSearchTools<T extends { name?: string; tool_name?: string }>(
    tools: T[], command: string | null | undefined, isMobilePlatform: boolean,
): T[] {
    if (!isMobilePlatform || command?.trim().split(/\s+/)[0] !== "aipp:search") return tools;
    return tools.filter(tool => !isMobileUnexecutableBuiltinTool(command, tool.tool_name ?? tool.name ?? ""));
}
