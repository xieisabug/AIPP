import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { MCPServer } from '@/data/MCP';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';

export default function AppTransportSettings({ server, onSaved }: { server: MCPServer; onSaved: () => void }) {
    const [timeout, setTimeout] = useState(String(server.timeout ?? 30000));
    const [proxy, setProxy] = useState(server.proxy_enabled);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState('');
    return <form className="space-y-3 border-t pt-5" onSubmit={async e => {
        e.preventDefault(); setBusy(true); setError('');
        try {
            const value = Number(timeout);
            if (!Number.isInteger(value) || value < 1000 || value > 300000) throw new Error('超时须为 1000 至 300000 毫秒');
            await invoke('update_mcp_server', { id: server.id, request: { ...server, timeout: value, proxy_enabled: proxy } });
            onSaved();
        } catch (e) { setError(String(e)); }
        finally { setBusy(false); }
    }}>
        <h4 className="text-sm font-medium">网络设置</h4>
        <Label htmlFor="app-timeout">调用超时（毫秒）</Label>
        <Input id="app-timeout" type="number" min={1000} max={300000} value={timeout} onChange={e => setTimeout(e.target.value)} disabled={busy} />
        <div className="flex items-center justify-between gap-4"><Label htmlFor="app-proxy">使用全局网络代理</Label><Switch id="app-proxy" checked={proxy} onCheckedChange={setProxy} disabled={busy} /></div>
        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
        <Button variant="outline" disabled={busy}>保存网络设置</Button>
    </form>;
}
