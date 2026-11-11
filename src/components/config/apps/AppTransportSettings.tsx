import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { MCPServer } from '@/data/MCP';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';

export default function AppTransportSettings({ server, onSaved }: { server: MCPServer; onSaved: () => void }) {
    const initialTimeout = String(server.timeout ?? 30000);
    const [timeoutMs, setTimeoutMs] = useState(initialTimeout);
    const [proxy, setProxy] = useState(!!server.proxy_enabled);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState('');
    const dirty = timeoutMs !== initialTimeout || proxy !== !!server.proxy_enabled;
    return <form className="space-y-4" onSubmit={async e => {
        e.preventDefault(); setBusy(true); setError('');
        try {
            const value = Number(timeoutMs);
            if (!Number.isInteger(value) || value < 1000 || value > 300000) throw new Error('超时须为 1000 至 300000 毫秒');
            // Same row, same credential reference: only the transport knobs change.
            await invoke('update_mcp_server', { id: server.id, request: { ...server, timeout: value, proxy_enabled: proxy } });
            onSaved();
        } catch (e) { setError(String(e)); }
        finally { setBusy(false); }
    }}>
        <div className="grid gap-2 sm:grid-cols-[1fr_12rem] sm:items-center">
            <div>
                <Label htmlFor="app-timeout">调用超时（毫秒）</Label>
                <p className="mt-1 text-xs text-muted-foreground">单次工具调用的最长等待时间，1000–300000。</p>
            </div>
            <Input id="app-timeout" type="number" min={1000} max={300000} value={timeoutMs} onChange={e => setTimeoutMs(e.target.value)} disabled={busy} />
        </div>
        <div className="flex items-center justify-between gap-4">
            <div>
                <Label htmlFor="app-proxy">使用全局网络代理</Label>
                <p className="mt-1 text-xs text-muted-foreground">测试、发现工具和调用都走同一代理设置。</p>
            </div>
            <Switch id="app-proxy" checked={proxy} onCheckedChange={setProxy} disabled={busy} />
        </div>
        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
        <div className="flex justify-end"><Button size="sm" variant="outline" disabled={busy || !dirty}>保存网络设置</Button></div>
    </form>;
}
