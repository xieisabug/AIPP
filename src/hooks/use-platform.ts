import * as React from "react"
import { invoke } from "@tauri-apps/api/core"

/**
 * 运行平台类型（由 Rust 侧 `get_platform` 命令返回）
 */
export type AppPlatform = "windows" | "macos" | "linux" | "android" | "ios" | "unknown"

// 模块级缓存：平台在运行期不会变化，只请求一次
let platformPromise: Promise<AppPlatform> | null = null

function fetchPlatform(): Promise<AppPlatform> {
  if (!platformPromise) {
    platformPromise = invoke<string>("get_platform")
      .then((p) => p as AppPlatform)
      .catch(() => "unknown" as AppPlatform)
  }
  return platformPromise
}

/**
 * 获取当前运行平台（移动平台 vs 桌面平台）。
 *
 * 注意语义区分：
 * - 本 hook 回答"是不是移动平台"，用于禁用桌面专属功能（托盘/快捷键/自动更新/脚本执行等）。
 * - `useIsMobile()`（use-mobile.ts）回答"是不是窄屏"，仅用于响应式布局，不要用来做平台判断。
 */
export function usePlatform(): AppPlatform | null {
  const [platform, setPlatform] = React.useState<AppPlatform | null>(null)

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

/**
 * 是否移动平台（android / ios）。加载完成前返回 false。
 */
export function useIsMobilePlatform(): boolean {
  const platform = usePlatform()
  return platform === "android" || platform === "ios"
}
