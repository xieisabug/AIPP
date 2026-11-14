import { VAULT_KEY_PATTERN } from "@/data/Vault";

/**
 * A local hint, not a save restriction or a password-strength guarantee. Vault redaction matches
 * substrings (including encoded values) from 4 UTF-8 bytes onward, so common short words can hide
 * unrelated content too. Use a wider, human-readable threshold rather than suggesting 4 bytes is
 * safe; also catch long values made by repeating a tiny pattern. Never trim the saved value.
 */
export function isSimpleVaultValue(value: string): boolean {
    const candidate = value.trim(); // Assessment only; callers save the original value unchanged.
    if (!candidate) return false;
    return Array.from(candidate).length <= 8 || /^(.{1,4})\1+$/su.test(candidate);
}

export function keyError(key: string, taken: string[]): string | null {
    if (!VAULT_KEY_PATTERN.test(key)) return "键需为 2–40 位小写字母、数字或下划线，并以字母开头";
    if (taken.includes(key)) return "这个键已被使用";
    return null;
}

/** 由名称生成键：保留字母数字，其他字符变下划线；没有可用字符（如中文名称）时用 entry。 */
export function suggestKey(name: string, taken: string[]): string {
    let base = name
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_+|_+$/g, "")
        .replace(/^[0-9_]+/, "")
        .slice(0, 32);
    if (base.length < 2) base = "entry";
    let candidate = base;
    for (let index = 2; taken.includes(candidate); index += 1) candidate = `${base}_${index}`;
    return candidate;
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
