/**
 * 新建提供商可选的 API 调用类型。
 *
 * - `http`：通过接口地址 + API Key 访问模型服务，向导第三步会获取模型列表。
 * - `copilot`：GitHub 账号授权，创建后在详情页完成授权。
 * - `local`：本机 CLI / SDK 智能体，创建后在详情页检测运行环境；移动端不可新建。
 */
import catalog from "./providerApiTypeCatalog.json";

export function isLocalAgentApiType(value: string): boolean {
    return findProviderApiType(value)?.kind === "local";
}

export type ProviderApiTypeKind = "http" | "copilot" | "local";

export interface ProviderApiTypeOption {
    value: string;
    label: string;
    description: string;
    kind: ProviderApiTypeKind;
    /** 与后端 `genai_client::DEFAULT_ENDPOINTS` 一致；向导第二步预填，可修改。 */
    defaultEndpoint?: string;
    /** 该类型没有接口地址字段（固定路由）。 */
    noEndpoint?: boolean;
}

export const PROVIDER_API_TYPES: readonly ProviderApiTypeOption[] = catalog as ProviderApiTypeOption[];

export function findProviderApiType(value: string): ProviderApiTypeOption | undefined {
    return PROVIDER_API_TYPES.find((type) => type.value === value);
}

/** 新提供商的建议名称：类型名，已被占用时追加序号。 */
export function suggestProviderName(label: string, existingNames: readonly string[]): string {
    const taken = new Set(existingNames.map((name) => name.trim()));
    if (!taken.has(label)) return label;
    for (let index = 2; ; index++) {
        const candidate = `${label} ${index}`;
        if (!taken.has(candidate)) return candidate;
    }
}

/** 新增提供商名称校验。键盘「完成」和确认按钮共用：空或重名时返回提示，不创建。 */
export function newProviderNameError(name: string, existingNames: readonly string[]): string | null {
    const trimmed = name.trim();
    if (!trimmed) return "请填写提供商名称";
    if (existingNames.some((existing) => existing.trim() === trimmed)) return "已有同名提供商";
    return null;
}

/**
 * ACP CLI 命令选项。大多数 AI 需要安装专门的 ACP 适配器：
 * - Claude Code: npm install -g @zed-industries/claude-code-acp -> 命令 claude-code-acp
 * - Gemini / Kimi Code CLI: 原生支持 ACP（kimi 启动时自动附加 acp 子命令）
 * - DeepSeek Harness: npm install -g @deepseek-ai/dsh dsh-acp-server -> 命令 dsh-acp-server
 */
export const ACP_CLI_OPTIONS = [
    { value: "claude-code-acp", label: "Claude Code (需安装 @zed-industries/claude-code-acp)" },
    { value: "gemini", label: "Gemini CLI (原生支持)" },
    { value: "kimi", label: "Kimi Code CLI (原生支持)" },
    { value: "dsh-acp-server", label: "DeepSeek Harness (需安装 dsh-acp-server)" },
] as const;

export const CODEX_APPROVAL_OPTIONS = [
    { value: "on-request", label: "按需询问" },
    { value: "never", label: "从不询问" },
    { value: "untrusted", label: "不可信操作询问" },
] as const;

export const CODEX_SANDBOX_OPTIONS = [
    { value: "read-only", label: "只读" },
    { value: "workspace-write", label: "工作区可写" },
    { value: "danger-full-access", label: "完全访问" },
] as const;

/** 后端 read_local_agent_defaults 能读取自身配置文件的本地智能体。 */
export const LOCAL_AGENT_DEFAULTS_READABLE: ReadonlySet<string> = new Set(["codex_app_server", "claude_sdk", "kiro", "pi", "opencode"]);

/** read_local_agent_defaults 的返回值。 */
export interface LocalAgentDefaults {
    source: string | null;
    found: boolean;
    /** 写入新提供商的配置（AIPP 不写就会用自己的默认值覆盖） */
    settings: Record<string, string>;
    /** 仅展示：AIPP 不覆盖，智能体按自身配置运行 */
    info: Array<{ label: string; value: string }>;
    notes: string[];
}
