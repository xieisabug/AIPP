import React, { useEffect, useCallback, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import debounce from "lodash/debounce";
import TagInputContainer from "./TagInputContainer";
import ReadOnlyModelList from "./ReadOnlyModelList";
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
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "../ui/dialog";
import { Input } from "../ui/input";
import { Trash2, ChevronDown, Share, Copy, Search, KeyRound, Edit } from "lucide-react";
import { useCopilot } from "@/hooks/useCopilot";

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
    onRename?: (newName: string) => void;
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
    onRename,
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
    const [hasApiKey, setHasApiKey] = useState<boolean>(false);
    const [manualTokenDialogOpen, setManualTokenDialogOpen] = useState<boolean>(false);
    const [manualToken, setManualToken] = useState<string>("");
    const [renameDialogOpen, setRenameDialogOpen] = useState<boolean>(false);
    const [newProviderName, setNewProviderName] = useState<string>("");

    const isCopilotProvider = apiType === "github_copilot";

    // API 类型显示标签映射
    const apiTypeLabels: Record<string, string> = {
        'openai_api': 'OpenAI API',
        'ollama': 'Ollama API',
        'anthropic': 'Anthropic API',
        'cohere': 'Cohere API',
        'deepseek': 'DeepSeek API',
        'github_copilot': 'GitHub Copilot',
    };
    const apiTypeLabel = apiTypeLabels[apiType] || apiType;

    // GitHub Copilot 授权管理
    const copilot = useCopilot({
        llmProviderId: id,
        onAuthSuccess: () => {
            // 授权成功后刷新配置
            invoke<Array<LLMProviderConfig>>("get_llm_provider_config", { id })
                .then((configArray) => {
                    const newConfig: Record<string, string> = {};
                    configArray.forEach((item) => {
                        newConfig[item.name] = item.value;
                    });
                    form.reset(newConfig);
                    setHasApiKey(!!newConfig.api_key);
                })
                .catch((e) => {
                    console.error("[Copilot] refresh provider config failed", e);
                });
        },
    });

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
                                            onClick={() => {
                                                copilot.cancelAuthorization();
                                                setHasApiKey(false);
                                            }}
                                        >
                                            取消授权
                                        </Button>
                                    </>
                                ) : (
                                    <>
                                        <div className="text-sm text-muted-foreground mb-2">
                                            选择一种授权方式来接入 GitHub Copilot：
                                        </div>

                                        {/* 授权码显示区域 */}
                                        {copilot.authInfo.userCode && (
                                            <div className="p-3 border border-border rounded-lg bg-muted">
                                                <div className="text-xs text-muted-foreground mb-2">
                                                    授权码 (User Code):
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    <code className="flex-1 text-lg font-mono font-bold bg-background px-3 py-2 rounded border border-border">
                                                        {copilot.authInfo.userCode}
                                                    </code>
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        size="sm"
                                                        onClick={() => {
                                                            if (copilot.authInfo.userCode) {
                                                                navigator.clipboard.writeText(copilot.authInfo.userCode);
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

                                        {/* 三种授权方式按钮 */}
                                        <div className="flex flex-col gap-2">
                                            {/* 方式1: 扫描已有配置 */}
                                            <Button
                                                type="button"
                                                variant="outline"
                                                className="justify-start gap-2"
                                                disabled={copilot.isAuthorizing}
                                                onClick={() => {
                                                    copilot.scanConfigAuth();
                                                }}
                                            >
                                                <Search className="h-4 w-4" />
                                                <div className="flex flex-col items-start">
                                                    <span>扫描已有授权</span>
                                                </div>
                                            </Button>

                                            {/* 方式2: OAuth 授权 */}
                                            <Button
                                                type="button"
                                                variant="outline"
                                                className="justify-start gap-2"
                                                disabled={copilot.isAuthorizing}
                                                onClick={() => {
                                                    copilot.oauthFlowAuth();
                                                }}
                                            >
                                                <KeyRound className="h-4 w-4" />
                                                <div className="flex flex-col items-start">
                                                    <span>{copilot.isAuthorizing ? "授权中..." : "OAuth 授权"}</span>
                                                </div>
                                            </Button>

                                            {/* 方式3: 手动输入 Token */}
                                            <Button
                                                type="button"
                                                variant="outline"
                                                className="justify-start gap-2"
                                                disabled={copilot.isAuthorizing}
                                                onClick={() => {
                                                    setManualToken("");
                                                    setManualTokenDialogOpen(true);
                                                }}
                                            >
                                                <Edit className="h-4 w-4" />
                                                <div className="flex flex-col items-start">
                                                    <span>手动输入 Token</span>
                                                </div>
                                            </Button>
                                        </div>

                                        <div className="text-xs text-muted-foreground mt-2">
                                            <p>OAuth 授权将通过浏览器进行 GitHub Device Flow 授权。</p>
                                            <p className="mt-1">授权成功后，Token 会自动保存并用于 Copilot API 调用。</p>
                                        </div>
                                    </>
                                )}
                            </div>
                        ),
                    },
                },
                ...(hasApiKey ? [{
                    key: "copilot_models",
                    config: {
                        type: "custom" as const,
                        label: "模型列表",
                        value: "",
                        customRender: () => (
                            <ReadOnlyModelList
                                llmProviderId={id}
                                tags={tags}
                                onTagsChange={onTagsChange}
                                onFetchModels={(modelData) => {
                                    setModelSelectionData(modelData);
                                    setModelSelectionDialogOpen(true);
                                }}
                            />
                        ),
                    },
                }] : []),
                // GitHub Copilot 高级配置（代理设置）
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
        }

        // 其它提供商：保持原有表单
        return [
            {
                key: "apiType",
                config: {
                    type: "static" as const,
                    label: "API类型",
                    value: apiTypeLabel,
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
    }, [apiType, apiTypeLabel, isCopilotProvider, tagInputRender, isAdvancedConfigExpanded, form, updateField, proxyEnabled, hasApiKey, copilot.authInfo, copilot.isAuthorizing, copilot.scanConfigAuth, copilot.oauthFlowAuth, copilot.cancelAuthorization, id, tags, onTagsChange]);

    // 打开改名对话框
    const handleOpenRenameDialog = useCallback(() => {
        setNewProviderName(name);
        setRenameDialogOpen(true);
    }, [name]);

    // 确认改名
    const handleConfirmRename = useCallback(() => {
        if (newProviderName.trim() && onRename) {
            onRename(newProviderName.trim());
            setRenameDialogOpen(false);
        }
    }, [newProviderName, onRename]);

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
                onTitleClick={onRename ? handleOpenRenameDialog : undefined}
            />
            <ModelSelectionDialog
                open={modelSelectionDialogOpen}
                onOpenChange={setModelSelectionDialogOpen}
                modelData={modelSelectionData}
                onConfirm={handleModelSelectionConfirm}
                loading={isUpdatingModels}
            />
            {/* 手动输入 Token 对话框 */}
            <Dialog open={manualTokenDialogOpen} onOpenChange={setManualTokenDialogOpen}>
                <DialogContent className="sm:max-w-md">
                    <DialogHeader>
                        <DialogTitle>输入 OAuth Token</DialogTitle>
                        <DialogDescription>
                            请输入 GitHub Copilot 的 OAuth Token (ghu_ 或 gho_ 开头)
                        </DialogDescription>
                    </DialogHeader>
                    <div className="flex flex-col gap-4 py-4">
                        <Input
                            placeholder="ghu_xxxxxxxxx 或 gho_xxxxxxxxx..."
                            value={manualToken}
                            onChange={(e) => setManualToken(e.target.value)}
                            className="font-mono"
                        />
                    </div>
                    <DialogFooter>
                        <Button
                            variant="outline"
                            onClick={() => setManualTokenDialogOpen(false)}
                        >
                            取消
                        </Button>
                        <Button
                            onClick={async () => {
                                await copilot.manualTokenAuth(manualToken);
                                setManualTokenDialogOpen(false);
                            }}
                            disabled={!manualToken.trim()}
                        >
                            保存
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>
            {/* 改名对话框 */}
            <Dialog open={renameDialogOpen} onOpenChange={setRenameDialogOpen}>
                <DialogContent className="sm:max-w-md">
                    <DialogHeader>
                        <DialogTitle>重命名提供商</DialogTitle>
                        <DialogDescription>
                            请输入新的提供商名称
                        </DialogDescription>
                    </DialogHeader>
                    <div className="flex flex-col gap-4 py-4">
                        <Input
                            placeholder="请输入提供商名称"
                            value={newProviderName}
                            onChange={(e) => setNewProviderName(e.target.value)}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter' && newProviderName.trim()) {
                                    handleConfirmRename();
                                }
                            }}
                        />
                    </div>
                    <DialogFooter>
                        <Button
                            variant="outline"
                            onClick={() => setRenameDialogOpen(false)}
                        >
                            取消
                        </Button>
                        <Button
                            onClick={handleConfirmRename}
                            disabled={!newProviderName.trim()}
                        >
                            确定
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>
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
        prevProps.onDelete === nextProps.onDelete &&
        prevProps.onRename === nextProps.onRename
    );
});
