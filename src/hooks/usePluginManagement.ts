import { useState, useEffect, useCallback, useMemo } from "react";
import { registerSubTaskIconComponent } from "../data/SubTask";
import { invoke } from "@tauri-apps/api/core";

// 用于存储AskAssistantApi中对应的处理函数
interface AskAssistantApiFunctions {
    onCustomUserMessage?: (question: string, assistantId: string, conversationId?: string) => any;
    onCustomUserMessageComing?: (aiResponse: any) => void;
    onStreamMessageListener?: (
        payload: string,
        aiResponse: any,
        responseIsResponsingFunction: (isFinish: boolean) => void
    ) => void;
}

export interface UsePluginManagementReturn {
    assistantTypePluginMap: Map<number, AippAssistantTypePlugin>;
    functionMap: Map<number, AskAssistantApiFunctions>;
    assistantTypeApi: AssistantTypeApi;
    setFunctionMapForMessage: (messageId: number) => void;
    getAssistantPlugin: (assistantType: number) => AippAssistantTypePlugin | undefined;
}

export function usePluginManagement(pluginList: any[]): UsePluginManagementReturn {
    // 助手类型插件映射表，key为助手类型，value为插件实例
    const [assistantTypePluginMap, setAssistantTypePluginMap] = useState<Map<number, AippAssistantTypePlugin>>(
        new Map()
    );

    // 插件函数映射表，用于存储每个消息对应的处理函数
    const [functionMap, setFunctionMap] = useState<Map<number, AskAssistantApiFunctions>>(new Map());

    // 助手类型API接口，提供给插件使用 - 使用 useMemo 避免重复创建
    const assistantTypeApi: AssistantTypeApi = useMemo(
        () => ({
            typeRegist: (_pluginType: number, code: number, _label: string, pluginInstance: AippAssistantTypePlugin) => {
                setAssistantTypePluginMap((prev) => {
                    const newMap = new Map(prev);
                    newMap.set(code, pluginInstance);
                    return newMap;
                });
            },
            subTaskRegist: async (_options: SubTaskRegistOptions) => {
                // 这个实现会被插件特定的实现覆盖
                console.warn("subTaskRegist called without plugin context");
            },
            markdownRemarkRegist: (_: any) => { },
            changeFieldLabel: (_: string, __: string) => { },
            addField: (_: AddFieldOptions) => { },
            addFieldTips: (_: string, __: string) => { },
            hideField: (_: string) => { },
            runLogic: (_: (assistantRunApi: AssistantRunApi) => void) => { },
            forceFieldValue: (_: string, __: string) => { },
        }),
        []
    );

    // 为指定消息设置函数映射
    const setFunctionMapForMessage = useCallback((messageId: number) => {
        setFunctionMap((prev) => {
            const newMap = new Map(prev);
            newMap.set(messageId, {
                onCustomUserMessage: undefined,
                onCustomUserMessageComing: undefined,
                onStreamMessageListener: undefined,
            });
            return newMap;
        });
    }, []);

    // 获取指定助手类型的插件实例
    const getAssistantPlugin = useCallback(
        (assistantType: number) => {
            return assistantTypePluginMap.get(assistantType);
        },
        [assistantTypePluginMap]
    );

    // 初始化助手类型插件
    useEffect(() => {
        pluginList
            .filter((plugin: any) => plugin.pluginType.includes("assistantType"))
            .forEach((plugin: any) => {
                if (plugin.instance) {
                    // initialize assistantType plugin
                    // 为每个插件创建一个包含插件ID的assistantTypeApi
                    const pluginAwareApi = {
                        ...assistantTypeApi,
                        subTaskRegist: async (options: SubTaskRegistOptions) => {
                            // 先进行前端图标注册，确保 UI 立即可见；后端失败也不影响图标展示
                            if (options.iconComponent) {
                                try {
                                    registerSubTaskIconComponent(options.code, options.iconComponent);
                                } catch { /* noop */ }
                            }

                            try {
                                await invoke("sub_task_regist", {
                                    code: options.code,
                                    name: options.name,
                                    description: options.description,
                                    systemPrompt: options.systemPrompt,
                                    pluginSource: "plugin",
                                    sourceId: plugin.id || 0, // 使用插件的ID，如果没有则使用0
                                });
                            } catch (error) {
                                console.error(`Failed to register sub task '${options.code}':`, error);
                            }
                        }
                    };
                    try {
                        plugin.instance?.onAssistantTypeInit(pluginAwareApi);
                    } catch (e) {
                        console.error(`[PluginManagement] onAssistantTypeInit failed for '${plugin.code}':`, e);
                    }
                }
            });
    }, [pluginList, assistantTypeApi]);

    return {
        assistantTypePluginMap,
        functionMap,
        assistantTypeApi,
        setFunctionMapForMessage,
        getAssistantPlugin,
    };
}
