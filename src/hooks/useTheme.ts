import { useState, useEffect, useCallback } from 'react';
import { useDisplayConfig } from './useDisplayConfig';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

export type ThemeMode = 'light' | 'dark' | 'system';
export type ResolvedTheme = 'light' | 'dark';

interface ThemeState {
    mode: ThemeMode;
    resolvedTheme: ResolvedTheme;
    systemTheme: ResolvedTheme;
}

type PluginThemeMode = 'light' | 'dark' | 'both';

interface StoredPluginThemeDefinition {
    mode?: PluginThemeMode;
    variables?: Record<string, string>;
    extraCss?: string;
    windowCss?: Record<string, string>;
}

const PLUGIN_THEME_REGISTRY_STORAGE_KEY = 'aipp-plugin-theme-registry';
const WINDOW_SCOPE_CLASS_PREFIX = 'aipp-window-';

export const useTheme = (windowLabel?: string) => {
    const { config, isLoading, refreshConfig } = useDisplayConfig();
    const [themeState, setThemeState] = useState<ThemeState>({
        mode: 'system',
        resolvedTheme: 'light',
        systemTheme: 'light'
    });

    const normalizeWindowLabel = useCallback((rawWindowLabel: string): string => {
        return String(rawWindowLabel || '')
            .trim()
            .toLowerCase()
            .replace(/[^a-z0-9_-]+/g, '-')
            .replace(/-{2,}/g, '-')
            .replace(/^[-_]+|[-_]+$/g, '');
    }, []);

    const resolveWindowLabel = useCallback((): string => {
        if (windowLabel && windowLabel.trim()) {
            return normalizeWindowLabel(windowLabel);
        }
        try {
            const currentLabel = getCurrentWebviewWindow().label;
            return normalizeWindowLabel(currentLabel || '');
        } catch {
            return '';
        }
    }, [windowLabel, normalizeWindowLabel]);

    // 检测系统主题
    const detectSystemTheme = useCallback((): ResolvedTheme => {
        if (typeof window !== 'undefined' && window.matchMedia) {
            return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
        }
        return 'light';
    }, []);

    const resolveThemeWindowCss = useCallback((
        windowCss: Record<string, string> | undefined,
        selector: string,
        activeWindowLabel: string
    ): string => {
        if (!windowCss || typeof windowCss !== 'object') {
            return '';
        }
        const normalizedActiveWindow = normalizeWindowLabel(activeWindowLabel);
        return Object.entries(windowCss)
            .map(([rawWindowLabel, rawCss]) => {
                const normalizedWindowLabel = normalizeWindowLabel(rawWindowLabel);
                const css = String(rawCss || '').trim();
                if (!normalizedWindowLabel || !css) {
                    return '';
                }
                if (normalizedActiveWindow && normalizedActiveWindow !== normalizedWindowLabel) {
                    return '';
                }
                const windowScopeSelector = `${selector}.${WINDOW_SCOPE_CLASS_PREFIX}${normalizedWindowLabel}`;
                if (css.includes(':scope')) {
                    return css.replace(/:scope/g, windowScopeSelector);
                }
                return `${windowScopeSelector} ${css}`;
            })
            .filter(Boolean)
            .join('\n');
    }, [normalizeWindowLabel]);

    const ensurePluginThemeStyle = useCallback((themeName?: string, activeWindowLabel?: string) => {
        if (!themeName || themeName === 'default') return;
        if (typeof window === 'undefined') return;

        const runtimeStyleId = `aipp-plugin-theme-${themeName}`;
        if (document.getElementById(runtimeStyleId)) {
            return;
        }

        const registryRaw = localStorage.getItem(PLUGIN_THEME_REGISTRY_STORAGE_KEY);
        if (!registryRaw) {
            return;
        }

        let registry: Record<string, StoredPluginThemeDefinition> = {};
        try {
            registry = JSON.parse(registryRaw) || {};
        } catch (error) {
            console.warn('[useTheme] Failed to parse plugin theme registry:', error);
            return;
        }

        const registryItem = registry[themeName];
        if (!registryItem || !registryItem.variables || typeof registryItem.variables !== 'object') {
            return;
        }

        const declarations = Object.entries(registryItem.variables)
            .map(([name, value]) => {
                const cssVarName = String(name || '').trim();
                const cssValue = String(value ?? '').trim();
                if (!cssVarName || !cssValue) return '';
                return `  ${cssVarName.startsWith('--') ? cssVarName : `--${cssVarName}`}: ${cssValue};`;
            })
            .filter(Boolean)
            .join('\n');
        if (!declarations) {
            return;
        }

        const selectorBase = `.theme-${themeName}`;
        const mode = registryItem.mode || 'light';
        const selector = mode === 'dark'
            ? `${selectorBase}.dark`
            : mode === 'both'
                ? selectorBase
                : `${selectorBase}:not(.dark)`;

        const styleId = `aipp-plugin-theme-preload-${themeName}`;
        let styleElement = document.getElementById(styleId) as HTMLStyleElement | null;
        if (!styleElement) {
            styleElement = document.createElement('style');
            styleElement.id = styleId;
            document.head.appendChild(styleElement);
        }
        const extraCssRaw = typeof registryItem.extraCss === 'string' ? registryItem.extraCss.trim() : '';
        const extraCss = extraCssRaw
            ? (extraCssRaw.includes(':scope') ? extraCssRaw.replace(/:scope/g, selector) : extraCssRaw)
            : '';
        const windowCss = resolveThemeWindowCss(registryItem.windowCss, selector, activeWindowLabel || '');
        styleElement.textContent = [ `${selector} {\n${declarations}\n}`, extraCss, windowCss ].filter(Boolean).join('\n');
    }, [resolveThemeWindowCss]);

    const applyWindowScopeClass = useCallback((activeWindowLabel?: string) => {
        const normalizedLabel = normalizeWindowLabel(activeWindowLabel || resolveWindowLabel() || '');
        if (!normalizedLabel || typeof document === 'undefined') {
            return;
        }
        const root = document.documentElement;
        [...root.classList].forEach((cls) => {
            if (cls.startsWith(WINDOW_SCOPE_CLASS_PREFIX)) {
                root.classList.remove(cls);
            }
        });
        root.classList.add(`${WINDOW_SCOPE_CLASS_PREFIX}${normalizedLabel}`);
    }, [normalizeWindowLabel, resolveWindowLabel]);

    // 应用主题到DOM
    const applyTheme = useCallback((theme: ResolvedTheme, themeName?: string) => {
        const activeWindowLabel = resolveWindowLabel();
        applyWindowScopeClass(activeWindowLabel);
        ensurePluginThemeStyle(themeName, activeWindowLabel);
        const root = document.documentElement;
        if (theme === 'dark') {
            root.classList.add('dark');
        } else {
            root.classList.remove('dark');
        }
        // 移除所有 theme- 前缀的 class，再添加当前主题
        root.classList.forEach(cls => {
            if (cls.startsWith('theme-')) root.classList.remove(cls);
        });
        if (themeName && themeName !== 'default') {
            root.classList.add(`theme-${themeName}`);
        }
    }, [applyWindowScopeClass, ensurePluginThemeStyle, resolveWindowLabel]);

    // 计算最终主题
    const resolveTheme = useCallback((mode: ThemeMode, systemTheme: ResolvedTheme): ResolvedTheme => {
        switch (mode) {
            case 'light':
                return 'light';
            case 'dark':
                return 'dark';
            case 'system':
                return systemTheme;
            default:
                return 'light';
        }
    }, []);

    // 监听系统主题变化
    useEffect(() => {
        if (typeof window === 'undefined' || !window.matchMedia) return;

        const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
        const handleChange = (e: MediaQueryListEvent) => {
            const newSystemTheme: ResolvedTheme = e.matches ? 'dark' : 'light';
            setThemeState(prev => {
                const newResolvedTheme = resolveTheme(prev.mode, newSystemTheme);
                applyTheme(newResolvedTheme, config?.theme);
                return {
                    ...prev,
                    systemTheme: newSystemTheme,
                    resolvedTheme: newResolvedTheme
                };
            });
        };

        // 使用现代API
        if (mediaQuery.addEventListener) {
            mediaQuery.addEventListener('change', handleChange);
            return () => mediaQuery.removeEventListener('change', handleChange);
        } else {
            // 兼容旧版本
            mediaQuery.addListener(handleChange);
            return () => mediaQuery.removeListener(handleChange);
        }
    }, [resolveTheme, applyTheme, config?.theme]);

    // 监听配置变化
    useEffect(() => {
        if (!config || isLoading) return;

        const mode = (config.color_mode as ThemeMode) || 'system';
        const systemTheme = detectSystemTheme();
        const resolvedTheme = resolveTheme(mode, systemTheme);

        setThemeState({
            mode,
            resolvedTheme,
            systemTheme
        });

        // 同步到localStorage以供加载页面使用
        localStorage.setItem('theme-mode', mode);
        localStorage.setItem('theme-name', config.theme || 'default');

        applyTheme(resolvedTheme, config.theme);
    }, [config, isLoading, detectSystemTheme, resolveTheme, applyTheme]);

    useEffect(() => {
        applyWindowScopeClass();
    }, [applyWindowScopeClass]);

    // 注意：不在初始化时强制设置主题，避免在子组件挂载时反复切换 .dark 导致白屏闪烁。
    // 初始主题已在 index.html 里通过内联脚本应用；当配置或系统主题变化时再更新。

    // 监听跨窗口主题同步事件
    useEffect(() => {
        const unlistenThemeChange = listen('theme-changed', () => {
            refreshConfig();
        });
        const unlistenFeatureConfigChanged = listen('feature_config_changed', () => {
            refreshConfig();
        });

        return () => {
            unlistenThemeChange.then(f => f());
            unlistenFeatureConfigChanged.then(f => f());
        };
    }, [refreshConfig]);

    // 设置主题模式
    const setThemeMode = useCallback(async (newMode: ThemeMode) => {
        try {
            // 保存到后端
            await invoke('save_feature_config', {
                featureCode: 'display',
                config: {
                    theme: config?.theme || 'default',
                    color_mode: newMode,
                    user_message_markdown_render: config?.user_message_markdown_render || 'enabled'
                }
            });

            // 同步到localStorage以供加载页面使用
            localStorage.setItem('theme-mode', newMode);
            localStorage.setItem('theme-name', config?.theme || 'default');

            // 发出主题变化事件，通知其他窗口
            await emit('theme-changed', { mode: newMode });

            // 刷新配置
            refreshConfig();
        } catch (error) {
            console.error('Failed to save theme mode:', error);
        }
    }, [config, refreshConfig]);

    // 切换主题（在light和dark之间切换）
    const toggleTheme = useCallback(() => {
        const newMode = themeState.resolvedTheme === 'dark' ? 'light' : 'dark';
        setThemeMode(newMode);
    }, [themeState.resolvedTheme, setThemeMode]);

    return {
        mode: themeState.mode,
        resolvedTheme: themeState.resolvedTheme,
        systemTheme: themeState.systemTheme,
        setThemeMode,
        toggleTheme,
        isLoading
    };
};
