import { invoke } from "@tauri-apps/api/core";
import type { MCPServer, MCPServerRequest } from "@/data/MCP";
import type { VaultCreateInput, VaultItem } from "@/data/Vault";
import { formatVaultRef, parseVaultValue } from "./vault/vaultRefs";
import { suggestKey } from "./vault/vaultHelpers";
import { isLocalSearch } from "./searchToolFields";

/**
 * DeepSeek 全家桶（需求 335）：一把 DeepSeek Key 同时给模型服务商与内置搜索（Search + Fetch）用。
 *
 * 只经过设置页已有的保存命令（update_llm_provider_config / update_mcp_server /
 * add_or_update_aipp_builtin_server）写配置，保险箱引用的去向确认因此与手动配置完全一致。
 */

export const SEARCH_COMMAND = "aipp:search";
export const DEEPSEEK_OFFICIAL_ORIGIN = "https://api.deepseek.com";
const SEARCH_PATH = "/v1/tools/search";

const HTTP_PROVIDER_LABELS: Record<string, string> = {
    deepseek: "DeepSeek",
    tavily: "Tavily",
    brave: "Brave Search",
    searxng: "SearXNG",
};

/** KEY=VALUE 文本，与后端搜索配置的解析一致（逐行、首个 = 分隔、忽略空行与 # 注释）。 */
export function parseEnvText(text: string | null | undefined): Array<[string, string]> {
    const out: Array<[string, string]> = [];
    for (const line of (text ?? "").split(/\r?\n/)) {
        const trimmed = line.trim();
        if (!trimmed || trimmed.startsWith("#")) continue;
        const eq = trimmed.indexOf("=");
        if (eq <= 0) continue;
        out.push([trimmed.slice(0, eq).trim(), trimmed.slice(eq + 1).trim()]);
    }
    return out;
}

function envValue(text: string | null | undefined, name: string): string {
    return parseEnvText(text).find(([key]) => key === name)?.[1] ?? "";
}

/**
 * 服务商地址对应的搜索地址。官方地址（或留空）返回空串，表示用内置默认值；
 * 自定义地址（中转）沿用它的源站，让搜索与模型请求发往同一个已确认的网站。
 */
export function deepSeekSearchEndpoint(providerEndpoint: string | null | undefined): string {
    const raw = (providerEndpoint ?? "").trim();
    if (!raw) return "";
    let origin: string;
    try {
        origin = new URL(raw).origin;
    } catch {
        return "";
    }
    return origin === DEEPSEEK_OFFICIAL_ORIGIN ? "" : `${origin}${SEARCH_PATH}`;
}

/** 在保留其它设置的前提下，把 HTTP 搜索与网页抓取切到 DeepSeek。 */
export function buildDeepSeekSearchEnv(existing: string | null | undefined, apiKey: string, providerEndpoint: string | null | undefined): string {
    const endpoint = deepSeekSearchEndpoint(providerEndpoint);
    const updates: Record<string, string | null> = {
        HTTP_SEARCH_PROVIDER: "deepseek",
        HTTP_SEARCH_ENDPOINT: endpoint || null,
        HTTP_SEARCH_API_KEY: apiKey,
        FETCH_PROVIDER: "deepseek",
    };
    const entries = parseEnvText(existing).filter(([key]) => !(key in updates));
    for (const [key, value] of Object.entries(updates)) {
        if (value) entries.push([key, value]);
    }
    return entries.map(([key, value]) => `${key}=${value}`).join("\n");
}

/** 内置搜索当前的搜索服务名；没有内置搜索时为 null。 */
export function searchServiceLabel(server: MCPServer | undefined): string | null {
    if (!server) return null;
    const provider = envValue(server.environment_variables, "HTTP_SEARCH_PROVIDER");
    return isLocalSearch(provider) ? "本地搜索" : HTTP_PROVIDER_LABELS[provider] ?? provider;
}

export type DeepSeekSearchStatus =
    | { kind: "none" }
    | { kind: "connected"; server: MCPServer; enabled: boolean; fetch: boolean }
    /** 内置搜索已用 DeepSeek，但不是这个服务商的 Key。 */
    | { kind: "other_key"; server: MCPServer }
    /** 内置搜索在用别的搜索服务（含本地浏览器搜索）。 */
    | { kind: "other_provider"; server: MCPServer; label: string };

export function findSearchServer(servers: readonly MCPServer[]): MCPServer | undefined {
    const builtins = servers.filter((server) => server.is_builtin && server.command === SEARCH_COMMAND);
    return builtins.find((server) => server.is_enabled) ?? builtins[0];
}

/** 这个服务商的 Key 与内置搜索的关系，用于服务商详情页的「能力」区块。 */
export function deepSeekSearchStatus(servers: readonly MCPServer[], providerKey: string): DeepSeekSearchStatus {
    const server = findSearchServer(servers);
    if (!server) return { kind: "none" };
    const env = server.environment_variables;
    const provider = envValue(env, "HTTP_SEARCH_PROVIDER");
    if (provider !== "deepseek") {
        return { kind: "other_provider", server, label: searchServiceLabel(server) ?? provider };
    }
    const key = providerKey.trim();
    if (!key || envValue(env, "HTTP_SEARCH_API_KEY") !== key) return { kind: "other_key", server };
    return { kind: "connected", server, enabled: server.is_enabled, fetch: envValue(env, "FETCH_PROVIDER") === "deepseek" };
}

export interface ShareKeyOptions {
    providerId: number;
    providerName: string;
    apiKey: string;
    vault: { available: boolean; items: VaultItem[] };
}

/**
 * 让服务商的 Key 可以被共用：已是保险箱引用则原样返回；否则在保险箱可用时存入保险箱，
 * 并通过设置页的保存命令把服务商改为引用（这一步记录服务商一侧的去向确认）。
 * 保险箱不可用（如部分移动设备）时返回明文 Key，与手动在搜索设置里填写一致。
 */
export async function ensureSharedKey({ providerId, providerName, apiKey, vault }: ShareKeyOptions): Promise<string> {
    const value = apiKey.trim();
    if (!value) throw new Error("服务商还没有填写 API Key");
    if (parseVaultValue(value)) return value;
    if (!vault.available) return value;
    const name = `${providerName} · API Key`;
    const input: VaultCreateInput = {
        name,
        key: suggestKey("deepseek api key", vault.items.map((item) => item.key)),
        description: "DeepSeek 模型与搜索共用",
        value,
        allowedSites: [],
        syncEnabled: true,
    };
    const created = await invoke<VaultItem>("vault_create", { input });
    const ref = formatVaultRef({ id: created.id });
    await invoke("update_llm_provider_config", { llmProviderId: providerId, name: "api_key", value: ref });
    return ref;
}

interface BuiltinTemplateEnv {
    key: string;
    default_value?: string | null;
}

interface BuiltinTemplate {
    id: string;
    name: string;
    description: string;
    default_timeout?: number | null;
    required_envs: BuiltinTemplateEnv[];
}

export interface ConnectSearchOptions extends ShareKeyOptions {
    /** 服务商的接口地址；中转地址时搜索也发往同一源站。 */
    endpoint: string;
}

/** 把内置搜索的 HTTP 搜索与网页抓取设为 DeepSeek，并启用内置搜索。返回共用后的 Key 值。 */
export async function connectDeepSeekSearch(options: ConnectSearchOptions): Promise<string> {
    const key = await ensureSharedKey(options);
    const server = findSearchServer(await invoke<MCPServer[]>("get_mcp_servers"));
    if (server) {
        const request: MCPServerRequest = {
            name: server.name,
            description: server.description ?? undefined,
            transport_type: server.transport_type,
            command: server.command ?? undefined,
            environment_variables: buildDeepSeekSearchEnv(server.environment_variables, key, options.endpoint),
            url: server.url ?? undefined,
            timeout: server.timeout ?? undefined,
            is_long_running: server.is_long_running,
            is_enabled: true,
            is_builtin: server.is_builtin,
            proxy_enabled: server.proxy_enabled ?? false,
        };
        await invoke("update_mcp_server", { id: server.id, request });
        return key;
    }
    const template = (await invoke<BuiltinTemplate[]>("list_aipp_builtin_templates")).find((tpl) => tpl.id === "search");
    if (!template) throw new Error("找不到内置搜索工具模板");
    const defaults = template.required_envs
        .filter((env) => env.default_value)
        .map((env) => `${env.key}=${env.default_value}`)
        .join("\n");
    const envs = Object.fromEntries(parseEnvText(buildDeepSeekSearchEnv(defaults, key, options.endpoint)));
    await invoke<number>("add_or_update_aipp_builtin_server", {
        templateId: template.id,
        name: template.name,
        description: template.description,
        envs,
        timeout: template.default_timeout ?? null,
    });
    return key;
}

/** 错误文本里不出现明文 Key（引用本身不是秘密）。 */
export function hideKey(text: string, apiKey: string): string {
    const key = apiKey.trim();
    return key && !parseVaultValue(key) ? text.split(key).join("[已隐藏]") : text;
}
