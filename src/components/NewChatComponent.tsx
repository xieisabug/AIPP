import AskWindowPrepare from "./AskWindowPrepare";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { useIsMobile } from "../hooks/use-mobile";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

interface AssistantListItem {
    id: number;
    name: string;
    assistant_type?: number;
}

interface AgentModelOption { code: string; name: string; provider_id: number; efforts: string[]; default_effort?: string | null; }

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
                <SelectTrigger className="w-100 mt-4" data-aipp-slot="chat-new-conversation-assistant-select">
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
                <div className="mt-3 flex w-100 flex-col gap-1">
                    <div className="flex items-center gap-2">
                    <Select value={selectedModel} onValueChange={(value) => {
                        const model = models.find((item) => item.code === value);
                        const nextEfforts = model?.efforts ?? [];
                        setEfforts(nextEfforts);
                        const nextEffort = model?.default_effort && nextEfforts.includes(model.default_effort)
                            ? model.default_effort
                            : nextEfforts.includes(selectedEffort) ? selectedEffort : (nextEfforts[0] ?? "");
                        onAgentConfigChange(value, nextEffort, codexDefaults?.approval_policy ?? "", codexDefaults?.sandbox ?? "");
                    }}>
                        <SelectTrigger className="min-w-0 flex-1" disabled={loadingConfig || models.length === 0}><SelectValue placeholder={loadingConfig ? "加载模型" : "选择模型"} /></SelectTrigger>
                        <SelectContent>{models.map((model) => <SelectItem key={model.code} value={model.code}>{model.name}</SelectItem>)}</SelectContent>
                    </Select>
                    <Select value={selectedEffort} onValueChange={(value) => onAgentConfigChange(selectedModel, value, selectedApprovalPolicy, selectedSandbox)}>
                        <SelectTrigger className="min-w-0 flex-1" disabled={loadingConfig || efforts.length === 0}><SelectValue placeholder={loadingConfig ? "加载强度" : "选择强度"} /></SelectTrigger>
                        <SelectContent>{efforts.map((effort) => <SelectItem key={effort} value={effort}>{effort}</SelectItem>)}</SelectContent>
                    </Select>
                    </div>
                    {codexDefaults ? (
                        <div className="flex items-center gap-2">
                            <Select value={selectedApprovalPolicy} onValueChange={(value) => onAgentConfigChange(selectedModel, selectedEffort, value, selectedSandbox)}>
                                <SelectTrigger className="min-w-0 flex-1"><SelectValue placeholder="审批策略" /></SelectTrigger>
                                <SelectContent>
                                    <SelectItem value="untrusted">仅读命令自动执行（untrusted）</SelectItem>
                                    <SelectItem value="on-request">由模型决定何时请求审批（on-request）</SelectItem>
                                    <SelectItem value="never">从不请求审批（never）</SelectItem>
                                </SelectContent>
                            </Select>
                            <Select value={selectedSandbox} onValueChange={(value) => onAgentConfigChange(selectedModel, selectedEffort, selectedApprovalPolicy, value)}>
                                <SelectTrigger className="min-w-0 flex-1"><SelectValue placeholder="沙箱模式" /></SelectTrigger>
                                <SelectContent>
                                    <SelectItem value="read-only">只读（read-only）</SelectItem>
                                    <SelectItem value="workspace-write">工作区可写（workspace-write）</SelectItem>
                                    <SelectItem value="danger-full-access">完全访问（danger-full-access）</SelectItem>
                                </SelectContent>
                            </Select>
                        </div>
                    ) : null}
                    {configError ? <span className="text-xs text-muted-foreground">{configError}</span> : null}
                </div>
            ) : null}
        </div>
    );
};

export default NewChatComponent;
