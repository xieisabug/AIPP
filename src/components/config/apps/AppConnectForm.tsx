import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Loader2 } from 'lucide-react';
import { AppConnection, AppDefinition, visibleAppFields } from '@/data/ConnectedApp';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

export default function AppConnectForm({ app, connection, onConnected }: {
    app: AppDefinition; connection?: AppConnection; onConnected: (serverId: number) => void;
}) {
    const [values, setValues] = useState<Record<string, string>>(() => Object.fromEntries(app.fields.map(f => [f.key, f.secret ? '' : connection?.config[f.key] ?? f.default ?? ''])));
    const [label, setLabel] = useState(connection?.label ?? app.name);
    const [requestId] = useState(() => crypto.randomUUID());
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState('');
    const [authorizationUrl, setAuthorizationUrl] = useState('');
    const [createdId, setCreatedId] = useState<number>();
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

    return <form className="space-y-5" onSubmit={e => { e.preventDefault(); void connect(); }}>
        <div className="space-y-2"><Label htmlFor="app-connection-label">连接名称</Label>
            <Input id="app-connection-label" value={label} onChange={e => setLabel(e.target.value)} required maxLength={80} disabled={busy || !!createdId} />
        </div>
        {visibleAppFields(app.fields, values).map(field => <div key={field.key} className="space-y-2">
            <Label htmlFor={`app-field-${field.key}`}>{field.label}{field.required ? ' *' : ''}</Label>
            {field.kind === 'select' ? <select id={`app-field-${field.key}`} className="w-full rounded-md border bg-background p-2 text-sm" value={values[field.key] ?? ''} onChange={e => update(field.key, e.target.value)} required={field.required} disabled={busy}>
                <option value="">请选择</option>{field.options?.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select> : field.kind === 'boolean' ? <input id={`app-field-${field.key}`} type="checkbox" checked={values[field.key] === 'true'} onChange={e => update(field.key, String(e.target.checked))} disabled={busy} /> :
                <Input id={`app-field-${field.key}`} type={field.secret || field.kind === 'password' ? 'password' : field.kind === 'number' ? 'number' : 'text'} value={values[field.key] ?? ''} onChange={e => update(field.key, e.target.value)} required={field.required} disabled={busy || !!createdId} autoComplete="off" />}
            {field.help && <p className="text-xs text-muted-foreground">{field.help}</p>}
        </div>)}
        {app.connection.auth === 'oauth' && <p className="text-sm text-muted-foreground">将在系统浏览器中登录并授权。完成后返回这里，AIPP 会验证连接并获取工具。</p>}
        {error && <p role="alert" className="text-sm text-destructive break-words">{error}</p>}
        {authorizationUrl && <Button type="button" variant="outline" onClick={() => void openUrl(authorizationUrl).then(() => { if (createdId) onConnected(createdId); }).catch(() => setError('系统浏览器打开失败，请检查默认浏览器设置。'))}>重新打开授权页面</Button>}
        {!createdId && <Button type="submit" disabled={busy}>{busy && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}{busy ? '正在准备连接…' : app.connection.auth === 'oauth' ? '登录并连接' : '保存并验证'}</Button>}
        {createdId && <p className="text-sm text-muted-foreground">连接已创建，实际可用状态显示在详情顶部。可在「助手授权」中选择使用它的助手。</p>}
    </form>;
}
