import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Switch } from '@/components/ui/switch';

interface Row { id: number; name: string; enabled: boolean }

export default function AppAssistantAccess({ serverId, pluginKey }: { serverId?: number; pluginKey?: string }) {
    const [rows, setRows] = useState<Row[]>([]);
    const [error, setError] = useState('');
    const [loading, setLoading] = useState(true);
    const [busy, setBusy] = useState<number>();
    useEffect(() => {
        let alive = true;
        setLoading(true); setError('');
        void invoke<Row[]>(pluginKey ? 'get_agent_plugin_assistants' : 'get_app_assistant_access', pluginKey ? { key: pluginKey, serverId: serverId ?? null } : { serverId }).then(result => {
            if (alive) setRows(result);
        }).catch(e => { if (alive) setError(String(e)); }).finally(() => { if (alive) setLoading(false); });
        return () => { alive = false; };
    }, [serverId, pluginKey]);

    const toggle = async (id: number, enabled: boolean) => {
        setBusy(id); setError('');
        try {
            if (pluginKey) await invoke('set_agent_plugin_assistant', { key: pluginKey, serverId: serverId ?? null, assistantId: id, enabled });
            else await invoke('update_assistant_mcp_config', { assistantId: id, mcpServerId: serverId, isEnabled: enabled });
            setRows(old => old.map(row => row.id === id ? { ...row, enabled } : row));
        } catch (e) { setError(String(e)); }
        finally { setBusy(undefined); }
    };
    const target = serverId ? '此连接' : '此插件';
    const allowed = rows.filter(row => row.enabled).length;
    return <section className="space-y-4">
        <div className="flex items-start justify-between gap-4">
            <div className="space-y-1">
                <h4 className="text-sm font-medium">可以使用{target}的助手</h4>
                <p className="text-xs leading-5 text-muted-foreground">
                    连接后不会自动开放给任何助手。{pluginKey && '开启时会同时授权配套技能及读取技能所需的 load_skill。'}具体工具开关和执行审核继续沿用助手现有设置。
                </p>
            </div>
            {!loading && rows.length > 0 && <span className="shrink-0 text-xs text-muted-foreground">已允许 {allowed} / {rows.length}</span>}
        </div>
        {error && <p role="alert" className="rounded-lg border border-destructive/30 px-3 py-2 text-sm text-destructive">{error}</p>}
        {loading && <div aria-label="正在读取助手权限" className="space-y-2">{[0, 1, 2].map(i => <div key={i} className="h-12 animate-pulse rounded-lg bg-muted" />)}</div>}
        {!loading && !error && !rows.length && <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">还没有可授权的助手。</p>}
        {rows.length > 0 && <div className="divide-y rounded-lg border">
            {rows.map(row => <div key={row.id} className="flex items-center gap-3 px-4 py-3">
                <span aria-hidden className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-muted text-xs text-muted-foreground">{row.name.slice(0, 1)}</span>
                <span className="min-w-0 flex-1 truncate text-sm">{row.name}</span>
                <Switch aria-label={`允许 ${row.name} 使用此连接`} checked={row.enabled} disabled={busy !== undefined} onCheckedChange={enabled => void toggle(row.id, enabled)} />
            </div>)}
        </div>}
    </section>;
}
