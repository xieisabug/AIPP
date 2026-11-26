import { Channel, invoke } from "@tauri-apps/api/core";
export { openSettings } from "@/lib/pendingSettingsRoute";
import type { Notes } from "./verifySources";
import { openUrl } from "@tauri-apps/plugin-opener";
import { isDecisionRequestMode, type ModelSelectionResponse } from "@/components/config/llmModelTypes";
import {
    isLocalAgentApiType,
    LOCAL_AGENT_DEFAULTS_READABLE,
    type LocalAgentDefaults,
} from "@/components/config/providerApiTypes";
import type { ModelForSelect } from "@/hooks/useFilteredModels";
import type { NativeAgentEnvironment } from "@/hooks/feature/useNativeAgentEnvironment";
import { recommendModels, recommendedCodes, type OnboardingPreset } from "@/utils/modelRecommend";

export type Path = "expert" | "lie_flat";
export interface Provider {
    endpoint?: string;
    description?: string;
    id: number;
    name: string;
    api_type: string;
    is_enabled: boolean;
}
export interface Role {
    providerId: number;
    code: string;
    verified: boolean;
}
export interface Config {
    notes?: Notes;
    chat: Role;
    light?: Role | null;
    review?: Role | null;
    clearLight?: boolean;
    clearReview?: boolean;
    butler?: Role | null;
    home: "chat_ui" | "butler_experiment";
    workspacePath?: string;
    workspaceDescription?: string;
    trustedWorkspacesRaw?: string;
    trustedPaths?: string[];
    displayName?: string;
    models?: { providerId: number; codes: string[] }[];
    extras: string[];
    confirmChannelChange?: boolean;
}

/** An explicit empty selection clears a role. An absent optional recommendation leaves settings alone. */
export function selectAuxiliaryRole(config: Config, key: "light" | "review", value: Role | null): Config {
    const next = { ...config, [key]: value };
    const clear = key === "light" ? "clearLight" : "clearReview";
    if (value) delete next[clear];
    else next[clear] = true;
    if (!value || roleKey(value) !== roleKey(config[key])) {
        next.notes = { ...config.notes };
        if (value) next.notes[key] = { reason: "手动选择" };
        else delete next.notes[key];
    }
    return next;
}
export interface OnboardingState {
    activeLight: string | null;
    activeReview: string | null;
    path: Path | null;
    state: string | null;
    step: string | null;
    canOpen: boolean;
    debug?: boolean;
    workspacePath: string;
    workspaceLocked: boolean;
    rerun: boolean;
    draft: {
        providers: { providerId: number; owned: boolean }[];
        config?: Config | null;
        rechargeProviderId?: number | null;
    };
}
export interface ProbeResult {
    ok: boolean;
    kind: string;
    message: string;
    latency_ms?: number;
}
export const local = isLocalAgentApiType;
export const role = (providerId: number, code: string): Role => ({ providerId, code, verified: false });
export const roleKey = (r?: Role | null) => (r ? `${r.code}%%${r.providerId}` : "");
export function roleOf(model: ModelForSelect) {
    return role(model.llm_provider_id, model.code);
}

export function detail(error: unknown) {
    if (error && typeof error === "object" && "message" in error) return String(error.message);
    return String(error);
}
export function explain(error: unknown, name = "服务商") {
    const kind = error && typeof error === "object" && "kind" in error ? String(error.kind) : "";
    const text: Record<string, string> = {
        auth: "这个 Key 不对，检查一下是否复制完整",
        insufficient_balance: "Key 没问题，但账户余额为 0，充值后才能对话",
        model_not_found: "这个服务商没有可用的推荐模型",
        rate_limited: "请求太频繁，稍等几秒再试",
        network: `连不上 ${name}，检查网络或代理设置`,
        timeout: `连不上 ${name}，检查网络或代理设置`,
    };
    return text[kind] ?? (/decrypt|解密/i.test(detail(error)) ? "密码不对" : detail(error));
}
export function deadline() {
    const end = Date.now() + 30_000;
    return async function within<T>(work: () => Promise<T>): Promise<T> {
        const remaining = end - Date.now();
        const timeout = { kind: "timeout", message: "检查已超过 30 秒" };
        if (remaining <= 0) throw timeout;
        let timer: ReturnType<typeof setTimeout> | undefined;
        try {
            return await Promise.race([
                work(),
                new Promise<never>((_, reject) => {
                    timer = setTimeout(() => reject(timeout), remaining);
                }),
            ]);
        } finally {
            clearTimeout(timer);
        }
    };
}
export type Within = ReturnType<typeof deadline>;
export async function providerSettings(id: number) {
    const rows = await invoke<{ name: string; value: string }[]>("get_llm_provider_config", { id });
    return Object.fromEntries(rows.map((row) => [row.name, row.value])) as Record<string, string>;
}
export async function providerList() {
    const [list, endpoints] = await Promise.all([
        invoke<Provider[]>("get_llm_providers"), invoke<Record<number, string>>("onboarding_provider_endpoints"),
    ]);
    return list.map((p) => ({ ...p, id: Number(p.id), endpoint: endpoints?.[p.id] ?? "" }));
}
export async function askModel() {
    const [assistant, models, providers] = await Promise.all([
        invoke<{ model: { provider_id: number; model_code: string }[] }>("get_assistant", { assistantId: 1 }),
        invoke<ModelForSelect[]>("get_filtered_models_for_select", { assistantType: 0 }),
        providerList(),
    ]);
    const binding = assistant.model?.[0];
    const match = models.find((m) => m.llm_provider_id === binding?.provider_id && m.code === binding?.model_code);
    const provider = match && providers.find((p) => p.id === match.llm_provider_id && p.is_enabled);
    if (!provider || !match || local(provider.api_type)) return null;
    if (requiresKey(provider.api_type) && !(await providerSettings(provider.id)).api_key?.trim()) return null;
    return `${provider.name} / ${match.code}`;
}
export const requiresKey = (type: string) => type !== "ollama" && type !== "magpie" && !local(type);
export async function createDraft(name: string, apiType: string, settings: Record<string, string>) {
    const id = await invoke<number>("add_llm_provider", { name, apiType, onboardingDraft: true });
    await invoke("track_onboarding_provider", { providerId: id, owned: true });
    for (const [key, value] of Object.entries(settings)) {
        await invoke("update_llm_provider_config", { llmProviderId: id, name: key, value });
    }
    return { id, name, api_type: apiType, is_enabled: false };
}
export async function createAgentDraft(type: string, name: string, cli?: string) {
    const settings: Record<string, string> = {};
    if (type === "codex_app_server") {
        settings.codex_approval_policy = "on-request";
        settings.codex_sandbox = "workspace-write";
    }
    if (type === "acp" && cli) settings.acp_cli_command = cli;
    if (LOCAL_AGENT_DEFAULTS_READABLE.has(type)) {
        const defaults = await invoke<LocalAgentDefaults>("read_local_agent_defaults", { apiType: type });
        for (const key of Object.keys(settings))
            if (defaults.settings[key]) settings[key] = defaults.settings[key];
    }
    return createDraft(name, type, settings);
}
export async function checkAgent(provider: Provider) {
    const providerId = provider.id;
    const settings = await providerSettings(providerId);
    if (
        provider.api_type === "antigravity" &&
        !(await invoke("get_antigravity_login_status", { providerId, settings }))
    ) {
        throw { kind: "login", providerId, message: "请先登录 Antigravity" };
    }
    if (provider.api_type === "cursor") {
        const env = await invoke<NativeAgentEnvironment>("detect_native_agent_environment", {
            driver: "cursor",
            providerId,
            settings,
        });
        if (!env.tool.account?.signed_in) throw { kind: "login", providerId, message: "请先登录 Cursor" };
    }
    if (provider.api_type === "codex_app_server") {
        await invoke("get_provider_quota", { providerId, force: true });
    }
}
export async function loginAgent(provider: Provider) {
    const onUrl = new Channel<string>();
    onUrl.onmessage = (url) => {
        void openUrl(url);
    };
    await invoke(provider.api_type === "cursor" ? "login_cursor" : "login_antigravity", {
        providerId: provider.id,
        settings: await providerSettings(provider.id),
        onUrl,
        action: "login",
    });
}
export async function fetchProviderModels(providerId: number) {
    const { available_models } = await invoke<ModelSelectionResponse>("preview_model_list", {
        llmProviderId: providerId,
    });
    return available_models;
}
export async function preview(provider: Provider, preset: OnboardingPreset, fetched?: ModelSelectionResponse["available_models"]) {
    const models = fetched ?? await fetchProviderModels(provider.id);
    const codes = models
        .filter((model) => {
            const mode = model.request_mode;
            return mode !== "unsupported" && mode !== "provider_mode_mismatch" && !isDecisionRequestMode(mode);
        })
        .map((model) => model.code);
    const endpoint = provider.endpoint ?? (await invoke<Record<number, string>>("onboarding_provider_endpoints"))?.[provider.id];
    return { codes, availableCodes: models.map(model => model.code), picks: recommendModels(codes, provider.api_type, preset, endpoint) };
}
export async function importProvider(code: string, password: string, draft: boolean, newName?: string) {
    const p = await invoke<Provider & { already_exists: boolean }>("import_llm_provider", {
        shareCode: code,
        password,
        newName,
        asDraft: draft,
    });
    return { ...p, id: Number(p.id) };
}
export async function populateImported(provider: Provider, preset: OnboardingPreset) {
    if (local(provider.api_type)) return;
    const { picks } = await preview(provider, preset);
    const codes = recommendedCodes(picks);
    const existing = await invoke<{ code: string }[]>("get_llm_models", { llmProviderId: String(provider.id) });
    for (const code of codes)
        if (!existing.some((m) => m.code === code))
            await invoke("add_llm_model", { llmProviderId: provider.id, code });
    if (!codes.length && !existing.length) throw new Error("未匹配到推荐模型，请在提供商详情中选择模型");
}
export function probeSession(within: Within, progress: (text: string) => void) {
    const checked = new Map<string, Promise<ProbeResult>>();
    return async (r: Role) => {
        const key = roleKey(r);
        if (!checked.has(key)) {
            progress(`正在测试 ${r.code}…`);
            checked.set(
                key,
                within(() =>
                    invoke<ProbeResult>("probe_chat_model", { providerId: r.providerId, modelCode: r.code })
                )
            );
        }
        const result = await checked.get(key)!;
        if (!result.ok) throw result;
        return { ...r, verified: true };
    };
}
