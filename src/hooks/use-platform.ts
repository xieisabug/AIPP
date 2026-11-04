import * as React from "react"
import { invoke } from "@tauri-apps/api/core"

/**
 * 运行平台类型（由 Rust 侧 `get_platform` 命令返回）
 */
export type AppPlatform = "windows" | "macos" | "linux" | "android" | "ios" | "unknown"

/**
 * 平台三态：
 * - "mobile" / "desktop"：已由 `get_platform` 确认（或 WebView UA 明确是移动端）
 * - "unknown"：尚未确认。调用方按桌面行为渲染，保证桌面不闪烁；
 *   移动端 WebView 的 UA 带 Android/iPhone/iPad，首帧即为 "mobile"，不会先露出桌面入口。
 */
export type PlatformKind = "mobile" | "desktop" | "unknown"

// 模块级缓存：平台在运行期不会变化，只请求一次；解析后同步可读
let platformPromise: Promise<AppPlatform> | null = null
let resolvedPlatform: AppPlatform | null = null

/** 移动平台时在 <html> 上挂 `aipp-mobile-platform`，供纯 CSS 的触控尺寸规则使用（桌面永远不挂）。 */
export const MOBILE_PLATFORM_HTML_CLASS = "aipp-mobile-platform"

function syncMobilePlatformHtmlClass(platform: AppPlatform | null): void {
  if (typeof document === "undefined") return
  document.documentElement.classList.toggle(MOBILE_PLATFORM_HTML_CLASS, isMobileAppPlatform(platform))
}

function fetchPlatform(): Promise<AppPlatform> {
  if (!platformPromise) {
    platformPromise = invoke<string>("get_platform")
      .then((p) => p as AppPlatform)
      .catch(() => "unknown" as AppPlatform)
      .then((p) => {
        resolvedPlatform = p
        syncMobilePlatformHtmlClass(p)
        return p
      })
  }
  return platformPromise
}

/** 首帧同步猜测：只在 UA 明确是移动 WebView 时给出移动平台，否则保持未知（null）。 */
function guessPlatformFromUserAgent(): AppPlatform | null {
  if (typeof navigator === "undefined") return null
  const ua = navigator.userAgent || ""
  if (/Android/i.test(ua)) return "android"
  if (/iPhone|iPad|iPod/i.test(ua)) return "ios"
  return null
}

// 首帧：移动 WebView 立即挂上 class，避免先按桌面尺寸绘制
syncMobilePlatformHtmlClass(guessPlatformFromUserAgent())

function initialPlatform(): AppPlatform | null {
  return resolvedPlatform ?? guessPlatformFromUserAgent()
}

export function isMobileAppPlatform(platform: AppPlatform | null | undefined): boolean {
  return platform === "android" || platform === "ios"
}

export function toPlatformKind(platform: AppPlatform | null | undefined): PlatformKind {
  if (platform == null || platform === "unknown") return "unknown"
  return isMobileAppPlatform(platform) ? "mobile" : "desktop"
}

/**
 * 获取当前运行平台（移动平台 vs 桌面平台）。未确认前返回 null（移动 WebView 首帧返回 UA 推断值）。
 *
 * 注意语义区分：
 * - 本 hook 回答"是不是移动平台"，用于禁用桌面专属功能（托盘/快捷键/自动更新/脚本执行等）。
 * - `useIsMobile()`（use-mobile.ts）回答"是不是窄屏"，仅用于响应式布局，不要用来做平台判断。
 */
export function usePlatform(): AppPlatform | null {
  const [platform, setPlatform] = React.useState<AppPlatform | null>(initialPlatform)

  React.useEffect(() => {
    let cancelled = false
    fetchPlatform().then((p) => {
      if (!cancelled) setPlatform(p)
    })
    return () => {
      cancelled = true
    }
  }, [])

  return platform
}

/** 三态平台值，见 {@link PlatformKind}。 */
export function usePlatformKind(): PlatformKind {
  return toPlatformKind(usePlatform())
}

/**
 * 是否移动平台（android / ios）。未确认（unknown）时返回 false，即按桌面渲染。
 */
export function useIsMobilePlatform(): boolean {
  return usePlatformKind() === "mobile"
}

/** 仅供测试：重置模块缓存。 */
export function __resetPlatformCacheForTests(): void {
  platformPromise = null
  resolvedPlatform = null
}
