import React, { useEffect, useCallback, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import debounce from "lodash/debounce";
import TagInputContainer from "./TagInputContainer";
import ModelSelectionDialog from "./ModelSelectionDialog";
import ConfigForm from "../ConfigForm";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import { Switch } from "../ui/switch";
import { Button } from "../ui/button";
import {
    Collapsible,
    CollapsibleContent,
    CollapsibleTrigger,
} from "../ui/collapsible";
import { Trash2, ChevronDown, Share, Copy } from "lucide-react";

interface LLMProviderConfig {
    name: string;
    value: string;
}

interface LLMModel {
    name: string;
}

interface ModelForSelection {
    name: string;
    code: string;
    description: string;
    vision_support: boolean;
    audio_support: boolean;
    video_support: boolean;
    is_selected: boolean;
}

interface ModelSelectionResponse {
    available_models: ModelForSelection[];
    missing_models: string[];
}

interface LLMProviderConfigFormProps {
    index: number;
    id: string;
    apiType: string;
    name: string;
    description: string;
    isOffical: boolean;
    enabled: boolean;
    onToggleEnabled: any;
    onDelete: any;
    onShare?: () => void;
}

const LLMProviderConfigForm: React.FC<LLMProviderConfigFormProps> = ({
    id,
    index,
    apiType,
    name,
    description,
    isOffical,
    enabled,
    onDelete,
    onToggleEnabled,
    onShare,
}) => {
    const [tags, setTags] = useState<string[]>([]);
    const [isModelListExpanded, setIsModelListExpanded] =
        useState<boolean>(false);
    const [isAdvancedConfigExpanded, setIsAdvancedConfigExpanded] =
        useState<boolean>(false);
    const [modelSelectionDialogOpen, setModelSelectionDialogOpen] =
        useState<boolean>(false);
    const [modelSelectionData, setModelSelectionData] =
        useState<ModelSelectionResponse | null>(null);
    const [isUpdatingModels, setIsUpdatingModels] = useState<boolean>(false);
    const [copilotAuthInfo, setCopilotAuthInfo] = useState<{
        userCode?: string;
        verificationUri?: string;
        isAuthorizing: boolean;
    }>({ isAuthorizing: false });
    const [hasApiKey, setHasApiKey] = useState<boolean>(false);

    const isCopilotProvider = apiType === "GitHub Copilot" || apiType === "github_copilot";

    const defaultValues = useMemo(
        () => ({
            endpoint: "",
            api_key: "",
            proxy_enabled: "false",
        }),
        [],
    );

    const form = useForm({
        defaultValues,
    });

    // 监听 proxy_enabled 字段变化
    const proxyEnabled = form.watch("proxy_enabled");

    // 更新字段
    const updateField = useCallback(
        debounce((key: string, value: string) => {
            invoke("update_llm_provider_config", {
                llmProviderId: id,
                name: key,
                value,
            })
                .then(() => console.log(`Field ${key} updated`))
                .catch((error) =>
                    console.error(`Error updating field ${key}:`, error),
                );
        }, 50),
        [id],
    );

    // 当 id 变化时，取消之前的 debounce 操作
    useEffect(() => {
        return () => {
            updateField.cancel();
        };
    }, [id, updateField]);

    // 监听字段更新后自动保存
    useEffect(() => {
        // 创建一个订阅
        const subscription = form.watch((value, { name, type }) => {
            if (name && type === "change") {
                // 当有字段变化时，调用对应的保存函数
                updateField(name, value[name] ?? "");
            }
        });

        // 清理订阅
        return () => subscription.unsubscribe();
    }, [form, updateField]);

    // 获取基础数据
    useEffect(() => {
        // 立即重置状态，避免显示旧的数据
        form.reset({
            endpoint: "",
            api_key: "",
            proxy_enabled: "false",
        });
        setTags([]);
        setHasApiKey(false);

        invoke<Array<LLMProviderConfig>>("get_llm_provider_config", {
            id,
        }).then((configArray) => {
            const newConfig: Record<string, string> = {};
            configArray.forEach((item) => {
                newConfig[item.name] = item.value;
            });
            form.reset(newConfig);

            // 检查 GitHub Copilot 是否有 api_key
            if (isCopilotProvider) {
                const apiKey = configArray.find((item) => item.name === "api_key")?.value;
                setHasApiKey(!!apiKey && apiKey.length > 0);
            }
        });

        invoke<Array<LLMModel>>("get_llm_models", {
            llmProviderId: "" + id,
        }).then((modelList) => {
            const newTags = modelList.map((model) => model.name);
            // 调用子组件的方法，更新 tags
            setTags(newTags);
        });
    }, [id, isCopilotProvider]);

    // 处理模型选择确认
    const handleModelSelectionConfirm = useCallback(
        async (selectedModels: ModelForSelection[]) => {
            setIsUpdatingModels(true);
            try {
                await invoke("update_selected_models", {
                    llmProviderId: parseInt(id),
                    selectedModels,
                });

                // 更新本地标签显示
                const selectedModelNames = selectedModels
                    .filter((model) => model.is_selected)
                    .map((model) => model.name);
                setTags(selectedModelNames);

                toast.success("模型列表更新成功");
            } catch (e) {
                toast.error("更新模型列表失败: " + e);
            } finally {
                setIsUpdatingModels(false);
            }
        },
        [id],
    );

    const onTagsChange = useCallback((newTags: string[]) => {
        setTags(newTags);
    }, []);
    // 定义稳定的 customRender，不再依赖父组件的状态或函数
    const tagInputRender = useCallback(
        () => (
            <TagInputContainer
                llmProviderId={id}
                tags={tags}
                onTagsChange={onTagsChange}
                isExpanded={isModelListExpanded}
                onExpandedChange={setIsModelListExpanded}
                onFetchModels={(modelData) => {
                    setModelSelectionData(modelData);
                    setModelSelectionDialogOpen(true);
                }}
            />
        ),
        [id, tags, onTagsChange, isModelListExpanded],
    );

    // 表单字段定义
    const configFields = useMemo(() => {
        // GitHub Copilot 提供商：特殊表单
        if (isCopilotProvider) {
            return [
                {
                    key: "apiType",
                    config: {
                        type: "static" as const,
                        label: "提供商类型",
                        value: "GitHub Copilot",
                    },
                },
                {
                    key: "copilot_auth",
                    config: {
                        type: "custom" as const,
                        label: "授权状态",
                        value: "",
                        customRender: () => (
                            <div className="flex flex-col gap-3">
                                {hasApiKey ? (
                                    <>
                                        <div className="text-sm text-green-600 dark:text-green-400">
                                            ✓ 已授权 GitHub Copilot
                                        </div>
                                        <Button
                                            type="button"
                                            variant="destructive"
                                            onClick={async () => {
                                                try {
                                                    await invoke("update_llm_provider_config", {
                                                        llmProviderId: parseInt(id, 10),
                                                        name: "api_key",
                                                        value: "",
                                                    });
                                                    setHasApiKey(false);
                                                    toast.success("已取消 GitHub Copilot 授权");

                                                    // 刷新配置
                                                    const configArray = await invoke<Array<LLMProviderConfig>>(
                                                        "get_llm_provider_config",
                                                        { id },
                                                    );
                                                    const newConfig: Record<string, string> = {};
                                                    configArray.forEach((item) => {
                                                        newConfig[item.name] = item.value;
                                                    });
                                                    form.reset(newConfig);
                                                } catch (e) {
                                                    console.error("[Copilot] Revoke failed", e);
                                                    toast.error("取消授权失败: " + e);
                                                }
                                            }}
                                        >
                                            取消授权
                                        </Button>
                                    </>
                                ) : (
                                    <>
                                        <div className="text-sm text-muted-foreground">
                                            需要先完成 GitHub Copilot 授权才能使用模型。点击下方按钮开始 Device Flow 授权。
                                        </div>

                                        {copilotAuthInfo.userCode && (
                                            <div className="p-3 border border-border rounded-lg bg-muted">
                                                <div className="text-xs text-muted-foreground mb-2">
                                                    授权码 (User Code):
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    <code className="flex-1 text-lg font-mono font-bold bg-background px-3 py-2 rounded border border-border">
                                                        {copilotAuthInfo.userCode}
                                                    </code>
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        size="sm"
                                                        onClick={() => {
                                                            if (copilotAuthInfo.userCode) {
                                                                navigator.clipboard.writeText(copilotAuthInfo.userCode);
                                                                toast.success("授权码已复制到剪贴板");
                                                            }
                                                        }}
                                                    >
                                                        <Copy className="h-4 w-4" />
                                                    </Button>
                                                </div>
                                                <div className="text-xs text-muted-foreground mt-2">
                                                    请在浏览器中打开的页面输入此授权码
                                                </div>
                                            </div>
                                        )}

                                        <Button
                                            type="button"
                                            className="bg-primary hover:bg-primary/90 text-primary-foreground"
                                            disabled={copilotAuthInfo.isAuthorizing}
                                            onClick={async () => {
                                                try {
                                                    setCopilotAuthInfo({ isAuthorizing: true });

                                                    // 1. 启动 device flow
                                                    const startResp = await invoke<{
                                                        device_code: string;
                                                        user_code: string;
                                                        verification_uri: string;
                                                        expires_in: number;
                                                        interval: number;
                                                    }>("start_github_copilot_device_flow", {
                                                        llmProviderId: parseInt(id, 10),
                                                    } as any);

                                                    console.info("[Copilot] Device flow started", startResp);

                                                    // 显示授权码
                                                    setCopilotAuthInfo({
                                                        userCode: startResp.user_code,
                                                        verificationUri: startResp.verification_uri,
                                                        isAuthorizing: true,
                                                    });

                                                    toast.info(
                                                        `浏览器将自动打开授权页面，请输入授权码: ${startResp.user_code}`,
                                                        { duration: 8000 },
                                                    );

                                                    // 2. 轮询授权结果
                                                    const authResult = await invoke<{
                                                        access_token: string;
                                                        token_type: string;
                                                    }>("poll_github_copilot_token", {
                                                        llmProviderId: parseInt(id, 10),
                                                        deviceCode: startResp.device_code,
                                                        interval: startResp.interval,
                                                    } as any);

                                                    console.info("[Copilot] Device flow authorized", authResult);

                                                    toast.success("GitHub Copilot 授权成功");

                                                    setCopilotAuthInfo({ isAuthorizing: false });
                                                    setHasApiKey(true);

                                                    // 授权成功后刷新一次配置
                                                    try {
                                                        const configArray = await invoke<Array<LLMProviderConfig>>(
                                                            "get_llm_provider_config",
                                                            { id },
                                                        );
                                                        const newConfig: Record<string, string> = {};
                                                        configArray.forEach((item) => {
                                                            newConfig[item.name] = item.value;
                                                        });
                                                        form.reset(newConfig);
                                                    } catch (e) {
                                                        console.error("[Copilot] refresh provider config failed", e);
                                                    }
                                                } catch (e) {
                                                    console.error("[Copilot] Device flow failed", e);
                                                    toast.error("GitHub Copilot 授权失败: " + e);
                                                    setCopilotAuthInfo({ isAuthorizing: false });
                                                }
                                            }}
                                        >
                                            {copilotAuthInfo.isAuthorizing ? "授权中..." : "去授权 GitHub Copilot"}
                                        </Button>

                                        <div className="text-xs text-muted-foreground">
                                            授权完成后，Access Token 会保存到当前提供商的 API Key 配置中，可直接用于调用 Copilot 模型。
                                        </div>
                                    </>
                                )}
                            </div>
                        ),
                    },
                },
            ];
        }

        // 其它提供商：保持原有表单
        return [
            {
                key: "apiType",
                config: {
                    type: "static" as const,
                    label: "API类型",
                    value: apiType,
                },
            },
            {
                key: "endpoint",
                config: {
                    type: "input" as const,
                    label: "Endpoint",
                    value: "",
                },
            },
            {
                key: "api_key",
                config: {
                    type: "password" as const,
                    label: "API Key",
                    value: "",
                },
            },
            {
                key: "tagInput",
                config: {
                    type: "custom" as const,
                    label: "模型列表",
                    value: "",
                    customRender: tagInputRender,
                },
            },
            {
                key: "advanced_config",
                config: {
                    type: "custom" as const,
                    label: "",
                    value: "",
                    customRender: () => (
                        <Collapsible
                            open={isAdvancedConfigExpanded}
                            onOpenChange={setIsAdvancedConfigExpanded}
                        >
                            <CollapsibleTrigger asChild>
                                <Button
                                    variant="ghost"
                                    className="w-full justify-between p-2 h-auto text-left hover:bg-muted"
                                >
                                    <span className="text-sm font-medium text-foreground">
                                        高级配置
                                    </span>
                                    <ChevronDown
                                        className={`h-4 w-4 transition-transform ${isAdvancedConfigExpanded
                                            ? "rotate-180"
                                            : ""
                                            }`}
                                    />
                                </Button>
                            </CollapsibleTrigger>
                            <CollapsibleContent className="mt-2">
                                <div className="p-3 border border-border rounded-lg bg-muted">
                                    <div className="flex items-center justify-between">
                                        <div className="flex flex-col">
                                            <label className="text-sm font-medium text-foreground">
                                                使用网络代理进行请求
                                            </label>
                                            <span className="text-xs text-muted-foreground">
                                                启用后将使用全局网络代理配置进行模型请求
                                            </span>
                                        </div>
                                        <Switch
                                            checked={proxyEnabled === "true"}
                                            onCheckedChange={(checked) => {
                                                form.setValue(
                                                    "proxy_enabled",
                                                    checked ? "true" : "false",
                                                );
                                                updateField(
                                                    "proxy_enabled",
                                                    checked ? "true" : "false",
                                                );
                                            }}
                                        />
                                    </div>
                                </div>
                            </CollapsibleContent>
                        </Collapsible>
                    ),
                },
            },
        ];
    }, [apiType, isCopilotProvider, tagInputRender, isAdvancedConfigExpanded, form, updateField, proxyEnabled, hasApiKey, copilotAuthInfo, id]);

    const extraButtons = useMemo(
        () => (
            <div className="flex items-center gap-2">
                <div className="flex items-center gap-2">
                    <Switch
                        checked={enabled}
                        onCheckedChange={() => onToggleEnabled(index)}
                    />
                </div>
                {onShare && (
                    <div className="flex items-center gap-1">
                        {onShare && (
                            <Button
                                variant="ghost"
                                size="sm"
                                onClick={onShare}
                                className="gap-1 text-xs px-2 py-1 h-7"
                            >
                                <Share className="h-3 w-3" />
                            </Button>
                        )}
                    </div>
                )}

                {!isOffical && (
                    <Button
                        variant="ghost"
                        size="sm"
                        onClick={onDelete}
                        className="hover:bg-red-50 hover:border-red-300 hover:text-red-700"
                    >
                        <Trash2 className="h-4 w-4 mr-1" />
                    </Button>
                )}
            </div>
        ),
        [enabled, onToggleEnabled, index, isOffical, onDelete, onShare],
    );

    // 表单部分结束
    return (
        <>
            <ConfigForm
                key={id}
                title={name}
                description={description}
                config={configFields}
                classNames="bottom-space"
                extraButtons={extraButtons}
                useFormReturn={form}
            />
            <ModelSelectionDialog
                open={modelSelectionDialogOpen}
                onOpenChange={setModelSelectionDialogOpen}
                modelData={modelSelectionData}
                onConfirm={handleModelSelectionConfirm}
                loading={isUpdatingModels}
            />
        </>
    );
};

export default React.memo(LLMProviderConfigForm, (prevProps, nextProps) => {
    return (
        prevProps.id === nextProps.id &&
        prevProps.index === nextProps.index &&
        prevProps.name === nextProps.name &&
        prevProps.apiType === nextProps.apiType &&
        prevProps.isOffical === nextProps.isOffical &&
        prevProps.enabled === nextProps.enabled &&
        prevProps.onToggleEnabled === nextProps.onToggleEnabled &&
        prevProps.onDelete === nextProps.onDelete
    );
});
