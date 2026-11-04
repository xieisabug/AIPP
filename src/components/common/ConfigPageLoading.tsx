import React, { useEffect, useState } from "react";

interface ConfigPageLoadingProps {
    /** 延迟显示（毫秒）。切换详情时读取通常很快，延迟可避免一闪而过的加载文案。 */
    delayMs?: number;
}

/**
 * 设置页首次读取数据期间的占位。
 * 与设置窗口 Suspense 的 fallback 保持同一外观，懒加载结束后无缝衔接，
 * 避免数据到达前先渲染「还没有配置…」空状态再跳成真实数据。
 */
const ConfigPageLoading: React.FC<ConfigPageLoadingProps> = ({ delayMs = 0 }) => {
    const [visible, setVisible] = useState(delayMs <= 0);
    useEffect(() => {
        if (delayMs <= 0) return;
        const timer = setTimeout(() => setVisible(true), delayMs);
        return () => clearTimeout(timer);
    }, [delayMs]);

    return (
        <div
            className="flex h-full min-h-32 items-center justify-center text-sm text-muted-foreground"
            role="status"
            aria-live="polite"
            data-aipp-slot="config-page-loading"
        >
            {visible ? "加载中..." : null}
        </div>
    );
};

export default ConfigPageLoading;
