import { useIsMobilePlatform } from "@/hooks/use-platform"

/**
 * 移动端（android / ios）不支持的功能中央清单。
 *
 * 用途：前端按平台隐藏对应入口，而不是等用户点了才在运行期报错。
 * 桌面平台一律视为可用，这里只列出移动端的限制。
 */
export type MobileUnsupportedFeature =
    // 开机自启动：Rust 侧 get_autostart_state / set_autostart 在移动端直接返回 Err
    | "autostart"
    // 全局快捷键：Rust 侧仅桌面平台编译注册（global shortcut 插件无移动端实现）
    | "global_shortcut"
    // 应用内更新：Rust 侧 check_update / download_and_install_update 在移动端返回 Err
    | "app_updater"
    // 脚本/代码执行：Android 无 shell 环境，代码块"运行"按钮（run_artifacts 执行脚本类语言）运行期必败
    | "script_execution"
    // 浏览器模式搜索：chromiumoxide 仅桌面编译，搜索工具中浏览器相关的配置项在移动端无意义
    | "browser_search"

export const MOBILE_UNSUPPORTED_FEATURES: ReadonlySet<MobileUnsupportedFeature> = new Set([
    "autostart",
    "global_shortcut",
    "app_updater",
    "script_execution",
    "browser_search",
])

/**
 * 内置 search 工具中依赖浏览器（chromiumoxide，仅桌面编译）的环境变量。
 * 移动端配置界面隐藏这些字段，保留搜索引擎等普通配置。
 */
export const BROWSER_SEARCH_ENV_KEYS: ReadonlySet<string> = new Set([
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
    if (!isMobile) return true
    return !MOBILE_UNSUPPORTED_FEATURES.has(feature)
}

/**
 * 与 Rust 侧 `system_api::MOBILE_UNSUPPORTED_PREFIX` 保持一致。
 * 后端对移动端不支持的功能统一返回此前缀开头的错误。
 */
export const MOBILE_UNSUPPORTED_ERROR_PREFIX = "MOBILE_UNSUPPORTED"

/**
 * 判断一个后端错误是否为"移动端不支持"错误，便于前端统一提示。
 */
export function isMobileUnsupportedError(error: unknown): boolean {
    const message =
        typeof error === "string" ? error : error instanceof Error ? error.message : String(error)
    return message.startsWith(MOBILE_UNSUPPORTED_ERROR_PREFIX)
}
