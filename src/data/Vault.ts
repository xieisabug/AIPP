// 凭据保险箱前端类型。后端任何接口都不会返回凭据值或其片段。

export type VaultAuthKind = "bearer" | "header" | "basic" | "query";

export interface VaultItem {
    id: string;
    handle: string;
    name: string;
    description: string;
    authKind: VaultAuthKind;
    /** 注入方式的可读描述，如 "Authorization: Bearer <值>"。 */
    injection: string[];
    /** 配置引用可选的值（vault://<id>#<key>）。只有名称，没有值。 */
    fields: VaultFieldMeta[];
    allowedSites: string[];
    syncEnabled: boolean;
    /** ready：本机可用；locked：来自其他设备、需要同步口令才能解密。 */
    status: "ready" | "locked";
    createdAt: number;
    updatedAt: number;
    lastUsedAt: number | null;
}

export interface VaultFieldMeta {
    key: string;
    label: string;
}

/** 引用某条凭据的配置，以及它当前的发送目标是否已由用户确认。 */
export interface VaultConfigReference {
    kind: "mcp" | "provider";
    configId: number;
    name: string;
    /** 当前目标：网站 origin，或「本机程序 …」。 */
    target: string | null;
    approved: boolean;
    problem: string | null;
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
    method: string;
    url: string;
    status: number | null;
    outcome: "ok" | "denied" | "error" | string;
    reason: string | null;
    createdAt: number;
}

export interface VaultNamedSecret {
    name: string;
    prefix?: string;
    value: string;
}

/** 提交给后端的认证信息；提交后前端立即清空。 */
export interface VaultAuthInput {
    kind: VaultAuthKind;
    token?: string;
    username?: string;
    password?: string;
    headers?: VaultNamedSecret[];
    query?: VaultNamedSecret[];
}

export interface VaultUseRequest {
    requestId: string;
    conversationId: number | null;
    assistantName: string | null;
    credentialId: string;
    credentialName: string;
    credentialHandle: string;
    method: string;
    url: string;
    origin: string;
    insecure: boolean;
    headerNames: string[];
    hasBody: boolean;
    createdAt: number;
}

export type VaultUseDecision = "deny" | "once" | "always";

export const VAULT_AUTH_KIND_LABELS: Record<VaultAuthKind, string> = {
    bearer: "Bearer 令牌",
    header: "自定义请求头",
    basic: "用户名 + 密码",
    query: "URL 参数",
};
