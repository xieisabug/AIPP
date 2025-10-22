import { useCallback } from 'react';
import { useDisplayConfig } from './useDisplayConfig';
import { useTheme } from './useTheme';

// 可用的代码主题配置 (Shiki themes)
export interface CodeThemeOption {
    id: string;
    name: string;
    category: 'light' | 'dark';
}

export const AVAILABLE_CODE_THEMES: CodeThemeOption[] = [
    // 浅色主题 (Shiki bundled themes)
    { id: 'github-light', name: 'GitHub Light', category: 'light' },
    { id: 'github-light-default', name: 'GitHub Light Default', category: 'light' },
    { id: 'min-light', name: 'Min Light', category: 'light' },
    { id: 'one-light', name: 'One Light', category: 'light' },
    
    // 深色主题 (Shiki bundled themes)
    { id: 'github-dark', name: 'GitHub Dark', category: 'dark' },
    { id: 'github-dark-default', name: 'GitHub Dark Default', category: 'dark' },
    { id: 'github-dark-dimmed', name: 'GitHub Dark Dimmed', category: 'dark' },
    { id: 'min-dark', name: 'Min Dark', category: 'dark' },
    { id: 'one-dark-pro', name: 'One Dark Pro', category: 'dark' },
];

export const useCodeTheme = () => {
    const { config } = useDisplayConfig();
    const { resolvedTheme } = useTheme();

    // 获取当前应该使用的主题
    const getCurrentTheme = useCallback((): string => {
        if (!config) return 'github-dark';
        
        return resolvedTheme === 'dark' 
            ? config.code_theme_dark || 'github-dark'
            : config.code_theme_light || 'github-light';
    }, [config, resolvedTheme]);

    // 提供明暗两套主题，供 Shiki 选择
    const getLightTheme = useCallback((): string => {
        return config?.code_theme_light || 'github-light';
    }, [config]);

    const getDarkTheme = useCallback((): string => {
        return config?.code_theme_dark || 'github-dark';
    }, [config]);

    // 预设主题选项
    const getLightThemes = useCallback((): CodeThemeOption[] => {
        return AVAILABLE_CODE_THEMES.filter(theme => theme.category === 'light');
    }, []);

    const getDarkThemes = useCallback((): CodeThemeOption[] => {
        return AVAILABLE_CODE_THEMES.filter(theme => theme.category === 'dark');
    }, []);

    return {
        currentTheme: getCurrentTheme(),
        lightTheme: getLightTheme(),
        darkTheme: getDarkTheme(),
        getLightThemes,
        getDarkThemes,
        availableThemes: AVAILABLE_CODE_THEMES
    };
};