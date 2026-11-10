import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Switch } from '@/components/ui/switch';

export default function AppAssistantAccess({ serverId }: { serverId: number }) {
    const [rows, setRows] = useState<{ id: number; name: string; enabled: boolean }[]>([]);
    const [error, setError] = useState('');
    const [loading, setLoading] = useState(true);
    const [busy, setBusy] = useState<number>();
    useEffect(() => {
        let alive = true;
        setLoading(true); setError('');
        void invoke<{ id: number; name: string; enabled: boolean }[]>('get_app_assistant_access', { serverId }).then(result => {
            if (alive) setRows(result);
        }).catch(e => { if (alive) setError(String(e)); }).finally(() => { if (alive) setLoading(false); });
        return () => { alive = false; };
    }, [serverId]);

    const toggle = async (id: number, enabled: boolean) => {
        setBusy(id); setError('');
        try {
            await invoke('update_assistant_mcp_config', { assistantId: id, mcpServerId: serverId, isEnabled: enabled });
            setRows(old => old.map(row => row.id === id ? { ...row, enabled } : row));
        } catch (e) { setError(String(e)); }
        finally { setBusy(undefined); }
    };
    return <div className="space-y-4">
        <p className="text-sm text-muted-foreground">在这里选择可以使用此连接的助手。具体工具开关及执行审核继续使用助手现有的设置。</p>
        {loading && <p className="text-sm text-muted-foreground">正在读取助手权限…</p>}
        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
        {rows.map(row => <div key={row.id} className="flex items-center justify-between gap-4 border-b py-3"><span className="text-sm">{row.name}</span><Switch aria-label={`允许 ${row.name} 使用此连接`} checked={row.enabled} disabled={busy !== undefined} onCheckedChange={enabled => void toggle(row.id, enabled)} /></div>)}
    </div>;
}
