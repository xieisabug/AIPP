import AskWindowPrepare from "./AskWindowPrepare";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";
import { useIsMobile } from "../hooks/use-mobile";
import { invoke } from "@tauri-apps/api/core";
import { Check, ChevronDown, Box, Cpu, Brain, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import type { ReactNode } from "react";

interface AssistantListItem {
    id: number;
    name: string;
    assistant_type?: number;
}

interface AgentModelOption { code: string; name: string; provider_id: number; efforts: string[]; default_effort?: string | null; }

interface AgentSelectOption {
    value: string;
    label: string;
}

interface AgentCompactSelectProps {
    value: string;
    options: AgentSelectOption[];
    placeholder: string;
    icon: ReactNode;
    disabled?: boolean;
    onChange: (value: string) => void;
}

function AgentCompactSelect({ value, options, placeholder, icon, disabled, onChange }: AgentCompactSelectProps) {
    const [open, setOpen] = useState(false);
    const currentLabel = options.find((option) => option.value === value)?.label ?? (value || placeholder);

    return (
        <Popover open={open} onOpenChange={setOpen}>
            <PopoverTrigger asChild>
                <button
                    type="button"
                    disabled={disabled}
                    className="flex h-8 min-w-0 flex-1 items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground outline-none transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50"
                    aria-label={placeholder}
                >
                    <span className="shrink-0 text-foreground/70">{icon}</span>
                    <span className="min-w-0 flex-1 truncate text-left">{currentLabel}</span>
                    <ChevronDown className="h-3.5 w-3.5 shrink-0 opacity-60" />
                </button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-56 p-1">
                {options.map((option) => {
                    const selected = option.value === value;
                    return (
                        <button
                            key={option.value}
                            type="button"
                            className={`flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none hover:bg-accent hover:text-accent-foreground ${selected ? "bg-accent text-accent-foreground" : ""}`}
                            onClick={() => {
                                onChange(option.value);
                                setOpen(false);
                            }}
                        >
                            <span className="min-w-0 flex-1 truncate">{option.label}</span>
                            {selected ? <Check className="h-4 w-4 shrink-0" /> : null}
                        </button>
                    );
                })}
            </PopoverContent>
        </Popover>
    );
}

interface NewChatComponentProps {
    selectedText: string;
    selectedAssistant: number;
    setSelectedAssistant: (assistantId: number) => void;
    assistants: AssistantListItem[];
    selectedModel: string;
    selectedEffort: string;
    selectedApprovalPolicy: string;
    selectedSandbox: string;
    onAgentConfigChange: (model: string, effort: string, approvalPolicy: string, sandbox: string) => void;
}

const NewChatComponent: React.FC<NewChatComponentProps> = ({
    selectedText,
    selectedAssistant,
    setSelectedAssistant,
    assistants,
    selectedModel,
    selectedEffort,
    selectedApprovalPolicy,
    selectedSandbox,
    onAgentConfigChange,
}: NewChatComponentProps) => {
    const isMobile = useIsMobile();
    const [models, setModels] = useState<AgentModelOption[]>([]);
    const [efforts, setEfforts] = useState<string[]>([]);
    const [isAgentAssistant, setIsAgentAssistant] = useState(false);
    const [codexDefaults, setCodexDefaults] = useState<{ approval_policy: string; sandbox: string } | null>(null);
    const [loadingConfig, setLoadingConfig] = useState(false);
    const [configError, setConfigError] = useState<string | null>(null);
    const selectedAssistantInfo = assistants.find((assistant) => assistant.id === selectedAssistant);

    useEffect(() => {
        let cancelled = false;
        const load = async () => {
            if (!selectedAssistant || selectedAssistant < 0) {
                setIsAgentAssistant(false);
                setConfigError(null);
                setModels([]); setEfforts([]); setCodexDefaults(null); onAgentConfigChange("", "", "", ""); return;
            }
            setLoadingConfig(true);
            setConfigError(null);
            try {
                const detail = await invoke<any>("get_assistant", { assistantId: selectedAssistant });
                const isAgent = detail?.assistant?.assistant_type === 4 || detail?.assistant_type === 4 || selectedAssistantInfo?.assistant_type === 4;
                setIsAgentAssistant(isAgent);
                if (!isAgent) {
                    setModels([]); setEfforts([]); setCodexDefaults(null); onAgentConfigChange("", "", "", ""); return;
                }
                const modelList = await invoke<AgentModelOption[]>("get_agent_model_options", { assistantId: selectedAssistant });
                if (cancelled) return;
                const nextModels = modelList;
                const configuredCode = detail?.model?.[0]?.model_code;
                const configured = configuredCode
                    ? nextModels.find((model) => model.code.startsWith(`${configuredCode}%%`))
                    : undefined;
                const initialModel = configured ?? nextModels[0];
                const firstEfforts = initialModel?.efforts ?? [];
                const configuredEffort = detail?.model_configs?.find((config: { name?: string; value?: string | null }) => config.name === "reasoning_effort")?.value;
                const initialEffort = initialModel?.default_effort && firstEfforts.includes(initialModel.default_effort)
                    ? initialModel.default_effort
                    : configuredEffort && firstEfforts.includes(configuredEffort)
                        ? configuredEffort
                        : (firstEfforts[0] ?? "");
                const codex = await invoke<{ approval_policy: string; sandbox: string } | null>("get_codex_agent_defaults", { assistantId: selectedAssistant });
                if (cancelled) return;
                setCodexDefaults(codex ?? null);
                setModels(nextModels);
                setEfforts(firstEfforts);
                onAgentConfigChange(initialModel?.code ?? "", initialEffort, codex?.approval_policy ?? "", codex?.sandbox ?? "");
            } catch {
                if (!cancelled) { setIsAgentAssistant(selectedAssistantInfo?.assistant_type === 4); setModels([]); setEfforts([]); setCodexDefaults(null); setConfigError("无法读取 Agent 可用模型，请检查对应 CLI 配置"); }
            } finally {
                if (!cancelled) setLoadingConfig(false);
            }
        };
        void load();
        return () => { cancelled = true; };
    }, [selectedAssistant, selectedAssistantInfo?.assistant_type]);

    // 移动端不需要拖动区域
    const dragProps = isMobile ? {} : { "data-tauri-drag-region": true };

    return (
        <div
            className="relative flex flex-col items-center justify-center h-full select-none p-10 theme-background-image"
            data-aipp-slot="chat-new-conversation"
            {...dragProps}
        >

            <div className="text-sm text-gray-500 text-center mb-4" data-aipp-slot="chat-new-conversation-hint" {...dragProps}>
                <AskWindowPrepare selectedText={selectedText} isMobile={isMobile} />
                <p className="mt-4" {...dragProps}>
                    请选择一个对话，或者选择一个助手开始新聊天
                </p>
            </div>
            <Select
                value={selectedAssistant.toString()}
                onValueChange={(value) => setSelectedAssistant(Number(value))}
            >
                <SelectTrigger className="mt-4 w-88" data-aipp-slot="chat-new-conversation-assistant-select">
                    <SelectValue placeholder="选择一个助手" />
                </SelectTrigger>
                <SelectContent>
                    {assistants.map((assistant) => (
                        <SelectItem key={assistant.id} value={assistant.id.toString()}>
                            {assistant.name}
                        </SelectItem>
                    ))}
                </SelectContent>
            </Select>
            {isAgentAssistant ? (
                <div className="mt-3 flex w-88 flex-col gap-1">
                    <div className="flex items-center gap-2">
                        <AgentCompactSelect
                            value={selectedModel}
                            options={models.map((model) => ({ value: model.code, label: model.name }))}
                            placeholder={loadingConfig ? "加载模型" : "选择模型"}
                            icon={<Cpu className="h-3.5 w-3.5" />}
                            disabled={loadingConfig || models.length === 0}
                            onChange={(value) => {
                                const model = models.find((item) => item.code === value);
                                const nextEfforts = model?.efforts ?? [];
                                setEfforts(nextEfforts);
                                const nextEffort = model?.default_effort && nextEfforts.includes(model.default_effort)
                                    ? model.default_effort
                                    : nextEfforts.includes(selectedEffort) ? selectedEffort : (nextEfforts[0] ?? "");
                                onAgentConfigChange(value, nextEffort, codexDefaults?.approval_policy ?? "", codexDefaults?.sandbox ?? "");
                            }}
                        />
                        <AgentCompactSelect
                            value={selectedEffort}
                            options={efforts.map((effort) => ({ value: effort, label: effort }))}
                            placeholder={loadingConfig ? "加载强度" : "选择强度"}
                            icon={<Brain className="h-3.5 w-3.5" />}
                            disabled={loadingConfig || efforts.length === 0}
                            onChange={(value) => onAgentConfigChange(selectedModel, value, selectedApprovalPolicy, selectedSandbox)}
                        />
                    </div>
                    {codexDefaults ? (
                        <div className="flex items-center gap-2">
                            <AgentCompactSelect
                                value={selectedApprovalPolicy}
                                options={[
                                    { value: "untrusted", label: "仅读命令自动执行" },
                                    { value: "on-request", label: "按需请求审批" },
                                    { value: "never", label: "从不请求审批" },
                                ]}
                                placeholder="审批策略"
                                icon={<Box className="h-3.5 w-3.5" />}
                                onChange={(value) => onAgentConfigChange(selectedModel, selectedEffort, value, selectedSandbox)}
                            />
                            <AgentCompactSelect
                                value={selectedSandbox}
                                options={[
                                    { value: "read-only", label: "只读" },
                                    { value: "workspace-write", label: "工作区可写" },
                                    { value: "danger-full-access", label: "完全访问" },
                                ]}
                                placeholder="沙箱模式"
                                icon={<ShieldCheck className="h-3.5 w-3.5" />}
                                onChange={(value) => onAgentConfigChange(selectedModel, selectedEffort, selectedApprovalPolicy, value)}
                            />
                        </div>
                    ) : null}
                    {configError ? <span className="text-xs text-muted-foreground">{configError}</span> : null}
                </div>
            ) : null}
        </div>
    );
};

export default NewChatComponent;
