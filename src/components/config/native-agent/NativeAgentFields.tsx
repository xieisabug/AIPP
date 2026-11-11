import React, { useCallback, useMemo } from "react";
import { useWatch, type UseFormReturn } from "react-hook-form";
import { toast } from "sonner";
import { Download } from "lucide-react";
import { Button } from "@/components/ui/button";
import { FolderPicker } from "@/components/config/FolderPicker";
import AntigravityLogin from "@/components/config/AntigravityLogin";
import {
    NATIVE_AGENT_DETECTION_KEYS,
    useNativeAgentEnvironment,
    type NativeAgentEnvironmentState,
} from "@/hooks/feature/useNativeAgentEnvironment";
import { InstallGuide, UpdatePanel } from "./UpdatePanel";
import { CursorLogin } from "./CursorLogin";
import { PathInput, ProgramPicker, ProgramStatus, samePath } from "./ProgramPicker";

export const NATIVE_AGENT_API_TYPES = ["pi", "cursor", "opencode", "kiro", "antigravity"];
const WATCHED_KEYS = [...NATIVE_AGENT_DETECTION_KEYS, "antigravity_auth_method"] as const;

type FieldRenderData = { value?: unknown; onChange: (value: string) => void };
const text = (value: unknown) => (typeof value === "string" ? value : "");

const Notes: React.FC<{ notes: string[] }> = ({ notes }) =>
    notes.length ? (
        <ul className="flex flex-col gap-0.5 text-xs text-muted-foreground">
            {notes.map((note) => <li key={note}>{note}</li>)}
        </ul>
    ) : null;

export const NodeRuntimeField: React.FC<{
    state: NativeAgentEnvironmentState;
    value: string;
    onChange: (value: string) => void;
}> = ({ state, value, onChange }) => {
    const node = state.environment?.node;
    const selected = node?.selected ?? null;
    const usable = Boolean(selected?.ok && !selected.warning);
    const suggestion = usable ? undefined : node?.candidates.find((candidate) => candidate.ok && !candidate.warning);
    return (
        <div className="flex flex-col gap-2">
            <ProgramPicker
                value={value}
                autoValues={["", "node"]}
                autoLabel="自动（PATH 中的 node）"
                selected={state.environment ? selected ?? { path: "node", source: "PATH", version: null, ok: false, warning: null, error: "PATH 中没有找到 node" } : null}
                candidates={node?.candidates ?? []}
                detecting={state.detecting}
                onChange={onChange}
                browseTitle="选择 Node.js 可执行文件"
                customPlaceholder="node 可执行文件的绝对路径"
                ariaLabel="Node.js 运行时"
            />
            {state.detectError && (
                <p role="alert" className="whitespace-pre-wrap break-all text-xs text-destructive">环境检测失败：{state.detectError}</p>
            )}
            {suggestion && (
                <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                    <span>检测到可用的 Node.js v{suggestion.version}（{suggestion.source}）</span>
                    <Button type="button" size="sm" variant="outline" className="h-7" onClick={() => onChange(suggestion.path)}>改用此版本</Button>
                </div>
            )}
            {state.environment && !suggestion && !usable && (
                <InstallGuide
                    intro={`未找到 Node.js ${node?.required ?? ""} 或更新版本。安装后点击「重新检测」，或选择自定义路径。`}
                    commands={[]}
                    docsUrl="https://nodejs.org/"
                />
            )}
        </div>
    );
};

export const AgentProgramField: React.FC<{
    state: NativeAgentEnvironmentState;
    value: string;
    onChange: (value: string) => void;
}> = ({ state, value, onChange }) => {
    const tool = state.environment?.tool;
    const command = tool?.default_command ?? "";
    // Without a working Node the candidates are unverified, so "not installed" would be a guess.
    const nodeReady = Boolean(state.environment?.node.selected?.ok);
    const missing = tool && nodeReady && !tool.selected?.ok && !tool.candidates.some((candidate) => candidate.ok);
    return (
        <div className="flex flex-col gap-2">
            <ProgramPicker
                value={value}
                autoValues={["", command]}
                autoLabel={`自动（PATH 中的 ${command || "程序"}）`}
                selected={tool?.selected ?? null}
                candidates={tool?.candidates ?? []}
                detecting={state.detecting}
                onChange={onChange}
                browseTitle={`选择 ${tool?.display_name ?? "智能体"} 程序`}
                customPlaceholder="可执行文件或 JavaScript 入口的绝对路径"
                ariaLabel={`${tool?.display_name ?? "智能体"} 程序`}
            />
            {missing && tool.install_commands.length > 0 && (
                <InstallGuide
                    intro={`未找到 ${tool.display_name}。在终端运行以下命令安装，然后点击「重新检测」：`}
                    commands={tool.install_commands}
                    docsUrl={tool.docs_url}
                />
            )}
            {tool && <Notes notes={tool.warnings} />}
            <UpdatePanel state={state} displayName={tool?.display_name ?? "智能体"} />
        </div>
    );
};

export const CursorSdkField: React.FC<{
    state: NativeAgentEnvironmentState;
    value: string;
    onChange: (value: string) => void;
}> = ({ state, value, onChange }) => {
    const tool = state.environment?.tool;
    const selected = tool?.selected ?? null;
    const managed = tool?.candidates.find((candidate) => candidate.source === "AIPP 管理");
    const nodeReady = Boolean(state.environment?.node.selected?.ok);
    const install = async () => {
        try {
            const directory = await state.runUpdate(false);
            if (directory) onChange(directory);
            toast.success("Cursor SDK 已安装");
        } catch (error) {
            toast.error("Cursor SDK 安装失败", { description: String(error).slice(0, 300) });
        }
    };
    return (
        <div className="flex flex-col gap-2">
            <FolderPicker value={value} onChange={onChange} placeholder="留空则从工作目录向上查找 node_modules/@cursor/sdk" />
            <ProgramStatus program={selected} detecting={state.detecting} />
            {managed && selected && !samePath(managed.path, selected.path) && (
                <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                    <span>AIPP 管理目录中已安装 v{managed.version}</span>
                    <Button type="button" size="sm" variant="outline" className="h-7" onClick={() => onChange(managed.path)}>使用此目录</Button>
                </div>
            )}
            {tool && selected && !selected.ok && !managed && !state.updating && (
                <div className="flex flex-col gap-2 rounded-md border border-border bg-muted/40 p-3">
                    <p className="text-xs text-muted-foreground">
                        {value.trim()
                            ? "将用所选 Node.js 自带的 npm 把 @cursor/sdk 安装到所选目录。"
                            : `将用所选 Node.js 自带的 npm 把 @cursor/sdk 安装到 AIPP 管理目录（${tool.managed_directory ?? "应用数据目录"}），并自动填入上方目录。`}
                    </p>
                    <Button type="button" size="sm" className="w-fit" disabled={!nodeReady} onClick={() => void install()}>
                        <Download className="mr-1 h-4 w-4" />一键安装 Cursor SDK
                    </Button>
                </div>
            )}
            {tool && <Notes notes={tool.warnings} />}
            <UpdatePanel state={state} displayName="Cursor SDK" onUpdated={(directory) => directory && onChange(directory)} />
        </div>
    );
};

interface Options {
    apiType: string;
    apiTypeLabel: string;
    providerId: string;
    form: UseFormReturn<any>;
    /** False until the provider's saved settings are in the form, so detection never runs on placeholders. */
    active?: boolean;
}

/**
 * Form fields for Pi / Cursor / OpenCode / Kiro / Antigravity providers: detected Node.js
 * and agent program pickers, update status, folder pickers. Returns null for other types.
 */
export function useNativeAgentProviderFields({ apiType, apiTypeLabel, providerId, form, active = true }: Options) {
    const enabled = NATIVE_AGENT_API_TYPES.includes(apiType);
    const watched = useWatch({ control: form.control, name: WATCHED_KEYS as unknown as string[] }) as unknown[];
    const values = Object.fromEntries(WATCHED_KEYS.map((key, index) => [key, text(watched?.[index])])) as Record<(typeof WATCHED_KEYS)[number], string>;
    const settingsKey = JSON.stringify(NATIVE_AGENT_DETECTION_KEYS.map((key) => values[key]));
    const settings = useMemo(
        () => Object.fromEntries(NATIVE_AGENT_DETECTION_KEYS.map((key) => [key, values[key]])),
        // eslint-disable-next-line react-hooks/exhaustive-deps
        [settingsKey],
    );
    const state = useNativeAgentEnvironment(enabled && active ? apiType : "", settings, 400, providerId);
    const workingDirectory = state.environment?.working_directory;
    const standalone = apiType === "antigravity" && values.antigravity_runtime === "standalone";
    const authMethod = values.antigravity_auth_method || "oauth-personal";
    const formSnapshot = useCallback(
        () => Object.fromEntries(Object.entries(form.getValues()).filter((entry): entry is [string, string] => typeof entry[1] === "string")),
        [form],
    );

    return useMemo(() => {
        if (!enabled) return null;
        const custom = (label: string, render: (field: FieldRenderData) => React.ReactNode, tooltip?: string) => ({
            type: "custom" as const, label, value: "", tooltip, customRender: render,
        });
        const programField = standalone
            ? [
                { key: "agent_cli_command", config: custom("独立 ACP 程序", (field) => (
                    <div className="flex flex-col gap-2">
                        <PathInput value={text(field.value)} onChange={field.onChange} browseTitle="选择 Antigravity 独立 ACP 程序" placeholder="可执行文件的绝对路径" />
                        <ProgramStatus program={state.environment?.tool.selected ?? null} detecting={state.detecting} />
                    </div>
                ), "独立程序由你自行安装和更新") },
                { key: "antigravity_harness", config: custom("配套 Harness", (field) => (
                    <PathInput value={text(field.value)} onChange={field.onChange} browseTitle="选择 localharness_external" placeholder="相同版本 localharness_external 的绝对路径" />
                ), "仅独立 ACP 程序需要：选择相同版本的 localharness_external") },
            ]
            : apiType === "cursor"
                ? [
                    { key: "agent_sdk_directory", config: custom("Cursor SDK", (field) => <CursorSdkField state={state} value={text(field.value)} onChange={field.onChange} />,
                        "Cursor 通过官方 @cursor/sdk 在本机执行；可一键安装到 AIPP 管理目录，或选择已有的安装目录") },
                    { key: "cursor_login", config: custom("Cursor 账号", () => <CursorLogin providerId={providerId} state={state} settings={formSnapshot} />,
                        "用浏览器登录 Cursor 账号（需要付费计划），无需手动复制 API Key") },
                    { key: "cursor_api_key", config: { type: "password" as const, label: "Cursor API Key（可选）", value: "", placeholder: "不登录时可直接填写 crsr_…",
                        tooltip: "填写后优先于浏览器登录；环境变量中的 CURSOR_API_KEY 会覆盖此处" } },
                    { key: "cursor_runtime_notice", config: { type: "static" as const, label: "执行策略", value: "使用 Cursor SDK 自动审核，不显示 AIPP 审批卡片。" } },
                ]
                : [
                    { key: "agent_cli_command", config: custom(`${apiTypeLabel} 程序`, (field) => <AgentProgramField state={state} value={text(field.value)} onChange={field.onChange} />,
                        "默认按 PATH 和常见安装位置自动查找；也可固定为某个安装或自定义路径") },
                ];

        return [
            { key: "apiType", config: { type: "static" as const, label: "提供商类型", value: apiTypeLabel } },
            { key: "agent_node_command", config: custom("Node.js 运行时", (field) => <NodeRuntimeField state={state} value={text(field.value)} onChange={field.onChange} />,
                "原生智能体通过 Node.js 桥接运行，需要 22.19 或更新版本。选择具体安装后，它的目录会优先加入会话 PATH") },
            ...(apiType === "antigravity" ? [
                { key: "antigravity_runtime", config: { type: "select" as const, label: "运行程序", value: "cli",
                    options: [{ value: "cli", label: "官方 agy CLI（acp 子命令）" }, { value: "standalone", label: "独立 ACP 程序" }] } },
            ] : []),
            ...programField,
            ...(apiType === "kiro" ? [
                { key: "kiro_agent", config: { type: "input" as const, label: "Kiro Agent", value: "", placeholder: "留空使用 Kiro 当前 Agent",
                    tooltip: "填写原生 mode 选项中的 Agent ID，不可用时明确报错" } },
                { key: "kiro_effort", config: { type: "input" as const, label: "思考强度", value: "", placeholder: "留空使用模型默认，例如 low / medium / high / xhigh / max",
                    tooltip: "填写模型公布的 effortLevel，不支持时明确报错" } },
            ] : []),
            ...(apiType === "antigravity" ? [
                { key: "antigravity_auth_method", config: { type: "select" as const, label: "认证方式", value: "oauth-personal",
                    options: [{ value: "oauth-personal", label: "Google 个人账号" }, { value: "gemini-api-key", label: "Gemini API Key" }] } },
                ...(authMethod === "gemini-api-key" ? [
                    { key: "antigravity_api_key", config: { type: "password" as const, label: "Gemini API Key", value: "", tooltip: "在 Google AI Studio 创建" } },
                ] : []),
                { key: "antigravity_login", config: custom("认证", () => <AntigravityLogin key={providerId} providerId={providerId} settings={formSnapshot} />) },
            ] : []),
            { key: "acp_working_directory", config: { type: "folder-picker" as const, label: "默认工作目录", value: "", placeholder: "留空使用用户主目录",
                tooltip: "助手可单独覆盖",
                hint: workingDirectory?.configured && !workingDirectory.exists
                    ? <span role="alert" className="text-destructive">目录不存在：{workingDirectory.resolved}</span>
                    : undefined } },
            { key: "acp_env_vars", config: { type: "textarea" as const, label: "环境变量（可选）", value: "", placeholder: "KEY=VALUE，每行一个",
                tooltip: apiType === "antigravity" ? "支持每行 KEY=VALUE 或 JSON；账号与认证目录由上方认证设置管理" : "支持每行 KEY=VALUE 或 JSON 对象；默认复用本机登录与配置" } },
        ];
    }, [enabled, apiType, apiTypeLabel, providerId, standalone, authMethod, state, workingDirectory, formSnapshot]);
}
