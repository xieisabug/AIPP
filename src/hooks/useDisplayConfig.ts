import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface DisplayConfig {
    theme: string;
    color_mode: string;
    user_message_markdown_render: string;
    code_theme_light: string;
    code_theme_dark: string;
}

interface DisplayConfigState {
    config: DisplayConfig | null;
    isLoading: boolean;
    error: string | null;
}

const DEFAULT_CONFIG: DisplayConfig = {
    theme: 'default',
    color_mode: 'system',
    user_message_markdown_render: 'enabled',
    code_theme_light: 'github',
    code_theme_dark: 'github-dark'
};

export const useDisplayConfig = () => {
    const [state, setState] = useState<DisplayConfigState>({
        config: null,
        isLoading: true,
        error: null
    });

    const loadConfig = useCallback(async () => {
        try {
            setState(prev => ({ ...prev, isLoading: true, error: null }));
            
            // 为避免极端情况下 invoke 被卡住，添加 2s 超时兜底
            const withTimeout = <T,>(p: Promise<T>, ms = 2000): Promise<T> => {
                return new Promise<T>((resolve, reject) => {
                    const t = setTimeout(() => reject(new Error('get_all_feature_config timeout')), ms);
                    p.then(v => { clearTimeout(t); resolve(v); })
                     .catch(e => { clearTimeout(t); reject(e); });
                });
            };

            const featureConfigList = await withTimeout(invoke<Array<{
                id: number;
                feature_code: string;
                key: string;
                value: string;
            }>>('get_all_feature_config'));
            
            // 提取显示配置
            const displayConfigMap = new Map<string, string>();
            featureConfigList
                .filter(item => item.feature_code === 'display')
                .forEach(item => {
                    displayConfigMap.set(item.key, item.value);
                });
            
            const config: DisplayConfig = {
                theme: displayConfigMap.get('theme') || DEFAULT_CONFIG.theme,
                color_mode: displayConfigMap.get('color_mode') || DEFAULT_CONFIG.color_mode,
                user_message_markdown_render: displayConfigMap.get('user_message_markdown_render') || DEFAULT_CONFIG.user_message_markdown_render,
                code_theme_light: displayConfigMap.get('code_theme_light') || DEFAULT_CONFIG.code_theme_light,
                code_theme_dark: displayConfigMap.get('code_theme_dark') || DEFAULT_CONFIG.code_theme_dark,
            };
            
            setState({
                config,
                isLoading: false,
                error: null
            });
        } catch (error) {
            console.error('Failed to load display config:', error);
            setState({
                config: DEFAULT_CONFIG,
                isLoading: false,
                error: error instanceof Error ? error.message : 'Unknown error'
            });
        }
    }, []);

    useEffect(() => {
        let didLoad = false;
        // 优先等待后端完成 setup，避免过早 invoke 被阻塞
        const unlistenPromise = listen('backend-ready', () => {
            if (!didLoad) {
                didLoad = true;
                loadConfig();
            }
        });

        // 兜底：若事件未到达，1.5s 后仍触发一次加载，避免卡住 UI
        const fallbackTimer = setTimeout(() => {
            if (!didLoad) {
                didLoad = true;
                loadConfig();
            }
        }, 1500);

        return () => {
            clearTimeout(fallbackTimer);
            unlistenPromise.then(f => f());
        };
    }, [loadConfig]);

    const isUserMessageMarkdownEnabled = state.config?.user_message_markdown_render === 'enabled';

    return {
        config: state.config,
        isLoading: state.isLoading,
        error: state.error,
        isUserMessageMarkdownEnabled,
        refreshConfig: loadConfig
    };
};
