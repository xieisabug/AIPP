import React from "react";
import { AlertTriangle, Check, Loader2 } from "lucide-react";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import type { AcpCliStatus, LocalAgentStatus } from "./useLocalAgentDetection";
import {
    CODEX_APPROVAL_OPTIONS,
    CODEX_SANDBOX_OPTIONS,
    type LocalAgentDefaults,
    type ProviderApiTypeOption,
} from "./providerApiTypes";

export type DefaultsState =
    | { status: "loading" }
    | { status: "done"; defaults: LocalAgentDefaults }
    | { status: "error"; message: string };

interface LocalAgentSetupProps {
    apiType: ProviderApiTypeOption;
    status: LocalAgentStatus | undefined;
    acpClis: AcpCliStatus[] | null;
    /** 不能读取配置文件的智能体为 undefined */
    defaults: DefaultsState | undefined;
    /** 将写入新提供商的设置 */
    values: Record<string, string>;
    onChange: (key: string, value: string) => void;
}

/* ── 展示原语：分区标题、键值列表、上标签下拉框 ── */

const Section: React.FC<{ title: string; aside?: React.ReactNode; children: React.ReactNode; testId?: string }> = ({ title, aside, children, testId }) => (
    <section className="space-y-2" data-testid={testId}>
        <div className="flex min-w-0 items-baseline justify-between gap-3">
            <h3 className="shrink-0 text-xs font-medium text-muted-foreground">{title}</h3>
            {aside}
        </div>
        <div className="space-y-3">{children}</div>
    </section>
);

/** 只读的键值列表：标签在左、值在右，行间分隔线。 */
const KeyValueList: React.FC<{ children: React.ReactNode }> = ({ children }) => (
    <dl className="divide-y divide-border overflow-hidden rounded-lg border border-border">{children}</dl>
);

const KeyValue: React.FC<{ label: string; children: React.ReactNode; title?: string }> = ({ label, children, title }) => (
    <div className="flex min-h-9 min-w-0 items-center justify-between gap-4 px-3 py-1.5">
        <dt className="shrink-0 text-xs text-muted-foreground">{label}</dt>
        <dd className="min-w-0 truncate text-right text-sm" title={title}>{children}</dd>
    </div>
);

/** 只有一行文字时用的占位行（检测中、未找到等）。 */
const StatusLine: React.FC<{ children: React.ReactNode; role?: string }> = ({ children, role }) => (
    <div role={role} className="flex min-h-9 items-center gap-1.5 px-3 py-1.5 text-sm text-muted-foreground">{children}</div>
);

const SelectField: React.FC<{
    id: string;
    label: string;
    value: string;
    options: ReadonlyArray<{ value: string; label: string }>;
    onChange: (value: string) => void;
    placeholder?: string;
}> = ({ id, label, value, options, onChange, placeholder }) => (
    <div className="min-w-0 space-y-1.5">
        <label htmlFor={id} className="text-sm font-medium text-foreground">{label}</label>
        <Select value={value} onValueChange={onChange}>
            <SelectTrigger id={id} aria-label={label} className="w-full">
                <SelectValue placeholder={placeholder} />
            </SelectTrigger>
            <SelectContent>
                {options.map((option) => (
                    <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>
                ))}
            </SelectContent>
        </Select>
    </div>
);

/* ── 本机环境 ── */

const EnvironmentList: React.FC<{ status: LocalAgentStatus | undefined }> = ({ status }) => {
    if (!status || status.state === "detecting") {
        return <KeyValueList><StatusLine><Loader2 className="h-3.5 w-3.5 animate-spin" />正在检测本机安装…</StatusLine></KeyValueList>;
    }
    if (status.state === "error") {
        return (
            <KeyValueList>
                <StatusLine role="alert"><span className="break-all text-xs text-destructive">检测失败：{status.message}</span></StatusLine>
            </KeyValueList>
        );
    }
    return (
        <KeyValueList>
            {status.state === "installed" ? (
                <>
                    <KeyValue label="程序">
                        <span className="inline-flex max-w-full items-center gap-1.5">
                            <Check className="h-3.5 w-3.5 shrink-0" />
                            <span className="truncate">{`${status.program}${status.version ? ` ${status.version}` : ""}`}</span>
                        </span>
                    </KeyValue>
                    {status.path && (
                        <KeyValue label="位置" title={status.path}>
                            <span className="font-mono text-xs text-muted-foreground">{status.path}</span>
                        </KeyValue>
                    )}
                </>
            ) : (
                <StatusLine>{status.error
                    ? `${status.program} 检测失败：${status.error}`
                    : `未检测到 ${status.program}，创建后可在详情页查看安装方法`}</StatusLine>
            )}
            {status.node && (
                <KeyValue label="Node.js">
                    {status.node.ok ? (
                        <span className="inline-flex items-center gap-1.5">
                            <Check className="h-3.5 w-3.5 shrink-0" />{status.node.version ? `v${status.node.version}` : "可用"}
                        </span>
                    ) : (
                        <span className="text-muted-foreground">需要 {status.node.required} 或更新版本</span>
                    )}
                </KeyValue>
            )}
        </KeyValueList>
    );
};

/** 向导第二步：本地智能体的本机检测结果与从其自身配置文件读取的默认配置。 */
const LocalAgentSetup: React.FC<LocalAgentSetupProps> = ({ apiType, status, acpClis, defaults, values, onChange }) => {
    const isCodex = apiType.value === "codex_app_server";
    const isAcp = apiType.value === "acp";
    const loaded = defaults?.status === "done" ? defaults.defaults : null;
    const sourceLabel = loaded?.source ? (
        <span className="min-w-0 truncate text-xs text-muted-foreground" title={loaded.source}>
            {loaded.found ? "来自 " : "未找到 "}
            <span className="font-mono">{loaded.source}</span>
        </span>
    ) : null;

    return (
        <div className="space-y-5">
            <Section title="本机环境">
                {isAcp ? (
                    acpClis === null ? (
                        <EnvironmentList status={{ state: "detecting" }} />
                    ) : (
                        <SelectField
                            id="new-provider-acp-cli"
                            label="CLI 命令"
                            value={values.acp_cli_command ?? ""}
                            placeholder="选择 ACP CLI"
                            options={acpClis.map((cli) => ({
                                value: cli.value,
                                label: `${cli.label} · ${cli.installed ? `已安装${cli.version ? ` ${cli.version}` : ""}` : "未安装"}`,
                            }))}
                            onChange={(value) => onChange("acp_cli_command", value)}
                        />
                    )
                ) : (
                    <EnvironmentList status={status} />
                )}
            </Section>

            {defaults && (
                <Section title="默认配置" aside={sourceLabel} testId="local-agent-defaults">
                    {defaults.status === "loading" && (
                        <KeyValueList><StatusLine><Loader2 className="h-3.5 w-3.5 animate-spin" />正在读取…</StatusLine></KeyValueList>
                    )}
                    {defaults.status === "error" && (
                        <p role="alert" className="break-all text-xs text-destructive">读取默认配置失败：{defaults.message}</p>
                    )}

                    {isCodex && defaults.status !== "loading" && (
                        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                            <SelectField
                                id="new-provider-codex-approval"
                                label="审批策略"
                                value={values.codex_approval_policy}
                                options={CODEX_APPROVAL_OPTIONS}
                                onChange={(value) => onChange("codex_approval_policy", value)}
                            />
                            <SelectField
                                id="new-provider-codex-sandbox"
                                label="沙箱模式"
                                value={values.codex_sandbox}
                                options={CODEX_SANDBOX_OPTIONS}
                                onChange={(value) => onChange("codex_sandbox", value)}
                            />
                        </div>
                    )}

                    {loaded && loaded.info.length > 0 && (
                        <KeyValueList>
                            {loaded.info.map((item) => (
                                <KeyValue key={item.label} label={item.label} title={item.value}>{item.value}</KeyValue>
                            ))}
                        </KeyValueList>
                    )}
                    {loaded && !isCodex && loaded.info.length === 0 && loaded.notes.length === 0 && (
                        <KeyValueList>
                            <StatusLine>
                                {loaded.found ? "配置文件中没有可显示的默认项" : `没有自定义配置，${apiType.label} 将使用它自己的默认值`}
                            </StatusLine>
                        </KeyValueList>
                    )}

                    {loaded?.notes.map((note) => (
                        <p key={note} className="flex items-start gap-1.5 break-all text-xs text-muted-foreground">
                            <AlertTriangle className="mt-0.5 h-3 w-3 shrink-0" />{note}
                        </p>
                    ))}
                    {defaults.status !== "loading" && (
                        <p className="text-xs text-muted-foreground">
                            {isCodex
                                ? `审批策略与沙箱模式由 AIPP 启动 ${apiType.label} 时指定，已按它的配置预填，可修改。`
                                : `AIPP 不覆盖这些设置，${apiType.label} 按自己的配置运行，之后在本机修改也会生效。`}
                        </p>
                    )}
                </Section>
            )}
        </div>
    );
};

export default LocalAgentSetup;
