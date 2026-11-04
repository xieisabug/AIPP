import * as React from "react"

const MOBILE_BREAKPOINT = 768

/**
 * 是否窄屏（窗口宽度 < 768px），仅用于响应式布局切换。
 *
 * 注意：这不是"移动平台"判断——桌面窗口拖窄也会返回 true。
 * 需要区分移动平台（禁用桌面功能、平台差异行为）时请用 `usePlatform()` / `useIsMobilePlatform()`（use-platform.ts）。
 */
export function useIsMobile() {
  // 初始值直接根据窗口宽度判断，避免初始渲染时的 undefined 导致布局切换
  const [isMobile, setIsMobile] = React.useState<boolean>(() => {
    if (typeof window !== 'undefined') {
      return window.innerWidth < MOBILE_BREAKPOINT
    }
    return false
  })

  React.useEffect(() => {
    const mql = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`)
    const onChange = () => {
      setIsMobile(window.innerWidth < MOBILE_BREAKPOINT)
    }
    mql.addEventListener("change", onChange)
    // 确保初始值正确
    setIsMobile(window.innerWidth < MOBILE_BREAKPOINT)
    return () => mql.removeEventListener("change", onChange)
  }, [])

  return isMobile
}
