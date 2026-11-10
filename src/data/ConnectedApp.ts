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
    connection: { transport: 'http'; url: string; auth: 'none' | 'fields' | 'oauth'; client_id?: string | null; scopes?: string[] };
    fields: AppField[];
    skill_ids?: string[];
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
}

export interface AppConnection {
    server_id: number;
    app_id: string;
    definition: AppDefinition;
    label: string;
    config: Record<string, string>;
    status: 'connecting' | 'available' | 'unchecked' | 'auth_required' | 'disconnected' | 'error';
    error: string | null;
}

export function visibleAppFields(fields: AppField[], values: Record<string, string>): AppField[] {
    return fields.filter(field => !field.visible_when ||
        (values[field.visible_when.field] ?? fields.find(f => f.key === field.visible_when?.field)?.default ?? '') === field.visible_when.equals);
}

export function connectionStatus(status?: AppConnection['status']): string {
    return ({ connecting: '等待授权', available: '可用', unchecked: '待验证', auth_required: '需要重新登录', disconnected: '已断开', error: '连接失败' } as const)[status ?? 'unchecked'];
}
