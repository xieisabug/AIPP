import * as React from "react"

const MOBILE_BREAKPOINT = 768

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
