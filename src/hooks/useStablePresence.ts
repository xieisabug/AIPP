import { useEffect, useState } from "react";

/**
 * 让"有 → 无"的变化延迟 `ms` 生效：非空值立即生效，变为空值后先保留上一个非空值。
 * 用于吸收运行状态在工具轮次之间的短暂空闲（pending → executing、工具完成 → 续写），
 * 避免状态文字、处理中提示来回闪烁。`scope` 变化（如切换会话）时立即丢弃保留值。
 */
export function useLinger<T>(value: T, ms: number, scope: unknown = undefined): T {
    const [held, setHeld] = useState({ value, scope });
    useEffect(() => {
        if (value) { setHeld({ value, scope }); return; }
        const timer = setTimeout(() => setHeld({ value, scope }), ms);
        return () => clearTimeout(timer);
    }, [value, ms, scope]);
    if (value || held.scope !== scope) return value;
    return held.value;
}

/** 让 `true` 持续 `ms` 后才生效，`false` 立即生效。用于过滤一闪而过的瞬时状态。 */
export function useSettledFlag(flag: boolean, ms: number): boolean {
    const [settled, setSettled] = useState(false);
    useEffect(() => {
        if (!flag) { setSettled(false); return; }
        const timer = setTimeout(() => setSettled(true), ms);
        return () => clearTimeout(timer);
    }, [flag, ms]);
    return flag && settled;
}
