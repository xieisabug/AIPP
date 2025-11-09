import { useState, useEffect, useMemo, useRef } from "react";
import { registerSubTaskIconComponent } from "../../data/SubTask";
import { registerSubTask } from "@/lib/subTaskRegistry";
import { AssistantType } from "@/types/assistant";

export const useAssistantTypePlugin = (pluginList: any[]) => {
    // 插件实例
    const [assistantTypePluginMap, setAssistantTypePluginMap] = useState<Map<number, AippAssistantTypePlugin>>(
        new Map()
    );

    // 跟踪哪些插件已经初始化过，避免重复调用
    const initializedPluginsRef = useRef<Set<any>>(new Set());

    // 插件名称
    const [assistantTypeNameMap, setAssistantTypeNameMap] = useState<Map<number, string>>(new Map<number, string>());

    // 插件自定义字段
    const [assistantTypeCustomField, setAssistantTypeCustomField] = useState<
        Array<{ key: string; value: Record<string, any> }>
    >([]);

    // 插件自定义label
    const [assistantTypeCustomLabel, setAssistantTypeCustomLabel] = useState<Map<string, string>>(
        new Map<string, string>()
    );

    // 插件自定义tips
    const [assistantTypeCustomTips, setAssistantTypeCustomTips] = useState<Map<string, string>>(
        new Map<string, string>()
    );

    // 插件隐藏字段
    const [assistantTypeHideField, setAssistantTypeHideField] = useState<Array<string>>([]);

    // 助手类型
    const [assistantTypes, setAssistantTypes] = useState<AssistantType[]>([{ code: 0, name: "普通对话助手" }]);

    // 使用 useMemo 缓存 assistantTypeApi
    const assistantTypeApi: AssistantTypeApi = useMemo(
        () => ({
            typeRegist: (_: number, code: number, label: string, pluginInstance: AippAssistantTypePlugin) => {
                // 检查是否已存在相同的 code
                setAssistantTypes((prev) => {
                    if (!prev.some((type) => type.code === code)) {
                        return [...prev, { code: code, name: label }];
                    } else {
                        return prev;
                    }
                });

                setAssistantTypePluginMap((prev) => {
                    const newMap = new Map(prev);
                    newMap.set(code, pluginInstance);
                    return newMap;
                });
                setAssistantTypeNameMap((prev) => {
                    const newMap = new Map(prev);
                    newMap.set(code, label);
                    return newMap;
                });
            },
            subTaskRegist: async (_options: SubTaskRegistOptions) => {
                // 这个实现会被插件特定的实现覆盖
                console.warn("subTaskRegist called without plugin context");
            },
            markdownRemarkRegist: (_: any) => { },
            changeFieldLabel: (fieldName: string, label: string) => {
                setAssistantTypeCustomLabel((prev) => {
                    const newMap = new Map(prev);
                    newMap.set(fieldName, label);
                    return newMap;
                });
            },
            addField: (options: AddFieldOptions) => {
                const { fieldName, label, type, fieldConfig } = options;
                setAssistantTypeCustomField((prev) => {
                    const newField = {
                        key: fieldName,
                        value: Object.assign(
                            {
                                type: type,
                                label: label,
                                value: "",
                            },
                            fieldConfig
                        ),
                    };
                    return [...prev, newField];
                });
            },
            hideField: (fieldName: string) => {
                setAssistantTypeHideField((prev) => {
                    return [...prev, fieldName];
                });
            },
            addFieldTips: (fieldName: string, tips: string) => {
                setAssistantTypeCustomTips((prev) => {
                    const newMap = new Map(prev);
                    newMap.set(fieldName, tips);
                    return newMap;
                });
            },
            runLogic: (_: (assistantRunApi: AssistantRunApi) => void) => { },
            forceFieldValue: function (_: string, __: string): void { },
        }),
        []
    );

    // 给默认的字段增加Label和Tips
    useEffect(() => {
        assistantTypeApi.changeFieldLabel("max_tokens", "Max Tokens");
        assistantTypeApi.changeFieldLabel("temperature", "Temperature");
        assistantTypeApi.changeFieldLabel("top_p", "Top P");
        assistantTypeApi.changeFieldLabel("stream", "Stream");
        assistantTypeApi.addFieldTips("max_tokens", "最大Token数，影响回复的长度");
        assistantTypeApi.addFieldTips("temperature", "控制生成的随机性，越高越随机");
        assistantTypeApi.addFieldTips("top_p", "控制生成的多样性，越高越多样");
        assistantTypeApi.addFieldTips("stream", "是否流式输出，开启后可能会有延迟");
        assistantTypeApi.hideField("use_native_toolcall");
    }, [assistantTypeApi]);

    // 加载助手类型的插件
    useEffect(() => {
        pluginList
            .filter((plugin: any) => plugin.pluginType.includes("assistantType"))
            .forEach((plugin: any) => {
                // 检查该插件实例是否已经初始化过
                if (!plugin.instance || initializedPluginsRef.current.has(plugin.instance)) {
                    return; // 跳过未加载或已初始化的插件
                }

                // 标记该插件实例已初始化
                initializedPluginsRef.current.add(plugin.instance);

                // 为每个插件创建一个包含插件ID的assistantTypeApi
                const pluginAwareApi = {
                    ...assistantTypeApi,
                    subTaskRegist: async (options: SubTaskRegistOptions) => {
                        // 先注册图标，保证 UI 立即可见；后端失败也不影响图标展示
                        if (options.iconComponent) {
                            try {
                                registerSubTaskIconComponent(options.code, options.iconComponent);
                            } catch { /* noop */ }
                        }

                        // 使用全局注册表，防止跨窗口重复注册
                        try {
                            await registerSubTask(
                                options.code,
                                options.name,
                                options.description,
                                options.systemPrompt,
                                "plugin",
                                plugin.id || 0
                            );
                        } catch (error) {
                            console.error(`Failed to register sub task '${options.code}':`, error);
                        }
                    }
                };
                plugin.instance.onAssistantTypeInit(pluginAwareApi);
            });
    }, [pluginList]); // 移除 assistantTypeApi 依赖，因为它是稳定的 (useMemo with [])

    return {
        assistantTypes,
        assistantTypePluginMap,
        assistantTypeNameMap,
        assistantTypeCustomField,
        setAssistantTypeCustomField,
        assistantTypeCustomLabel,
        assistantTypeCustomTips,
        assistantTypeHideField,
        assistantTypeApi,
    };
};
