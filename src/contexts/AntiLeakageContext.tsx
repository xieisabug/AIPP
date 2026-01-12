import React, { createContext, useContext, useState, useCallback, ReactNode } from "react";

interface AntiLeakageContextValue {
    /** 防泄露模式是否启用 */
    enabled: boolean;
    /** 是否临时显示原文 */
    isRevealed: boolean;
    /** 切换临时显示状态 */
    toggleReveal: () => void;
    /** 重置为隐藏状态 */
    resetReveal: () => void;
    /** 设置启用状态 */
    setEnabled: (enabled: boolean) => void;
}

const AntiLeakageContext = createContext<AntiLeakageContextValue | undefined>(undefined);

interface AntiLeakageProviderProps {
    children: ReactNode;
    /** 防泄露模式是否启用（从 feature config 读取） */
    enabled: boolean;
}

export const AntiLeakageProvider: React.FC<AntiLeakageProviderProps> = ({ children, enabled }) => {
    const [isRevealed, setIsRevealed] = useState(false);

    const toggleReveal = useCallback(() => {
        setIsRevealed((prev) => !prev);
    }, []);

    const resetReveal = useCallback(() => {
        setIsRevealed(false);
    }, []);

    const setEnabled = useCallback((newEnabled: boolean) => {
        // 关闭防泄露模式时重置临时显示状态
        if (!newEnabled) {
            setIsRevealed(false);
        }
    }, []);

    const value: AntiLeakageContextValue = {
        enabled,
        isRevealed,
        toggleReveal,
        resetReveal,
        setEnabled,
    };

    return <AntiLeakageContext.Provider value={value}>{children}</AntiLeakageContext.Provider>;
};

/**
 * Hook to access the anti-leakage mode context
 */
export const useAntiLeakage = (): AntiLeakageContextValue => {
    const context = useContext(AntiLeakageContext);
    if (!context) {
        throw new Error("useAntiLeakage must be used within an AntiLeakageProvider");
    }
    return context;
};

export default AntiLeakageContext;
