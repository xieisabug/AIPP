import { AppConnection, missingCredentialHint } from '@/data/ConnectedApp';
import type { MCPServer, MCPServerTool } from '@/data/MCP';

export type StatusTone = 'ok' | 'idle' | 'pending' | 'warn' | 'error';
export interface DisplayStatus { label: string; tone: StatusTone; attention: boolean }

/**
 * One display status for a server row, catalog card and detail header.
 * Authorization problems win over the enable switch: a disabled connection that needs re-login
 * must say so, otherwise the user re-enables it and gets an opaque failure.
 */
export function displayStatus(server: Pick<MCPServer, 'is_enabled' | 'is_builtin'> | undefined, connection?: AppConnection, supported = true): DisplayStatus {
    if (!supported) return { label: '此设备不可执行', tone: 'idle', attention: false };
    switch (connection?.status) {
        case 'connecting': return { label: '等待授权', tone: 'pending', attention: true };
        case 'disconnected': return connection.error
            ? { label: '断开未完成', tone: 'warn', attention: true }
            : { label: '已断开', tone: 'idle', attention: false };
    }
    // The connection synced here but its credential did not. This device cannot re-login usefully, so
    // this wins over a synced auth_required. Only nag when it can fix it (enter the passphrase);
    // without a passphrase, credentials are per-device by design.
    if (connection?.credential_on_device === false) {
        return missingCredentialHint(connection.credential_sync).actionable
            ? { label: '凭据待解锁', tone: 'warn', attention: true }
            : { label: '此设备无凭据', tone: 'idle', attention: false };
    }
    if (connection?.status === 'auth_required') return { label: '需要重新登录', tone: 'warn', attention: true };
    if (server && !server.is_enabled) return { label: '已停用', tone: 'idle', attention: false };
    switch (connection?.status) {
        case 'error': return { label: '连接失败', tone: 'error', attention: true };
        case 'available': return { label: '可用', tone: 'ok', attention: false };
        case 'unchecked': return { label: '待验证', tone: 'idle', attention: false };
    }
    return { label: '已启用', tone: 'ok', attention: false };
}

export interface ToolSummary { known: boolean; enabled: number; total: number; label: string }

/** Undiscovered tools are "unknown", never "0" — a fresh connection has simply not been listed yet. */
export function toolSummary(tools: MCPServerTool[] | undefined, server?: Pick<MCPServer, 'is_builtin'>, connection?: AppConnection): ToolSummary {
    const total = tools?.length ?? 0;
    const enabled = tools?.filter(tool => tool.is_enabled).length ?? 0;
    const known = total > 0 || !!server?.is_builtin || connection?.status === 'available';
    return { known, enabled, total, label: known ? `${enabled} / ${total} 个工具` : '工具未检测' };
}

const severity: Record<StatusTone, number> = { error: 4, warn: 3, pending: 2, ok: 1, idle: 0 };

/** Catalog card summary across every local connection of one app. */
export function aggregateStatus(statuses: DisplayStatus[]): DisplayStatus | undefined {
    if (!statuses.length) return undefined;
    if (statuses.length === 1) return statuses[0];
    const worst = statuses.reduce((a, b) => severity[b.tone] > severity[a.tone] ? b : a);
    const attention = statuses.filter(s => s.attention).length;
    return {
        label: attention ? `${statuses.length} 个连接 · ${attention} 个需处理` : `${statuses.length} 个连接`,
        tone: worst.tone, attention: attention > 0,
    };
}

export function authLabel(auth: 'none' | 'fields' | 'oauth'): string {
    return auth === 'oauth' ? '浏览器登录（OAuth）' : auth === 'fields' ? '密钥或令牌' : '无需登录';
}
