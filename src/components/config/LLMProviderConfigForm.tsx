import { useDataRefresh } from "@/hooks/useDataRefresh";
import React, { useEffect, useCallback, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import TagInputContainer from "./TagInputContainer";
import ReadOnlyModelList from "./ReadOnlyModelList";
import ModelSelectionDialog from "./ModelSelectionDialog";
import ConfigForm from "../ConfigForm";
import ConfigPageLoading from "../common/ConfigPageLoading";
import { useNativeAgentProviderFields } from "./native-agent/NativeAgentFields";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import { Switch } from "../ui/switch";
import { ProviderProxySwitch, useProviderProxyConfigured } from "./ProviderProxySwitch";
import { Button } from "../ui/button";
import {
    Collapsible,
    CollapsibleContent,
    CollapsibleTrigger,
} from "../ui/collapsible";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from "../ui/dialog";
import { Input } from "../ui/input";
import { RadioGroup, RadioGroupItem } from "../ui/radio-group";
import { Trash2, ChevronDown, Share, Copy, Search, KeyRound, Edit, CheckCircle2 } from "lucide-react";
import { useCopilot, type CopilotTokenScanResult } from "@/hooks/useCopilot";
import { useAcpEnvironment } from "@/hooks/feature/useAcpEnvironment";
import CliEnvironmentStatus from "./CliEnvironmentStatus";
import { getProviderEndpointStatus } from "@/utils/providerEndpoint";
import { ACP_CLI_OPTIONS, CODEX_APPROVAL_OPTIONS, CODEX_SANDBOX_OPTIONS } from "./providerApiTypes";
import { useVaultItems } from "@/hooks/useVaultItems";
import VaultRefField from "./vault/VaultRefField";
import DeepSeekCapabilities from "./DeepSeekCapabilities";
import { containsVaultRef } from "./vault/vaultRefs";
import {
    LLMModel,
    ModelForSelection,
    ModelSelectionResponse,
    ModelTagItem,
    toModelTagItem,
} from "./llmModelTypes";

interface LLMProviderConfig {
    name: string;
    value: string;
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
    enabled,
    onDelete,
    onToggleEnabled,
    onShare,
    onRename,
}) => {
    const [tags, setTags] = useState<ModelTagItem[]>([]);
    const proxyConfigured = useProviderProxyConfigured();
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
    // 已完成读取的提供商 id；与当前 id 不一致时表单仍是上一份（或空）数据，不应展示
    const [loadedId, setLoadedId] = useState<string | null>(null);
    const [manualTokenDialogOpen, setManualTokenDialogOpen] = useState<boolean>(false);
    const [manualToken, setManualToken] = useState<string>("");
    const [scanTokenDialogOpen, setScanTokenDialogOpen] = useState<boolean>(false);
    const [scanCandidates, setScanCandidates] = useState<CopilotTokenScanResult[]>([]);
    const [selectedScanCandidateId, setSelectedScanCandidateId] = useState<string>("");
    const [renameDialogOpen, setRenameDialogOpen] = useState<boolean>(false);
    const [newProviderName, setNewProviderName] = useState<string>("");

    const isCopilotProvider = apiType === "github_copilot";
    const isAcpProvider = apiType === "acp";
    const isCodexAppServerProvider = apiType === "codex_app_server";
    const isClaudeSdkProvider = apiType === "claude_sdk";

    // API 类型显示标签映射
    const apiTypeLabels: Record<string, string> = {
        'openai_api': 'OpenAI API',
        'ollama': 'Ollama API',
        'anthropic': 'Anthropic API',
        'cohere': 'Cohere API',
        'deepseek': 'DeepSeek API',
        'opencode_go': 'OpenCode Go',
        'magpie': 'Magpie',
        'github_copilot': 'GitHub Copilot',
        'acp': 'ACP(Agent Client Protocol)',
        'codex_app_server': 'Codex',
        'claude_sdk': 'Claude Code',
        'pi': 'Pi',
        'cursor': 'Cursor',
        'opencode': 'OpenCode',
        'kiro': 'Kiro',
        'antigravity': 'Antigravity',
    };
    const apiTypeLabel = apiTypeLabels[apiType] || apiType;

    const acpCliOptions = ACP_CLI_OPTIONS;

    // GitHub Copilot 授权管理
    const copilot = useCopilot({
        llmProviderId: id,
        onAuthSuccess: () => {
            // 授权成功后刷新配置
            const snapshot = saves.snapshot();
            invoke<Array<LLMProviderConfig>>("get_llm_provider_config", { id })
                .then((configArray) => {
                    const newConfig: Record<string, string> = {};
                    configArray.forEach((item) => {
                        newConfig[item.name] = item.value;
                    });
                    const merged = saves.merge(newConfig, snapshot);
                    form.reset(merged);
                    setHasApiKey(!!merged.api_key);
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
            deepseek_web_enabled: "false",
            deepseek_show_balance_in_chat: "false",
            proxy_enabled: "false",
            acp_cli_command: "",
            acp_claude_auth_mode: "claude_settings",
            acp_claude_env_vars: "",
            codex_cli_command: "codex",
            codex_additional_args: "",
            codex_approval_policy: "on-request",
            codex_sandbox: "workspace-write",
            claude_cli_command: "claude",
            claude_permission_mode: "default",
            claude_effort: "default",
            acp_env_vars: "",
            // Antigravity 运行时的实际默认值（未保存时 bridge 也按这两个值运行）
            antigravity_runtime: "cli",
            antigravity_auth_method: "oauth-personal",
        }),
        [],
    );

    const form = useForm({
        defaultValues,
    });

    const selectedScanCandidate = useMemo(
        () => scanCandidates.find((candidate) => candidate.id === selectedScanCandidateId) ?? null,
        [scanCandidates, selectedScanCandidateId],
    );

    // 监听 proxy_enabled 字段变化
    const webEnabled = form.watch("deepseek_web_enabled") === "true";
    const showBalanceInChat = form.watch("deepseek_show_balance_in_chat") === "true";
    const proxyEnabled = form.watch("proxy_enabled");
    const endpoint = form.watch("endpoint");
    const endpointStatus = useMemo(() => getProviderEndpointStatus(endpoint), [endpoint]);
    const vault = useVaultItems();
    const apiKeyValue = form.watch("api_key") ?? "";
    const apiKeyIsVaultRef = containsVaultRef(apiKeyValue);
    // DeepSeek：Key 存入保险箱后改显示引用（已由设置页保存命令写入，不再触发保存）
    const handleKeyShared = useCallback((value: string) => form.setValue("api_key", value), [form]);
    // 保存配置后被确认的目标：自定义地址的 origin，或该类型的默认地址。
    const vaultTarget = useMemo(() => {
        try {
            return endpoint?.trim() ? new URL(endpoint.trim()).origin : "该 Provider 的默认接口地址";
        } catch {
            return null;
        }
    }, [endpoint]);

    // 监听 ACP CLI 命令变化
    const acpCliCommand = form.watch("acp_cli_command");
    const claudeAuthMode = form.watch("acp_claude_auth_mode") || "claude_settings";

    // Keep the probe on the same provider environment while edits are being saved.
    const environmentValues = Object.entries(form.watch()).filter(([key, value]) =>
        typeof value === "string" && (key.startsWith("acp_env_") || key === "proxy_enabled"
            || (isAcpProvider && (key === "acp_claude_auth_mode" || key === "acp_claude_env_vars"))
            || (isClaudeSdkProvider && ["api_key", "endpoint", "base_url"].includes(key))));
    const environmentKey = JSON.stringify(environmentValues);
    const probeSettings = useMemo<Record<string, string>>(() => Object.fromEntries(JSON.parse(environmentKey)), [environmentKey]);
    const acpEnv = useAcpEnvironment(isAcpProvider && loadedId === id ? (acpCliCommand || "") : "", undefined, probeSettings);
    const codexEnv = useAcpEnvironment(isCodexAppServerProvider && loadedId === id ? "codex" : "",
        form.watch("codex_cli_command")?.trim() || "codex", probeSettings);
    const claudeEnv = useAcpEnvironment(isClaudeSdkProvider && loadedId === id ? "claude" : "",
        form.watch("claude_cli_command")?.trim() || acpCliCommand?.trim() || "claude", probeSettings);

    const saves = useMemo(() => {
        type Field = { value: string; revision: number; pending: boolean; timer?: ReturnType<typeof setTimeout>; running: boolean };
        const fields = new Map<string, Field>();
        const flush = (key: string) => {
            const field = fields.get(key)!;
            if (field.timer) clearTimeout(field.timer);
            field.timer = undefined;
            if (field.running || !field.pending) return;
            const { value, revision } = field;
            field.running = true;
            void invoke("update_llm_provider_config", { llmProviderId: id, name: key, value })
                .then(() => { if (field.revision === revision) field.pending = false; })
                .catch((error) => {
                    const detail = String(error);
                    const safeDetail = value ? detail.split(value).join("[已隐藏]") : detail;
                    toast.error(`保存提供商字段 ${key} 失败: ${safeDetail}`);
                })
                .finally(() => {
                    field.running = false;
                    if (field.revision !== revision && !field.timer) flush(key);
                });
        };
        return {
            update(key: string, value: string) {
                const field: Field = fields.get(key) ?? { value, revision: 0, pending: false, running: false };
                field.value = value;
                field.revision++;
                field.pending = true;
                fields.set(key, field);
                if (field.timer) clearTimeout(field.timer);
                field.timer = setTimeout(() => flush(key), 50);
            },
            snapshot: () => new Map(Array.from(fields, ([key, field]) => [key, { revision: field.revision, pending: field.pending }])),
            merge(config: Record<string, string>, snapshot?: Map<string, { revision: number; pending: boolean }>) {
                const merged = { ...defaultValues, ...config };
                if (isClaudeSdkProvider && !config.claude_cli_command?.trim() && config.acp_cli_command?.trim()) {
                    merged.claude_cli_command = config.acp_cli_command;
                }
                for (const [key, field] of fields) {
                    const previous = snapshot?.get(key);
                    if (field.pending || previous?.pending || (snapshot && field.revision !== previous?.revision)) {
                        Object.assign(merged, { [key]: field.value });
                    }
                }
                return merged;
            },
            flushAll() { for (const [key, field] of fields) if (field.timer) flush(key); },
        };
    }, [id, defaultValues, isClaudeSdkProvider]);
    const updateField = useCallback((key: string, value: string) => saves.update(key, value), [saves]);
    useEffect(() => () => saves.flushAll(), [saves]);

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

    useDataRefresh({
        events: ["llm_provider_changed"],
        identity: id,
        initial: true,
        refresh: async (isCurrent) => {
            const snapshot = saves.snapshot();
            let configArray: Array<LLMProviderConfig>;
            let modelList: Array<LLMModel>;
            try {
                [configArray, modelList] = await Promise.all([
                    invoke<Array<LLMProviderConfig>>("get_llm_provider_config", { id }),
                    invoke<Array<LLMModel>>("get_llm_models", { llmProviderId: String(id) }),
                ]);
            } catch (error) {
                // 读取失败也结束加载态（错误由 onError 提示），避免一直停在加载中
                if (isCurrent()) setLoadedId(id);
                throw error;
            }
            if (!isCurrent()) return;
            const config = Object.fromEntries(configArray.map((item) => [item.name, item.value]));
            const merged = saves.merge(config, snapshot);
            form.reset(merged);
            setHasApiKey(!!merged.api_key);
            setTags(modelList.map(toModelTagItem));
            setLoadedId(id);
        },
        onError: (error) => toast.error(`同步后刷新模型提供商失败（provider=${id}）: ${String(error)}`),
    });

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
                const selectedModelTags = selectedModels
                    .filter((model) => model.is_selected)
                    .map(toModelTagItem);
                setTags(selectedModelTags);
                setModelSelectionDialogOpen(false);

                toast.success("模型列表更新成功");
            } catch (e) {
                toast.error("更新模型列表失败: " + e);
            } finally {
                setIsUpdatingModels(false);
            }
        },
        [id],
    );

    const onTagsChange = useCallback((newTags: ModelTagItem[]) => {
        setTags(newTags);
    }, []);
    // 定义稳定的 customRender，不再依赖父组件的状态或函数
    const tagInputRender = useCallback(
        () => (
            <TagInputContainer
                llmProviderId={id}
                apiType={apiType}
                tags={tags}
                onTagsChange={onTagsChange}
                isExpanded={isModelListExpanded}
                onExpandedChange={setIsModelListExpanded}
                beforeFetchModels={() => {
                    const status = getProviderEndpointStatus(form.getValues("endpoint"));
                    if (status.kind === "invalid") throw new Error(status.message);
                }}
                onFetchModels={(modelData) => {
                    setModelSelectionData(modelData);
                    setModelSelectionDialogOpen(true);
                }}
            />
        ),
        [apiType, id, tags, onTagsChange, isModelListExpanded, form],
    );

    const renderProxyAdvancedConfig = useCallback(
        (description: string) => (
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
                        <ProviderProxySwitch
                            id={`provider-proxy-${id}`}
                            description={description}
                            configured={proxyConfigured}
                            checked={proxyEnabled === "true"}
                            onCheckedChange={(checked) => {
                                const value = checked ? "true" : "false";
                                form.setValue("proxy_enabled", value);
                                updateField("proxy_enabled", value);
                            }}
                        />
                    </div>
                </CollapsibleContent>
            </Collapsible>
        ),
        [form, id, isAdvancedConfigExpanded, proxyConfigured, proxyEnabled, updateField],
    );

    // Pi / Cursor / OpenCode / Kiro / Antigravity：自动检测运行环境与版本
    const nativeAgentFields = useNativeAgentProviderFields({ apiType, apiTypeLabel, providerId: id, form, active: loadedId === id });

    // 表单字段定义
    const configFields = useMemo(() => {
        const agentProxyField = {
            key: "advanced_config",
            config: { type: "custom" as const, label: "", value: "",
                customRender: () => renderProxyAdvancedConfig("使用全局网络代理启动此 Agent"),
            },
        };
        if (nativeAgentFields) return [...nativeAgentFields, agentProxyField];
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
                            <div className="flex flex-col gap-2">
                                {hasApiKey ? (
                                    <>
                                        <p className="flex items-center gap-1.5 text-sm">
                                            <CheckCircle2 className="h-4 w-4 text-muted-foreground" />
                                            已授权 GitHub Copilot
                                        </p>
                                        <Button
                                            type="button"
                                            size="sm"
                                            variant="outline"
                                            className="w-fit"
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
                                        {/* 授权码显示区域 */}
                                        {copilot.authInfo.userCode && (
                                            <div className="flex flex-col gap-2 rounded-md border border-border bg-muted/40 p-3">
                                                <div className="text-xs text-muted-foreground">
                                                    授权码 (User Code)
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    <code className="flex-1 text-lg font-mono font-bold bg-background px-3 py-2 rounded border border-border">
                                                        {copilot.authInfo.userCode}
                                                    </code>
                                                    <Button
                                                        iconLabel="复制授权码"
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
                                                <div className="text-xs text-muted-foreground">
                                                    请在浏览器中打开的页面输入此授权码
                                                </div>
                                            </div>
                                        )}

                                        {/* 三种授权方式：OAuth 为主，其余为备选 */}
                                        <div className="flex flex-wrap gap-2">
                                            <Button
                                                type="button"
                                                size="sm"
                                                disabled={copilot.isAuthorizing}
                                                onClick={() => {
                                                    copilot.oauthFlowAuth();
                                                }}
                                            >
                                                <KeyRound className="mr-1 h-4 w-4" />
                                                {copilot.isAuthorizing ? "授权中…" : "OAuth 授权"}
                                            </Button>
                                            <Button
                                                type="button"
                                                size="sm"
                                                variant="outline"
                                                disabled={copilot.isAuthorizing}
                                                onClick={async () => {
                                                    const candidates = await copilot.scanConfigAuth();
                                                    if (candidates.length === 0) {
                                                        return;
                                                    }

                                                    setScanCandidates(candidates);
                                                    setSelectedScanCandidateId(candidates[0].id);
                                                    setScanTokenDialogOpen(true);
                                                }}
                                            >
                                                <Search className="mr-1 h-4 w-4" />
                                                扫描已有授权
                                            </Button>
                                            <Button
                                                type="button"
                                                size="sm"
                                                variant="outline"
                                                disabled={copilot.isAuthorizing}
                                                onClick={() => {
                                                    setManualToken("");
                                                    setManualTokenDialogOpen(true);
                                                }}
                                            >
                                                <Edit className="mr-1 h-4 w-4" />
                                                手动输入 Token
                                            </Button>
                                        </div>

                                        <p className="text-xs text-muted-foreground">
                                            OAuth 授权通过浏览器完成 GitHub Device Flow；授权成功后 Token 自动保存，用于 Copilot API 调用。
                                        </p>
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
                                apiType={apiType}
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
                        customRender: () =>
                            renderProxyAdvancedConfig("启用后将使用全局网络代理配置进行模型请求"),
                    },
                },
            ];
        }

        if (isCodexAppServerProvider) {
            return [
                {
                    key: "apiType",
                    config: {
                        type: "static" as const,
                        label: "提供商类型",
                        value: "Codex",
                    },
                },
                {
                    key: "codex_cli_command",
                    config: {
                        type: "input" as const,
                        label: "Codex CLI 命令",
                        value: "codex",
                        placeholder: "codex 或绝对路径",
                        tooltip: "启动 Codex app-server 使用的 CLI 命令或绝对路径",
                    },
                },
                {
                    key: "codex_environment",
                    config: {
                        type: "custom" as const,
                        label: "环境状态",
                        value: "",
                        customRender: () => (
                            <CliEnvironmentStatus
                                env={codexEnv}
                                cliCommand="codex"
                                displayName="Codex CLI"
                            />
                        ),
                    },
                },
                {
                    key: "codex_approval_policy",
                    config: {
                        type: "select" as const,
                        label: "审批策略",
                        value: "on-request",
                        options: [...CODEX_APPROVAL_OPTIONS],
                    },
                },
                {
                    key: "codex_sandbox",
                    config: {
                        type: "select" as const,
                        label: "沙箱模式",
                        value: "workspace-write",
                        options: [...CODEX_SANDBOX_OPTIONS],
                    },
                },
                {
                    key: "codex_additional_args",
                    config: {
                        type: "input" as const,
                        label: "额外启动参数",
                        value: "",
                        placeholder: "例如 --enable collaboration_modes",
                    },
                },
                {
                    key: "acp_env_vars",
                    config: {
                        type: "textarea" as const,
                        label: "环境变量（JSON）",
                        value: "",
                        className: "min-h-24",
                        placeholder: '{"OPENAI_BASE_URL":"..."}',
                        tooltip: "默认复用 ~/.codex 登录态；这里只填写需要覆盖的环境变量",
                    },
                },
                agentProxyField,
            ];
        }

        if (isClaudeSdkProvider) {
            return [
                {
                    key: "apiType",
                    config: {
                        type: "static" as const,
                        label: "提供商类型",
                        value: "Claude Code",
                    },
                },
                {
                    key: "claude_cli_command",
                    config: {
                        type: "input" as const,
                        label: "Claude CLI 命令",
                        value: "claude",
                        placeholder: "claude 或绝对路径",
                        tooltip: "启动官方 Claude Code stream-json 通道使用的命令",
                    },
                },
                {
                    key: "claude_environment",
                    config: {
                        type: "custom" as const,
                        label: "环境状态",
                        value: "",
                        customRender: () => (
                            <CliEnvironmentStatus
                                env={claudeEnv}
                                cliCommand="claude"
                                displayName="Claude Code"
                            />
                        ),
                    },
                },
                {
                    key: "claude_permission_mode",
                    config: {
                        type: "select" as const,
                        label: "权限模式",
                        value: "default",
                        options: [
                            { value: "default", label: "默认询问" },
                            { value: "acceptEdits", label: "自动接受编辑" },
                            { value: "auto", label: "Claude Code 原生自动审核（auto）" },
                            { value: "plan", label: "计划模式" },
                            { value: "dontAsk", label: "不询问（自动拒绝未授权工具）" },
                        ],
                    },
                },
                {
                    key: "claude_effort",
                    config: {
                        type: "select" as const,
                        label: "思考强度",
                        value: "default",
                        options: [
                            { value: "default", label: "默认" },
                            { value: "low", label: "低" },
                            { value: "medium", label: "中" },
                            { value: "high", label: "高" },
                            { value: "xhigh", label: "极高" },
                            { value: "max", label: "最大" },
                        ],
                    },
                },
                {
                    key: "acp_env_vars",
                    config: {
                        type: "textarea" as const,
                        label: "环境变量（JSON）",
                        value: "",
                        className: "min-h-24",
                        placeholder: '{"ANTHROPIC_BASE_URL":"..."}',
                        tooltip: "默认复用本机 Claude Code 登录态；这里只填写需要覆盖的环境变量",
                    },
                },
                agentProxyField,
            ];
        }

        // ACP 提供商：特殊表单
        if (isAcpProvider) {
            // ACP 环境状态渲染
            const renderAcpEnvironmentStatus = () => {
                // 未选择 CLI 命令时不显示环境状态
                if (!acpCliCommand) {
                    return <p className="text-xs text-muted-foreground">请先选择 CLI 命令</p>;
                }

                // 选项标签去掉括号说明，例如 "Kimi Code CLI (原生支持)" → "Kimi Code CLI"
                const acpCliName = acpCliOptions
                    .find((option) => option.value === acpCliCommand)
                    ?.label.replace(/\s*\(.*\)$/, "") ?? acpCliCommand;
                return (
                    <CliEnvironmentStatus
                        env={acpEnv}
                        cliCommand={acpCliCommand}
                        displayName={acpCliName}
                    />
                );
            };

            return [
                {
                    key: "apiType",
                    config: {
                        type: "static" as const,
                        label: "提供商类型",
                        value: "ACP(Agent Client Protocol)",
                    },
                },
                {
                    key: "acp_cli_command",
                    config: {
                        type: "select" as const,
                        label: "CLI 命令",
                        value: "",
                        options: [...acpCliOptions],
                        tooltip: "选择要使用的 ACP 兼容 CLI 工具",
                    },
                },
                {
                    key: "acp_environment",
                    config: {
                        type: "custom" as const,
                        label: "环境状态",
                        value: "",
                        customRender: renderAcpEnvironmentStatus,
                    },
                },
                ...(acpCliCommand === "claude-code-acp"
                    ? [
                        {
                            key: "acp_claude_auth_mode",
                            config: {
                                type: "radio" as const,
                                label: "Claude 认证来源",
                                value: "claude_settings",
                                tooltip: "决定 claude-code-acp 启动时如何准备认证环境变量",
                                options: [
                                    {
                                        value: "claude_settings",
                                        label: "自动读取 ~/.claude/settings.json",
                                        tooltip:
                                            "读取 settings.json 中 env 对象里的环境变量，例如 ANTHROPIC_API_KEY、ANTHROPIC_BASE_URL",
                                    },
                                    {
                                        value: "env_vars",
                                        label: "手动填写环境变量",
                                        tooltip:
                                            "在下方按 KEY=VALUE 每行一条填写，将在启动 claude-code-acp 时注入",
                                    },
                                ],
                            },
                        },
                        ...(claudeAuthMode === "env_vars"
                            ? [
                                {
                                    key: "acp_claude_env_vars",
                                    config: {
                                        type: "textarea" as const,
                                        label: "Claude 环境变量",
                                        value: "",
                                        className: "min-h-32",
                                        placeholder:
                                            "ANTHROPIC_API_KEY=sk-ant-...\nANTHROPIC_BASE_URL=https://your-proxy.example.com",
                                        tooltip:
                                            "每行一个 KEY=VALUE；仅在“手动填写环境变量”模式下生效",
                                    },
                                },
                            ]
                            : []),
                    ]
                    : []),
                {
                    key: "advanced_config",
                    config: {
                        type: "custom" as const,
                        label: "",
                        value: "",
                        customRender: () =>
                            renderProxyAdvancedConfig(
                                "启用后将在启动 ACP agent client 时注入全局网络代理环境变量",
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
            ...(apiType === "opencode_go" ? [] : [{
                key: "endpoint",
                config: {
                    type: "input" as const,
                    label: "接口地址",
                    value: "",
                    inputMode: "url" as const,
                    hint: endpointStatus.kind === "invalid"
                        ? <span role="alert" className="text-destructive">{endpointStatus.message}</span>
                        : apiType === "magpie" && endpointStatus.kind === "unset"
                            ? <span>留空则连接本机 magpie（默认 127.0.0.1:3425，端口以 settings.json 为准）。</span>
                            : undefined,
                },
            }]),
            apiType === "magpie" ? {
                key: "api_key",
                config: {
                    type: "password" as const,
                    label: "API Key",
                    hint: <span>留空时使用 magpie-aipp，在 magpie 用量里记为本应用。访问另一台电脑上的网关时，改为那里的网关密钥。</span>,
                    value: "",
                },
            } : {
                key: "api_key",
                config: {
                    type: "custom" as const,
                    label: "API Key",
                    hint: apiKeyIsVaultRef
                        ? <span>值保存在凭据保险箱中，只会发往已确认的网站{apiType === "anthropic" ? "；Claude Code 助手不能使用保险箱中的 Key" : ""}。</span>
                        : apiType === "opencode_go"
                            ? <a href="https://opencode.ai/auth" target="_blank" rel="noreferrer">从 OpenCode Console 获取订阅 API Key</a>
                            : undefined,
                    value: "",
                    customRender: (field: { value?: string; onChange: (value: string) => void; name?: string }) => (
                        <VaultRefField
                            vault={vault}
                            id={field.name}
                            ariaLabel="API Key"
                            value={field.value ?? ""}
                            onChange={field.onChange}
                            suggestedName={`${name} · API Key`}
                            targetHint={vaultTarget}
                        />
                    ),
                },
            },
            ...(apiType === "deepseek" ? [{
                key: "deepseek_capabilities",
                config: {
                    type: "custom" as const,
                    label: "能力",
                    hint: <span>同一把 Key 可供内置搜索使用；搜索参数在「应用与工具 → 搜索工具」中调整。</span>,
                    value: "",
                    customRender: () => (
                        <DeepSeekCapabilities
                            webEnabled={webEnabled}
                            onWebChange={enabled => {
                                form.setValue("deepseek_web_enabled", String(enabled));
                                updateField("deepseek_web_enabled", String(enabled));
                            }}
                            showBalanceInChat={showBalanceInChat}
                            onShowBalanceInChatChange={enabled => {
                                form.setValue("deepseek_show_balance_in_chat", String(enabled));
                                updateField("deepseek_show_balance_in_chat", String(enabled));
                            }}
                            providerId={Number(id)}
                            providerName={name}
                            apiKey={apiKeyValue}
                            endpoint={endpoint ?? ""}
                            vault={vault}
                            onKeyShared={handleKeyShared}
                        />
                    ),
                },
            }] : []),
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
                    customRender: () =>
                        renderProxyAdvancedConfig("启用后将使用全局网络代理配置进行模型请求"),
                },
            },
        ];
    }, [nativeAgentFields, apiType, apiTypeLabel, isCopilotProvider, isAcpProvider, isCodexAppServerProvider, isClaudeSdkProvider, acpCliOptions, tagInputRender, renderProxyAdvancedConfig, hasApiKey, copilot.authInfo, copilot.isAuthorizing, copilot.scanConfigAuth, copilot.oauthFlowAuth, copilot.cancelAuthorization, id, tags, onTagsChange, acpCliCommand, claudeAuthMode, acpEnv, codexEnv, claudeEnv, endpointStatus, vault, apiKeyIsVaultRef, vaultTarget, name, apiKeyValue, endpoint, handleKeyShared, webEnabled, showBalanceInChat, form, updateField]);

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
                                iconLabel="分享提供商"
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

                <Button
                    iconLabel="删除提供商"
                    variant="ghost"
                    size="sm"
                    onClick={onDelete}
                    className="hover:bg-destructive/10"
                >
                    <Trash2 className="h-4 w-4 mr-1" />
                </Button>
            </div>
        ),
        [enabled, onToggleEnabled, index, onDelete, onShare],
    );

    // 表单部分结束
    return (
        <>
            {loadedId === id ? (
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
            ) : (
                // 首次进入立即显示（衔接列表加载）；切换提供商时延迟显示，读取很快时不闪烁
                <ConfigPageLoading delayMs={loadedId === null ? 0 : 200} />
            )}
            <ModelSelectionDialog
                open={modelSelectionDialogOpen}
                onOpenChange={setModelSelectionDialogOpen}
                modelData={modelSelectionData}
                onConfirm={handleModelSelectionConfirm}
                loading={isUpdatingModels}
            />
            {/* 手动输入 Token 对话框 */}
            <Dialog open={manualTokenDialogOpen} onOpenChange={setManualTokenDialogOpen}>
                <CompactDialogContent className="sm:max-w-md">
                    <CompactDialogHeader
                        title="输入 OAuth Token"
                        description="请输入 GitHub Copilot 的 OAuth Token (ghu_ 或 gho_ 开头)"
                    />
                    <CompactDialogBody className="space-y-3">
                        <p className="text-xs text-muted-foreground">
                            请输入 GitHub Copilot 的 OAuth Token (ghu_ 或 gho_ 开头)
                        </p>
                        <Input
                            autoFocus
                            autoComplete="off"
                            enterKeyHint="done"
                            placeholder="ghu_xxxxxxxxx 或 gho_xxxxxxxxx..."
                            value={manualToken}
                            onChange={(e) => setManualToken(e.target.value)}
                            className="font-mono"
                        />
                    </CompactDialogBody>
                    <CompactDialogFooter>
                        <Button
                            type="button"
                            variant="outline"
                            className="min-h-11 px-4 sm:min-h-9"
                            onClick={() => setManualTokenDialogOpen(false)}
                        >
                            取消
                        </Button>
                        <Button
                            type="button"
                            className="min-h-11 px-5 sm:min-h-9"
                            onClick={async () => {
                                await copilot.manualTokenAuth(manualToken);
                                setManualTokenDialogOpen(false);
                            }}
                            disabled={!manualToken.trim()}
                        >
                            保存
                        </Button>
                    </CompactDialogFooter>
                </CompactDialogContent>
            </Dialog>
            <Dialog
                open={scanTokenDialogOpen}
                onOpenChange={(open) => {
                    setScanTokenDialogOpen(open);
                    if (!open) {
                        setScanCandidates([]);
                        setSelectedScanCandidateId("");
                    }
                }}
            >
                <CompactDialogContent className="sm:max-w-2xl">
                    <CompactDialogHeader
                        title="选择扫描到的 Copilot 授权"
                        subtitle={scanCandidates.length > 0 ? `${scanCandidates.length} 个候选` : undefined}
                        description="选择当前机器上扫描到的 Copilot 授权候选"
                    />
                    <CompactDialogBody className="space-y-3">
                        <p className="text-xs text-muted-foreground">
                            下面列出了当前机器上扫描到的授权候选。仅展示前 5 位，后续内容已脱敏。请选择你想导入的 Token。
                        </p>
                        <RadioGroup
                            value={selectedScanCandidateId}
                            onValueChange={setSelectedScanCandidateId}
                            className="space-y-1.5"
                        >
                            {scanCandidates.map((candidate) => (
                                <label
                                    key={candidate.id}
                                    className={`flex cursor-pointer items-start gap-2.5 rounded-lg border px-3 py-2.5 transition-colors hover:bg-muted/60 ${
                                        selectedScanCandidateId === candidate.id
                                            ? "border-foreground/40 bg-muted/60"
                                            : "border-border"
                                    }`}
                                >
                                    <RadioGroupItem
                                        value={candidate.id}
                                        id={candidate.id}
                                        className="mt-0.5"
                                    />
                                    <div className="min-w-0 space-y-0.5 text-sm">
                                        <div className="font-mono font-medium">
                                            {candidate.maskedToken}
                                        </div>
                                        <div className="text-xs text-muted-foreground">
                                            来源：{candidate.source}
                                        </div>
                                        <div className="break-all text-xs text-muted-foreground">
                                            位置：{candidate.location}
                                        </div>
                                    </div>
                                </label>
                            ))}
                        </RadioGroup>
                    </CompactDialogBody>
                    <CompactDialogFooter>
                        <Button
                            type="button"
                            variant="outline"
                            className="min-h-11 px-4 sm:min-h-9"
                            onClick={() => setScanTokenDialogOpen(false)}
                        >
                            取消
                        </Button>
                        <Button
                            type="button"
                            className="min-h-11 px-5 sm:min-h-9"
                            onClick={async () => {
                                if (!selectedScanCandidate) {
                                    toast.error("请先选择一个授权候选");
                                    return;
                                }

                                await copilot.applyScannedTokenAuth(selectedScanCandidate);
                                setScanTokenDialogOpen(false);
                            }}
                            disabled={!selectedScanCandidate || copilot.isAuthorizing}
                        >
                            导入所选 Token
                        </Button>
                    </CompactDialogFooter>
                </CompactDialogContent>
            </Dialog>
            {/* 改名对话框 */}
            <Dialog open={renameDialogOpen} onOpenChange={setRenameDialogOpen}>
                <CompactDialogContent className="sm:max-w-md">
                    <CompactDialogHeader
                        title="重命名提供商"
                        description="请输入新的提供商名称"
                    />
                    <CompactDialogBody className="space-y-3">
                        <p className="text-xs text-muted-foreground">
                            请输入新的提供商名称
                        </p>
                        <Input
                            autoFocus
                            autoComplete="off"
                            enterKeyHint="done"
                            placeholder="请输入提供商名称"
                            value={newProviderName}
                            onChange={(e) => setNewProviderName(e.target.value)}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter' && !e.nativeEvent.isComposing && newProviderName.trim()) {
                                    handleConfirmRename();
                                }
                            }}
                        />
                    </CompactDialogBody>
                    <CompactDialogFooter>
                        <Button
                            type="button"
                            variant="outline"
                            className="min-h-11 px-4 sm:min-h-9"
                            onClick={() => setRenameDialogOpen(false)}
                        >
                            取消
                        </Button>
                        <Button
                            type="button"
                            className="min-h-11 px-5 sm:min-h-9"
                            onClick={handleConfirmRename}
                            disabled={!newProviderName.trim()}
                        >
                            确定
                        </Button>
                    </CompactDialogFooter>
                </CompactDialogContent>
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
