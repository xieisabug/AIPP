import type { VaultAuthInput, VaultAuthKind } from "@/data/Vault";

export const HANDLE_PATTERN = /^[a-z][a-z0-9_]{1,39}$/;

export function handleError(handle: string, taken: string[]): string | null {
    if (!HANDLE_PATTERN.test(handle)) return "引用名需为 2–40 位小写字母、数字或下划线，并以字母开头";
    if (taken.includes(handle)) return "引用名已被使用";
    return null;
}

/** 由名称生成引用名：保留字母数字，其他字符变下划线；没有可用字符时用 credential。 */
export function suggestHandle(name: string, taken: string[]): string {
    let base = name
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_+|_+$/g, "")
        .replace(/^[0-9_]+/, "")
        .slice(0, 32);
    if (base.length < 2) base = "credential";
    let candidate = base;
    for (let index = 2; taken.includes(candidate); index += 1) candidate = `${base}_${index}`;
    return candidate;
}

const TOKEN_SITES: Array<[RegExp, string]> = [
    [/^(ghp_|gho_|ghu_|ghs_|github_pat_)/, "https://api.github.com"],
    [/^glpat-/, "https://gitlab.com"],
    [/^sk-ant-/, "https://api.anthropic.com"],
    [/^sk-(proj-)?[A-Za-z0-9]/, "https://api.openai.com"],
    [/^xox[bpa]-/, "https://slack.com"],
    [/^hf_/, "https://huggingface.co"],
    [/^(ntn_|secret_)/, "https://api.notion.com"],
];

/** 根据常见令牌前缀推荐允许的网站（只是建议，用户确认后才写入）。 */
export function suggestSiteForToken(token: string | undefined): string | null {
    const value = token?.trim() ?? "";
    if (!value) return null;
    return TOKEN_SITES.find(([pattern]) => pattern.test(value))?.[1] ?? null;
}

export function emptyAuth(kind: VaultAuthKind = "bearer"): VaultAuthInput {
    switch (kind) {
        case "bearer": return { kind, token: "" };
        case "basic": return { kind, username: "", password: "" };
        case "header": return { kind, headers: [{ name: "", prefix: "", value: "" }] };
        case "query": return { kind, query: [{ name: "", value: "" }] };
    }
}

/** 本地校验，避免把明显不完整的输入发给后端。返回错误文案或 null。 */
export function authError(auth: VaultAuthInput): string | null {
    switch (auth.kind) {
        case "bearer":
            return auth.token?.trim() ? null : "请填写令牌";
        case "basic":
            if (!auth.username?.trim()) return "请填写用户名";
            return auth.password?.trim() ? null : "请填写密码";
        case "header": {
            const rows = auth.headers ?? [];
            if (rows.length === 0) return "至少需要一个请求头";
            if (rows.some((row) => !row.name.trim())) return "请填写请求头名称";
            if (rows.some((row) => !/^[A-Za-z0-9!#$%&'*+.^_`|~-]+$/.test(row.name.trim()))) return "请求头名称只能包含字母、数字和 - 等符号";
            return rows.some((row) => !row.value.trim()) ? "请填写请求头的值" : null;
        }
        case "query": {
            const rows = auth.query ?? [];
            if (rows.length === 0) return "至少需要一个参数";
            if (rows.some((row) => !row.name.trim())) return "请填写参数名";
            return rows.some((row) => !row.value.trim()) ? "请填写参数值" : null;
        }
    }
}

export function formatTime(ms: number | null | undefined): string {
    if (!ms) return "—";
    const date = new Date(ms);
    const pad = (value: number) => String(value).padStart(2, "0");
    return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function formatRelative(ms: number | null | undefined, now = Date.now()): string {
    if (!ms) return "未使用";
    const minutes = Math.floor((now - ms) / 60000);
    if (minutes < 1) return "刚刚使用";
    if (minutes < 60) return `${minutes} 分钟前使用`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours} 小时前使用`;
    return `${Math.floor(hours / 24)} 天前使用`;
}
