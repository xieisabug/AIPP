import { invoke } from '@tauri-apps/api/core';
import { ask } from '@tauri-apps/plugin-dialog';
import type { AppConnection } from '@/data/ConnectedApp';
import type { MCPServer, MCPServerRequest } from '@/data/MCP';

export interface ConnectionCheck { server_id: number; current: boolean; error: string | null }

export function shareConfig(server: MCPServer, connection?: AppConnection, publicUrl?: string | null, catalogId?: string) {
    // Only the published catalog ID is portable. Custom endpoints and commands can contain
    // account IDs or opaque credentials, including in the hostname and positional arguments.
    const id = catalogId ?? connection?.app_id;
    if (id && /^[a-zA-Z0-9_-]+$/.test(id)) return { format: 'aipp.connection.v1', app: id };
    return {
        format: 'aipp.connection.v1', transport: server.transport_type,
        ...(publicUrl ? {url: publicUrl} : {}),
        timeout: server.timeout, longRunning: server.is_long_running,
    };
}

export async function importDisabled(requests: MCPServerRequest[]) {
    await invoke('import_disabled_connections', { requests });
}

export async function reviewRefresh(serverId: number) {
    const diff = await invoke<{token: string; added: string[]; changed: string[]; removed: string[]; permissions?: string[]}>('preview_connection_refresh', {serverId});
    const lines = [
        ...diff.added.map(name => '新增（停用）　' + name),
        ...diff.changed.map(name => '定义变化（保留原设置）　' + name),
        ...diff.removed.map(name => '移除工具及其授权　' + name),
        ...(diff.permissions ?? []),
    ];
    if (lines.length && !await ask(lines.join('\n'), { title: '确认工具及权限变化', okLabel: '应用变化', cancelLabel: '取消' })) return false;
    await invoke('apply_connection_refresh', {serverId, token: diff.token});
    return true;
}

export function checkFailure(detail: string) {
    if (/\b403\b|forbidden|permission denied|权限不足/i.test(detail)) return '权限不足，请检查账号权限';
    if (/\b401\b|invalid_grant|unauthorized|登录已过期/i.test(detail)) return '认证失败，请更新凭据';
    if (/服务工具已变化|已移除工具/.test(detail)) return '工具已变化，请刷新';
    if (/不支持|unsupported|method not found/i.test(detail)) return '服务不支持，请检查协议';
    if (/timeout|timed out|connect|dns|network|网络|超时/i.test(detail)) return '网络连接失败，请检查地址和网络';
    return '连接失败';
}
