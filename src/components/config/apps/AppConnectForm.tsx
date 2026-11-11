import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { ExternalLink, KeyRound, Loader2, LogIn, ShieldCheck } from 'lucide-react';
import { AppConnection, AppDefinition, visibleAppFields } from '@/data/ConnectedApp';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { PasswordInput } from '@/components/ui/password-input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';

const selectClass = 'border-input dark:bg-input/30 h-9 w-full rounded-md border bg-transparent px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50';

function Intro({ app, connection }: { app: AppDefinition; connection?: AppConnection }) {
    const reconfiguring = !!connection;
    const { auth, scopes } = app.connection;
    const Icon = auth === 'oauth' ? LogIn : auth === 'fields' ? KeyRound : ShieldCheck;
    const text = auth === 'oauth'
        ? `将在系统浏览器打开 ${app.publisher || app.name} 的授权页面。完成登录后回到 AIPP，连接状态会自动更新。`
        : auth === 'fields'
            ? '填写的密钥只保存在本机加密凭据库，不写入普通配置，也不会发送给模型。实际访问权限由你提供的令牌决定。'
            : '此服务不需要登录，保存后会立即验证并获取工具列表。';
    return <div className="flex gap-3 rounded-lg border bg-muted/30 p-3 text-sm">
        <Icon className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
        <div className="space-y-1 text-muted-foreground">
            <p>{text}</p>
            {auth === 'oauth' && <p>请求的授权范围：{scopes?.length ? scopes.join('、') : '由服务商决定'}，实际权限以授权页面为准。</p>}
            {reconfiguring && <p>重新配置会先停用此连接；新凭据保存成功后才删除旧凭据。出于安全原因，已保存的密钥不会回显，需要重新填写。</p>}
            {connection?.credential_on_device === false && <p className="text-foreground">这台设备没有原凭据。重新配置后新凭据替换原凭据；未设置同步口令时，其他设备上的这个连接将无法使用。</p>}
        </div>
    </div>;
}

export default function AppConnectForm({ app, connection, onConnected, onCancel }: {
    app: AppDefinition; connection?: AppConnection; onConnected: (serverId: number) => void; onCancel?: () => void;
}) {
    const [values, setValues] = useState<Record<string, string>>(() => Object.fromEntries(app.fields.map(f => [f.key, f.secret ? '' : connection?.config[f.key] ?? f.default ?? ''])));
    const [label, setLabel] = useState(connection?.label ?? app.name);
    // One id per form instance: retries of the same submit are idempotent on the backend.
    const [requestId] = useState(() => crypto.randomUUID());
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState('');
    const [authorizationUrl, setAuthorizationUrl] = useState('');
    const [createdId, setCreatedId] = useState<number>();
    const locked = busy || createdId !== undefined;
    const update = (key: string, value: string) => setValues(old => ({ ...old, [key]: value }));

    const connect = async () => {
        setBusy(true); setError('');
        try {
            const result = await invoke<{ server_id: number; authorization_url: string | null }>('connect_catalog_app', {
                request: { app_id: app.id, request_id: requestId, label, values, server_id: connection?.server_id ?? null },
            });
            setValues(old => Object.fromEntries(Object.entries(old).map(([key, value]) => [key, app.fields.find(f => f.key === key)?.secret ? '' : value])));
            setCreatedId(result.server_id);
            if (result.authorization_url) {
                setAuthorizationUrl(result.authorization_url);
                try { await openUrl(result.authorization_url); }
                catch { setError('无法打开系统浏览器，请点击下方按钮重试。授权尚未完成。'); return; }
            }
            onConnected(result.server_id);
        } catch (e) { setError(String(e)); }
        finally { setBusy(false); }
    };

    const submitLabel = busy ? '正在准备连接…' : app.connection.auth === 'oauth' ? '登录并连接' : '保存并验证';
    return <form className="space-y-5" onSubmit={e => { e.preventDefault(); void connect(); }}>
        <Intro app={app} connection={connection} />
        <div className="space-y-2">
            <Label htmlFor="app-connection-label">连接名称</Label>
            <Input id="app-connection-label" value={label} onChange={e => setLabel(e.target.value)} required maxLength={80} disabled={locked} />
            <p className="text-xs text-muted-foreground">用于区分同一应用的多个账号或工作区。</p>
        </div>
        {visibleAppFields(app.fields, values).map(field => {
            const id = `app-field-${field.key}`;
            const common = { id, disabled: locked, required: field.required };
            return <div key={field.key} className="space-y-2">
                {field.kind === 'boolean'
                    ? <div className="flex items-center justify-between gap-4">
                        <Label htmlFor={id}>{field.label}</Label>
                        <Switch id={id} checked={values[field.key] === 'true'} onCheckedChange={checked => update(field.key, String(checked))} disabled={locked} />
                    </div>
                    : <Label htmlFor={id}>{field.label}{field.required ? ' *' : ''}</Label>}
                {field.kind === 'select'
                    ? <select {...common} className={selectClass} value={values[field.key] ?? ''} onChange={e => update(field.key, e.target.value)}>
                        <option value="">请选择</option>
                        {field.options?.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
                    </select>
                    : field.kind === 'boolean' ? null
                        : field.secret || field.kind === 'password'
                            ? <PasswordInput {...common} value={values[field.key] ?? ''} onChange={e => update(field.key, e.target.value)} autoComplete="off" />
                            : <Input {...common} type={field.kind === 'number' ? 'number' : 'text'} value={values[field.key] ?? ''} onChange={e => update(field.key, e.target.value)} autoComplete="off" />}
                {field.help && <p className="text-xs text-muted-foreground">{field.help}</p>}
            </div>;
        })}
        {error && <p role="alert" className="rounded-lg border border-destructive/30 px-3 py-2 text-sm break-words text-destructive">{error}</p>}
        <div className="flex flex-wrap items-center justify-end gap-2 border-t pt-4">
            {authorizationUrl && <Button type="button" variant="outline" className="mr-auto" onClick={() => void openUrl(authorizationUrl).then(() => { setError(''); if (createdId) onConnected(createdId); }).catch(() => setError('系统浏览器打开失败，请检查默认浏览器设置。'))}>
                <ExternalLink className="h-4 w-4" />重新打开授权页面
            </Button>}
            {createdId !== undefined && !authorizationUrl && <p role="status" className="mr-auto text-sm text-muted-foreground">连接已创建，状态会在详情中更新。</p>}
            {onCancel && <Button type="button" variant="ghost" disabled={busy} onClick={onCancel}>取消</Button>}
            {createdId === undefined && <Button type="submit" disabled={busy}>{busy && <Loader2 className="h-4 w-4 animate-spin" />}{submitLabel}</Button>}
        </div>
    </form>;
}
