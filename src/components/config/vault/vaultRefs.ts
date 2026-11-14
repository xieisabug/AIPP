// 配置里的保险箱引用：vault://<凭据 id>#<字段>。引用本身不是秘密；真值只在后端发送时注入。

export const VAULT_REF_MARKER = "vault://";
const REF_SOURCE = "vault://([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})#([a-z][a-z0-9_]{0,39})";

export interface VaultRef {
    id: string;
    field: string;
}

/** 整个值是「可选前缀 + 一个引用」时返回各部分，例如 "Bearer vault://…#token"。 */
export function parseVaultValue(value: string | null | undefined): (VaultRef & { prefix: string }) | null {
    const match = new RegExp(`^([\\s\\S]*?)${REF_SOURCE}$`).exec(value ?? "");
    return match ? { prefix: match[1], id: match[2], field: match[3] } : null;
}

export function formatVaultRef(ref: VaultRef, prefix = ""): string {
    return `${prefix}${VAULT_REF_MARKER}${ref.id}#${ref.field}`;
}

export function containsVaultRef(value: string | null | undefined): boolean {
    return (value ?? "").includes(VAULT_REF_MARKER);
}

/** KEY=VALUE 文本里每一行的引用（环境变量），保留变量名以便展示。 */
export function envVaultRefs(text: string | null | undefined): Array<VaultRef & { name: string }> {
    const out: Array<VaultRef & { name: string }> = [];
    for (const line of (text ?? "").split(/\r?\n/)) {
        const trimmed = line.trim();
        if (!trimmed || trimmed.startsWith("#")) continue;
        const eq = trimmed.indexOf("=");
        if (eq <= 0) continue;
        const name = trimmed.slice(0, eq).trim();
        for (const match of trimmed.slice(eq + 1).matchAll(new RegExp(REF_SOURCE, "g"))) {
            out.push({ name, id: match[1], field: match[2] });
        }
    }
    return out;
}

/** 由引用名生成环境变量名：github_pat → GITHUB_PAT。 */
export function suggestEnvName(handle: string): string {
    return handle.toUpperCase().replace(/[^A-Z0-9_]/g, "_");
}
