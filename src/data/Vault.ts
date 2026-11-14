// 凭据保险箱前端类型。后端任何接口都不会返回保险箱里的值或其片段。

/** 一个条目就是一个键对应一个值。助手在工具参数里写 `<vault:键>` 使用它。 */
export interface VaultItem {
    id: string;
    /** 占位符里的键，如 id_card、github_token。 */
    key: string;
    name: string;
    description: string;
    /** 用户选过「总是允许」的网络地点（origin），在这些地方使用不再询问。 */
    allowedSites: string[];
    syncEnabled: boolean;
    /** ready：本机可用；locked：来自其他设备、需要同步口令才能解密。 */
    status: "ready" | "locked";
    createdAt: number;
    updatedAt: number;
    lastUsedAt: number | null;
}

/** 引用某个条目的配置，以及它当前的发送目标是否已由用户确认。 */
export interface VaultConfigReference {
    kind: "mcp" | "provider";
    configId: number;
    name: string;
    /** 当前目标：网站 origin，或「本机程序 …」。 */
    target: string | null;
    approved: boolean;
    problem: string | null;
    /** 绑定当前条目和展示时配置的摘要；确认时原样传回后端校验。 */
    snapshot: string | null;
}

export interface VaultCapability {
    available: boolean;
    backend: string | null;
    reason: string | null;
    syncState: "unavailable" | "not_configured" | "needs_passphrase" | "mismatch" | "ready" | string;
    syncMessage: string;
    pendingFromOtherDevices: number;
}

export interface VaultUsageEntry {
    id: number;
    itemId: string;
    conversationId: number | null;
    assistantId: number | null;
    /** 用到这个值的工具，如「操作 · execute_bash」。 */
    tool: string;
    /** 值去了哪里，如 https://api.github.com 或「本机命令」。 */
    destination: string;
    outcome: "ok" | "denied" | "error" | string;
    reason: string | null;
    createdAt: number;
}

export interface VaultCreateInput {
    name: string;
    key: string;
    description: string;
    /** 提交后前端立即清空。 */
    value: string;
    allowedSites: string[];
    syncEnabled: boolean;
}

export interface VaultEntryLabel {
    id: string;
    key: string;
    name: string;
}

/** 这次调用会把值交给谁。site 可以「总是允许」；local 每次都要确认。 */
export interface VaultDestination {
    kind: "site" | "local";
    label: string;
    origin: string | null;
    insecure: boolean;
}

/** 助手在工具调用里写了占位符，等待用户批准这一次使用。 */
export interface VaultUseRequest {
    requestId: string;
    conversationId: number | null;
    messageId: number | null;
    assistantName: string | null;
    entries: VaultEntryLabel[];
    tool: string;
    destination: VaultDestination;
    /** 助手写的调用原文（含占位符，不含值）：命令文本或 JSON 参数。 */
    preview: string;
    previewKind: "command" | "json";
    canAlways: boolean;
    createdAt: number;
}

export type VaultUseDecision = "deny" | "once" | "always";

export interface VaultInputField {
    key: string;
    name: string;
    description: string;
}

/** 助手请用户填写它还没有的值。 */
export interface VaultInputRequest {
    requestId: string;
    conversationId: number | null;
    messageId: number | null;
    assistantName: string | null;
    purpose: string;
    fields: VaultInputField[];
    createdAt: number;
}

export const VAULT_KEY_PATTERN = /^[a-z][a-z0-9_]{1,39}$/;

export function vaultPlaceholder(key: string): string {
    return `<vault:${key}>`;
}
