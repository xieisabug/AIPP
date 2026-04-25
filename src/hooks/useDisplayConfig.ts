import { useCallback, useEffect, useSyncExternalStore } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface DisplayConfig {
    theme: string;
    color_mode: string;
    user_message_markdown_render: string;
    code_theme_light: string;
    code_theme_dark: string;
    merge_assistant_messages: string;
    show_thinking: string;
    preview_code_show_toolbar: string;
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
    code_theme_dark: 'github-dark',
    merge_assistant_messages: 'enabled',
    show_thinking: 'enabled',
    preview_code_show_toolbar: 'disabled',
};

type DisplayConfigStore = {
    state: DisplayConfigState;
    listeners: Set<() => void>;
    loadPromise?: Promise<void>;
};

const store: DisplayConfigStore = {
    state: {
        config: null,
        isLoading: false,
        error: null,
    },
    listeners: new Set(),
};

const emit = () => {
    store.listeners.forEach((listener) => listener());
};

const setState = (
    updater:
        | Partial<DisplayConfigState>
        | ((prev: DisplayConfigState) => Partial<DisplayConfigState>),
) => {
    const patch =
        typeof updater === 'function' ? (updater as (prev: DisplayConfigState) => Partial<DisplayConfigState>)(store.state) : updater;
    store.state = { ...store.state, ...patch };
    emit();
};

const loadConfig = async () => {
    // 防止并发重复请求：复用进行中的 Promise
    if (store.loadPromise) return store.loadPromise;

    store.loadPromise = (async () => {
        try {
            setState({ isLoading: true, error: null });

            const featureConfigList = await invoke<Array<{
                id: number;
                feature_code: string;
                key: string;
                value: string;
            }>>('get_all_feature_config');

            // 提取显示配置
            const displayConfigMap = new Map<string, string>();
            featureConfigList
                .filter((item) => item.feature_code === 'display')
                .forEach((item) => {
                    displayConfigMap.set(item.key, item.value);
                });

            const config: DisplayConfig = {
                theme: displayConfigMap.get('theme') || DEFAULT_CONFIG.theme,
                color_mode: displayConfigMap.get('color_mode') || DEFAULT_CONFIG.color_mode,
                user_message_markdown_render:
                    displayConfigMap.get('user_message_markdown_render') || DEFAULT_CONFIG.user_message_markdown_render,
                code_theme_light: displayConfigMap.get('code_theme_light') || DEFAULT_CONFIG.code_theme_light,
                code_theme_dark: displayConfigMap.get('code_theme_dark') || DEFAULT_CONFIG.code_theme_dark,
                merge_assistant_messages:
                    displayConfigMap.get('merge_assistant_messages') || DEFAULT_CONFIG.merge_assistant_messages,
                show_thinking:
                    displayConfigMap.get('show_thinking') || DEFAULT_CONFIG.show_thinking,
                preview_code_show_toolbar:
                    displayConfigMap.get('preview_code_show_toolbar') || DEFAULT_CONFIG.preview_code_show_toolbar,
            };

            setState({
                config,
                isLoading: false,
                error: null,
            });
        } catch (error) {
            console.error('Failed to load display config:', error);
            setState({
                config: DEFAULT_CONFIG,
                isLoading: false,
                error: error instanceof Error ? error.message : 'Unknown error',
            });
        } finally {
            store.loadPromise = undefined;
        }
    })();

    return store.loadPromise;
};

const subscribe = (listener: () => void) => {
    store.listeners.add(listener);
    return () => {
        store.listeners.delete(listener);
    };
};

const getSnapshot = () => store.state;

export const useDisplayConfig = () => {
    const state = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);

    // 初次挂载时触发一次加载（仅一次请求，多组件共享）
    useEffect(() => {
        if (!state.config && !state.isLoading) {
            loadConfig();
        }
    }, [state.config, state.isLoading]);

    // 监听配置变化事件，实时刷新
    useEffect(() => {
        const unsubscribe = listen('display-config-changed', () => {
            loadConfig();
        });
        const unsubscribeTheme = listen('theme-changed', () => {
            loadConfig();
        });
        return () => {
            unsubscribe.then((fn) => fn());
            unsubscribeTheme.then((fn) => fn());
        };
    }, []);

    const refreshConfig = useCallback(() => loadConfig(), []);

    const isUserMessageMarkdownEnabled = state.config?.user_message_markdown_render === 'enabled';
    const isMergeAssistantMessages = state.config?.merge_assistant_messages !== 'disabled';
    const isShowThinking = state.config?.show_thinking !== 'disabled';
    const isPreviewCodeShowToolbar = state.config?.preview_code_show_toolbar === 'enabled';

    return {
        config: state.config,
        isLoading: state.isLoading,
        error: state.error,
        isUserMessageMarkdownEnabled,
        isMergeAssistantMessages,
        isShowThinking,
        isPreviewCodeShowToolbar,
        refreshConfig,
    };
};