import { useDataRefresh } from "@/hooks/useDataRefresh";
import React, { useCallback, useRef, useState, useMemo } from 'react';
import { invoke } from "@tauri-apps/api/core";
import LLMProviderConfigForm from "./LLMProviderConfigForm";
import FormDialog from "../FormDialog";
import ConfirmDialog from "../ConfirmDialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import ShareDialog from "../ShareDialog";
import ImportDialog from "../ImportDialog";
import PasswordDialog from "../PasswordDialog";
import { Button } from "../ui/button";
import { Tooltip, TooltipTrigger, TooltipContent } from "../ui/tooltip";
import { Zap, Settings, ServerCrash, Download, Plus } from "lucide-react";
import { toast } from 'sonner';
import { PinyinFilter, usePinyinReady } from "../../utils/pinyinFilter";
import { useFeatureAvailableOnPlatform } from "@/lib/mobileUnsupported";

// 导入公共组件
import {
    ConfigPageLayout,
    SidebarList,
    ListItemButton,
    EmptyState,
    ConfigPageLoading,
    SelectOption
} from "../common";

interface LLMProvider {
    id: string;
    name: string;
    api_type: string;
    description: string;
    is_official: boolean;
    is_enabled: boolean;
}

const LOCAL_CLI_AGENT_API_TYPES: ReadonlySet<string> = new Set(['acp', 'codex_app_server', 'claude_sdk']);

/** 新增提供商名称。键盘「完成」和确认按钮共用：空或重名时返回提示，不创建。 */
export function newProviderNameError(name: string, existingNames: readonly string[]): string | null {
    const trimmed = name.trim();
    if (!trimmed) return "请填写提供商名称";
    if (existingNames.some((existing) => existing.trim() === trimmed)) return "已有同名提供商";
    return null;
}

const LLMProviderConfig: React.FC = () => {
    const [LLMProviders, setLLMProviders] = useState<Array<LLMProvider>>([]);
    const [selectedProvider, setSelectedProvider] = useState<LLMProvider | null>(null);
    // 首次列表读取完成前不渲染空状态，避免先闪「还没有配置提供商」
    const [providersLoaded, setProvidersLoaded] = useState(false);
    const [searchQuery, setSearchQuery] = useState('');
    // 有搜索输入时才动态加载 pinyin-pro，加载完成后触发重新过滤
    const pinyinReady = usePinyinReady(searchQuery.trim().length > 0);

    const handleToggle = useCallback((index: number) => {
        const newProviders = [...LLMProviders];
        newProviders[index].is_enabled = !newProviders[index].is_enabled;
        setLLMProviders(newProviders);

        invoke('update_llm_provider', {
            id: LLMProviders[index].id,
            name: LLMProviders[index].name,
            apiType: LLMProviders[index].api_type,
            description: LLMProviders[index].description,
            isEnabled: newProviders[index].is_enabled
        });

        // 如果是当前选中的提供商，更新选中状态
        if (selectedProvider && selectedProvider.id === LLMProviders[index].id) {
            setSelectedProvider(newProviders[index]);
        }
    }, [LLMProviders, selectedProvider]);

    const getLLMProviderList = useCallback((isCurrent: () => boolean = () => true) => {
        return invoke<Array<LLMProvider>>('get_llm_providers')
            .then((providers) => {
                if (!isCurrent()) return;
                setLLMProviders(providers);
                setSelectedProvider((current) => providers.find((p) => p.id === current?.id) ?? providers[0] ?? null);
            })
            .catch((e) => {
                toast.error('获取大模型提供商失败: ' + e);
            })
            .finally(() => {
                if (isCurrent()) setProvidersLoaded(true);
            });
    }, []);

    useDataRefresh({
        events: ["llm_provider_changed"],
        initial: true,
        refresh: async (isCurrent) => { await getLLMProviderList(isCurrent); },
    });

    const [newProviderDialogOpen, setNewProviderDialogOpen] = useState(false);
    const [providerName, setProviderName] = useState('');
    const [providerNameError, setProviderNameError] = useState<string | null>(null);
    const [formApiType, setFormApiType] = useState('openai_api');
    const creatingProviderRef = useRef(false);
    const apiTypes = [
        { value: 'openai_api', label: 'OpenAI API' },
        { value: 'ollama', label: 'Ollama API' },
        { value: 'anthropic', label: 'Anthropic API' },
        { value: 'cohere', label: 'Cohere API' },
        { value: 'deepseek', label: 'DeepSeek API' },
        { value: 'opencode_go', label: 'OpenCode Go' },
        { value: 'github_copilot', label: 'GitHub Copilot' },
        { value: 'acp', label: 'ACP (Agent Client Protocol)' },
        { value: 'codex_app_server', label: 'Codex' },
        { value: 'claude_sdk', label: 'Claude Code' },
    ]
    // 本地 CLI Agent 类型仅桌面可新建；已有记录仍按 apiTypes 显示名称
    const localCliAgentAvailable = useFeatureAvailableOnPlatform("local_cli_agent");
    const creatableApiTypes = localCliAgentAvailable
        ? apiTypes
        : apiTypes.filter((type) => !LOCAL_CLI_AGENT_API_TYPES.has(type.value));

    const openNewProviderDialog = useCallback(() => {
        setProviderNameError(null);
        setNewProviderDialogOpen(true);
    }, []);

    const closeNewProviderDialog = useCallback(() => {
        setProviderNameError(null);
        setNewProviderDialogOpen(false);
    }, []);

    const handleNewProviderSubmit = useCallback(() => {
        // 键盘「完成 / Enter」与确认按钮相同：校验通过才提交。输入法组字中的 Enter 不走这里。
        const error = newProviderNameError(providerName, LLMProviders.map((provider) => provider.name));
        if (error) {
            setProviderNameError(error);
            return;
        }
        if (creatingProviderRef.current) return;
        creatingProviderRef.current = true;
        const name = providerName.trim();
        invoke('add_llm_provider', {
            name,
            apiType: formApiType
        }).then(() => {
            toast.success('添加大模型提供商成功');
            setProviderName('');
            setProviderNameError(null);
            setFormApiType('openai_api');
            closeNewProviderDialog();
            getLLMProviderList();
        }).catch((e) => {
            toast.error('添加大模型提供商失败: ' + e);
        }).finally(() => {
            creatingProviderRef.current = false;
        });
    }, [providerName, formApiType, LLMProviders, closeNewProviderDialog, getLLMProviderList]);

    const [confirmDialogIsOpen, setConfirmDialogIsOpen] = useState(false);
    const [deleteLLMProviderId, setDeleteLLMProviderId] = useState("");

    const onConfirmDeleteProvider = useCallback(() => {
        if (!deleteLLMProviderId) {
            return;
        }
        invoke('delete_llm_provider', { llmProviderId: deleteLLMProviderId }).then(() => {
            toast.success('删除大模型提供商成功');
            getLLMProviderList();
        }).catch(e => {
            toast.error('删除大模型提供商失败: ' + e);
        });
        closeConfirmDialog();
    }, [deleteLLMProviderId, getLLMProviderList]);

    const openConfirmDialog = useCallback((LLMProviderId: string) => {
        setConfirmDialogIsOpen(true)
        setDeleteLLMProviderId(LLMProviderId);
    }, []);

    const closeConfirmDialog = useCallback(() => {
        setConfirmDialogIsOpen(false)
    }, []);

    // 分享和导入相关状态
    const [shareDialogOpen, setShareDialogOpen] = useState(false);
    const [importDialogOpen, setImportDialogOpen] = useState(false);
    const [passwordDialogOpen, setPasswordDialogOpen] = useState(false);
    const [shareCode, setShareCode] = useState('');

    // 重命名提供商
    const handleRenameProvider = useCallback(async (newName: string) => {
        if (!selectedProvider) return;

        try {
            await invoke('update_llm_provider', {
                id: selectedProvider.id,
                name: newName,
                apiType: selectedProvider.api_type,
                description: selectedProvider.description,
                isEnabled: selectedProvider.is_enabled
            });
            toast.success('重命名成功');

            // 立即更新 selectedProvider 的名称
            setSelectedProvider(prev => prev ? { ...prev, name: newName } : null);

            // 同时更新列表
            getLLMProviderList();
        } catch (error) {
            toast.error('重命名失败: ' + error);
        }
    }, [selectedProvider, getLLMProviderList]);

    // 分享提供商
    const handleShareProvider = useCallback(async () => {
        if (!selectedProvider) return;
        setPasswordDialogOpen(true);
    }, [selectedProvider]);

    // 确认导出（设置密码后）
    const handleConfirmExport = useCallback(async (password: string) => {
        if (!selectedProvider) return;

        try {
            const code = await invoke<string>('export_llm_provider', {
                providerId: selectedProvider.id,
                password
            });
            setShareCode(code);
            setShareDialogOpen(true);
        } catch (error) {
            toast.error('分享失败: ' + error);
            throw error; // 重新抛出错误给PasswordDialog处理
        }
    }, [selectedProvider]);

    // 导入提供商
    const handleImportProvider = useCallback(async (
        shareCode: string,
        password?: string,
        newName?: string
    ) => {
        if (!password) {
            throw new Error('请输入密码');
        }

        await invoke('import_llm_provider', {
            shareCode,
            password,
            newName
        });

        // 导入成功后重新获取提供商列表
        getLLMProviderList();
    }, [getLLMProviderList]);

    // 关闭分享对话框
    const closeShareDialog = useCallback(() => {
        setShareDialogOpen(false);
        setShareCode('');
    }, []);

    // 关闭导入对话框
    const closeImportDialog = useCallback(() => {
        setImportDialogOpen(false);
    }, []);

    // 关闭密码对话框
    const closePasswordDialog = useCallback(() => {
        setPasswordDialogOpen(false);
    }, []);

    // 选择提供商
    const handleSelectProvider = useCallback((provider: LLMProvider) => {
        setSelectedProvider(provider);
    }, []);

    // 下拉菜单选项
    const selectOptions: SelectOption[] = useMemo(() =>
        LLMProviders.map(provider => ({
            id: provider.id,
            label: provider.name,
            icon: provider.is_enabled ? <Zap className="h-4 w-4" /> : <Settings className="h-4 w-4" />
        })), [LLMProviders]);

    // 下拉菜单选择回调
    const handleSelectFromDropdown = useCallback((providerId: string) => {
        const provider = LLMProviders.find(p => p.id === providerId);
        if (provider) {
            handleSelectProvider(provider);
        }
    }, [LLMProviders, handleSelectProvider]);

    // 新增按钮组件
    const addButton = useMemo(() => (
        <div className="flex gap-2">
            <Tooltip delayDuration={500}>
                <TooltipTrigger asChild>
                    <Button
                        onClick={openNewProviderDialog}
                        aria-label="添加提供商"
                        className="gap-2 bg-primary hover:bg-primary/90 text-primary-foreground shadow-sm hover:shadow-md transition-all"
                    >
                        <Plus className="h-4 w-4" />
                    </Button>
                </TooltipTrigger>
                <TooltipContent>添加提供商</TooltipContent>
            </Tooltip>
            <Tooltip delayDuration={500}>
                <TooltipTrigger asChild>
                    <Button
                        variant="outline"
                        onClick={() => setImportDialogOpen(true)}
                        className="shadow-sm hover:shadow-md transition-all"
                    >
                        <Download className="h-4 w-4" />
                    </Button>
                </TooltipTrigger>
                <TooltipContent>导入提供商</TooltipContent>
            </Tooltip>
        </div>
    ), [openNewProviderDialog]);

    // 侧边栏内容 - 使用 useMemo 避免重复创建（必须在条件返回之前）
    const filteredProviders = useMemo(() => {
        if (!searchQuery.trim()) return LLMProviders;
        return LLMProviders.filter(provider =>
            PinyinFilter.matches(provider.name, searchQuery) ||
            PinyinFilter.matches(provider.api_type, searchQuery)
        );
    }, [LLMProviders, searchQuery, pinyinReady]);

    const sidebar = useMemo(() => (
        <SidebarList
            title="模型提供商"
            description="选择提供商进行配置"
            icon={<ServerCrash className="h-5 w-5" />}
            searchValue={searchQuery}
            onSearchChange={setSearchQuery}
            searchPlaceholder="搜索提供商..."
            addButton={addButton}
        >
            {filteredProviders.map((provider) => (
                <ListItemButton
                    key={provider.id}
                    isSelected={selectedProvider?.id === provider.id}
                    onClick={() => handleSelectProvider(provider)}
                >
                    <div className="flex items-center w-full">
                        <div className="flex-1 truncate">
                            <div className="font-medium truncate">{provider.name}</div>
                        </div>
                        {provider.is_enabled && (
                            <Zap className="h-3 w-3 ml-2 flex-shrink-0" />
                        )}
                    </div>
                </ListItemButton>
            ))}
        </SidebarList>
    ), [filteredProviders, selectedProvider?.id, handleSelectProvider, addButton, searchQuery]);

    const selectedProviderApiType = selectedProvider ? apiTypes.find(type => type.value === selectedProvider.api_type)?.label || selectedProvider.api_type : "";

    // 右侧内容 - 使用 useMemo 避免重复创建（必须在条件返回之前）
    const content = useMemo(() => selectedProvider ? (
        <div className="space-y-6">
            <LLMProviderConfigForm
                id={selectedProvider.id}
                index={LLMProviders.findIndex(p => p.id === selectedProvider.id)}
                apiType={selectedProvider.api_type}
                name={selectedProvider.name}
                description={selectedProvider.description || `${selectedProviderApiType} 提供商配置`}
                isOffical={selectedProvider.is_official}
                enabled={selectedProvider.is_enabled}
                onToggleEnabled={handleToggle}
                onDelete={() => openConfirmDialog(selectedProvider.id)}
                onShare={handleShareProvider}
                onRename={handleRenameProvider}
            />
        </div>
    ) : (
        <EmptyState
            icon={<Settings className="h-8 w-8 text-muted-foreground" />}
            title="选择一个提供商"
            description="从左侧列表中选择一个提供商开始配置"
        />
    ), [selectedProvider, LLMProviders, selectedProviderApiType, handleToggle, openConfirmDialog, handleShareProvider, handleRenameProvider]);

    if (!providersLoaded) {
        return <ConfigPageLoading />;
    }

    // 空状态
    if (LLMProviders.length === 0) {
        return (
            <>
                <ConfigPageLayout
                    sidebar={null}
                    content={
                        <EmptyState
                            icon={<ServerCrash className="h-8 w-8 text-muted-foreground" />}
                            title="还没有配置提供商"
                            description="开始添加你的第一个 AI 模型提供商，享受智能助手的强大功能"
                            action={
                                <div className="flex flex-col gap-3">
                                    <div className="flex gap-2 justify-center">
                                        <Button
                                            onClick={openNewProviderDialog}
                                            className="gap-2 bg-primary hover:bg-primary/90 text-primary-foreground shadow-lg hover:shadow-xl transition-all"
                                        >
                                            <Plus className="h-4 w-4" />
                                            添加第一个提供商
                                        </Button>
                                        <Tooltip delayDuration={500}>
                                            <TooltipTrigger asChild>
                                                <Button
                                                    variant="outline"
                                                    onClick={() => setImportDialogOpen(true)}
                                                    className="shadow-lg hover:shadow-xl transition-all"
                                                >
                                                    <Download className="h-4 w-4" />
                                                </Button>
                                            </TooltipTrigger>
                                            <TooltipContent>导入提供商</TooltipContent>
                                        </Tooltip>
                                    </div>
                                </div>
                            }
                        />
                    }
                />

                {/* 导入对话框 */}
                <ImportDialog
                    title="提供商配置"
                    isOpen={importDialogOpen}
                    requiresPassword={true}
                    onClose={closeImportDialog}
                    onImport={handleImportProvider}
                />
            </>
        );
    }

    return (
        <>
            <ConfigPageLayout
                sidebar={sidebar}
                content={content}
                selectOptions={selectOptions}
                selectedOptionId={selectedProvider?.id}
                onSelectOption={handleSelectFromDropdown}
                selectPlaceholder="选择提供商"
                addButton={addButton}
            />

            {/* 新增提供商对话框 */}
            <FormDialog
                title='新增模型提供商'
                isOpen={newProviderDialogOpen}
                onClose={closeNewProviderDialog}
                onSubmit={handleNewProviderSubmit}
            >
                <div className="space-y-5">
                    <div className="space-y-2">
                        <label htmlFor="new-provider-name" className="text-sm font-medium text-foreground">提供商名称</label>
                        <input
                            id="new-provider-name"
                            className="w-full px-3 py-2 border border-input rounded-lg focus:outline-none focus:ring-2 focus:ring-ring focus:border-ring transition-colors bg-background text-foreground"
                            type="text"
                            autoCapitalize="none"
                            enterKeyHint="done"
                            placeholder="例如：我的 OpenAI"
                            value={providerName}
                            aria-invalid={providerNameError ? true : undefined}
                            aria-describedby={providerNameError ? "new-provider-name-error" : undefined}
                            onChange={e => {
                                setProviderName(e.target.value);
                                if (providerNameError) setProviderNameError(null);
                            }}
                            onKeyDown={event => {
                                if (event.key !== "Enter" || event.nativeEvent.isComposing || event.keyCode === 229) return;
                                event.preventDefault();
                                handleNewProviderSubmit();
                            }}
                        />
                        {providerNameError && (
                            <p id="new-provider-name-error" role="alert" className="text-sm text-destructive">
                                {providerNameError}
                            </p>
                        )}
                    </div>
                    <div className="space-y-2">
                        <label className="text-sm font-medium text-foreground">API 调用类型</label>
                        <Select value={formApiType} onValueChange={setFormApiType}>
                            <SelectTrigger className="w-full">
                                <SelectValue placeholder="选择 API 类型" />
                            </SelectTrigger>
                            <SelectContent>
                                {creatableApiTypes.map((type) => (
                                    <SelectItem key={type.value} value={type.value}>
                                        {type.label}
                                    </SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                    </div>
                </div>
            </FormDialog>

            {/* 确认删除对话框 */}
            <ConfirmDialog
                isOpen={confirmDialogIsOpen}
                title='确认删除'
                confirmText='确定要删除这个提供商吗？删除后相关配置将无法恢复。'
                onConfirm={onConfirmDeleteProvider}
                onCancel={closeConfirmDialog}
            />

            {/* 密码设置对话框 */}
            <PasswordDialog
                title={selectedProvider?.name || "提供商"}
                isOpen={passwordDialogOpen}
                onClose={closePasswordDialog}
                onConfirm={handleConfirmExport}
            />

            {/* 分享对话框 */}
            <ShareDialog
                title="提供商配置"
                shareCode={shareCode}
                isOpen={shareDialogOpen}
                onClose={closeShareDialog}
            />

            {/* 导入对话框 */}
            <ImportDialog
                title="提供商配置"
                isOpen={importDialogOpen}
                requiresPassword={true}
                onClose={closeImportDialog}
                onImport={handleImportProvider}
            />
        </>
    );
}

export default LLMProviderConfig;
