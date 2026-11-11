export interface AppField {
    key: string;
    label: string;
    kind: 'text' | 'password' | 'select' | 'boolean' | 'number';
    required?: boolean;
    secret?: boolean;
    help?: string;
    default?: string;
    options?: { label: string; value: string }[];
    visible_when?: { field: string; equals: string } | null;
    header?: string | null;
    prefix?: string;
}

export interface AppDefinition {
    id: string;
    version: string;
    name: string;
    description: string;
    publisher: string;
    category: string;
    icon?: string | null;
    homepage?: string | null;
    notice: string;
    platforms: string[];
    connection: { transport: 'http' | 'stdio'; url: string; auth: 'none' | 'fields' | 'oauth'; client_id?: string | null; scopes?: string[] };
    fields: AppField[];
    skill_ids?: string[];
    plugin_source?: { repo: string; git_ref: string; path: string } | null;
}

export interface AppCatalog {
    schema_version: number;
    version: string;
    apps: AppDefinition[];
}

export interface CatalogResponse {
    catalog: AppCatalog | null;
    fetched_at: string | null;
    warning: string | null;
    /** Entries this client cannot use yet (new auth kind/field type); skipped instead of rejecting the catalog. */
    skipped?: string[];
}

export interface AppConnection {
    server_id: number;
    app_id: string;
    definition: AppDefinition;
    label: string;
    config: Record<string, string>;
    status: 'connecting' | 'available' | 'unchecked' | 'auth_required' | 'disconnected' | 'error';
    error: string | null;
    credential_status?: string | null;
    /** Per device, never synced. `false`: the connection syncs here but its credential does not. */
    credential_on_device?: boolean | null;
    credential_sync?: CredentialSync;
}

export type CredentialSync = 'not_configured' | 'needs_passphrase' | 'mismatch' | 'ready' | 'unavailable';

/** Why this device has no copy of a synced connection's credential, and what the user can do about it. */
export function missingCredentialHint(sync?: CredentialSync): { title: string; body: string; actionable: boolean } {
    switch (sync) {
        case 'needs_passphrase': return { title: '凭据需要同步口令解锁', body: '其他设备已开启凭据加密同步。在「程序功能 → 数据」中输入同步口令后即可使用此连接。', actionable: true };
        case 'mismatch': return { title: '同步口令已在其他设备更换', body: '在「程序功能 → 数据」中输入新的同步口令后即可使用此连接。', actionable: true };
        case 'not_configured': return { title: '这台设备没有此连接的凭据', body: '未设置同步口令时，连接凭据只保存在创建连接的设备上。要在多台设备使用，请在每台设备的「程序功能 → 数据」中设置相同的同步口令。', actionable: false };
        default: return { title: '凭据尚未同步到这台设备', body: '完成一次同步后即可使用。如果原设备没有设置同步口令，凭据不会上传，请先在原设备设置。', actionable: false };
    }
}

export function visibleAppFields(fields: AppField[], values: Record<string, string>): AppField[] {
    return fields.filter(field => !field.visible_when ||
        (values[field.visible_when.field] ?? fields.find(f => f.key === field.visible_when?.field)?.default ?? '') === field.visible_when.equals);
}

export function connectionStatus(status?: AppConnection['status']): string {
    return ({ connecting: '等待授权', available: '可用', unchecked: '待验证', auth_required: '需要重新登录', disconnected: '已断开', error: '连接失败' } as const)[status ?? 'unchecked'];
}
