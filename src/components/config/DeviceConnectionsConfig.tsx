import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { AssistantListItem } from "@/data/Assistant";
import SettingsPageShell from "./pages/SettingsPageShell";

interface Grant { assistants: number[]; butler: boolean; chat: boolean }
interface Phone { id: string; name: string; access: Grant }
interface Status {
    addresses: string[];
    devices: Phone[];
    listening: { name: string; address: string } | null;
    pending: { id: string; name: string } | null;
}

function PairDecision({ phone, decide, busy }: {
    phone: { id: string; name: string }; decide: (grant: Grant | null) => void; busy: boolean;
}) {
    const [assistants, setAssistants] = useState<AssistantListItem[]>([]);
    const [grant, setGrant] = useState<Grant>({ assistants: [], butler: false, chat: false });
    const [error, setError] = useState("");
    useEffect(() => {
        let live = true;
        invoke<AssistantListItem[]>("get_assistants").then(list => { if (live) setAssistants(list); })
            .catch(e => { if (live) setError(String(e)); });
        return () => { live = false; };
    }, []);
    return <section className="space-y-3 rounded-lg border p-4">
        <h3 className="font-medium">{phone.name} 请求配对</h3>
        <p className="text-sm text-muted-foreground">选择这部手机可以访问的助手和管家会话。</p>
        {assistants.map(a => <label className="flex items-center gap-2" key={a.id}>
            <Checkbox checked={grant.assistants.includes(a.id)}
                onCheckedChange={v => setGrant({ ...grant, assistants: v === true ? [...grant.assistants, a.id] : grant.assistants.filter(id => id !== a.id) })} />
            {a.name}
        </label>)}
        <label className="flex items-center gap-2"><Checkbox checked={grant.butler}
            onCheckedChange={v => setGrant({ ...grant, butler: v === true })} />总管家和所选助手的管家私聊</label>
        <label className="flex items-center gap-2"><Checkbox checked={grant.chat}
            onCheckedChange={v => setGrant({ ...grant, chat: v === true })} />允许发送消息和停止本手机发起的回复</label>
        {grant.chat && <p className="text-sm text-muted-foreground">使用电脑上相应助手或管家的模型、工作区和工具权限。需要批准的操作仍在电脑上确认。</p>}
        {error && <p role="alert">{error}</p>}
        <div className="flex gap-2"><Button disabled={busy || (!grant.butler && !grant.assistants.length)} onClick={() => decide(grant)}>允许配对</Button>
            <Button variant="outline" disabled={busy} onClick={() => decide(null)}>拒绝</Button></div>
    </section>;
}

export default function DeviceConnectionsConfig() {
    const [status, setStatus] = useState<Status>();
    const [name, setName] = useState(() => localStorage.getItem("host-bridge-name") ?? "我的电脑");
    const [address, setAddress] = useState("");
    const [qr, setQr] = useState("");
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState("");
    const refresh = useCallback(async () => {
        const data = await invoke<Status>("bridge_host_status");
        setStatus(data);
        if (data.pending || !data.listening) setQr("");
    }, []);
    useEffect(() => { void refresh().catch(e => setError(String(e))); }, [refresh]);
    const listening = !!status?.listening;
    useEffect(() => {
        if (!listening) return;
        const timer = window.setInterval(() => {
            if (!document.hidden) void refresh().catch(e => setError(String(e)));
        }, 2000);
        return () => window.clearInterval(timer);
    }, [listening, refresh]);
    useEffect(() => {
        if (!qr) return;
        const timer = window.setTimeout(() => setQr(""), 300_000);
        return () => window.clearTimeout(timer);
    }, [qr]);
    async function act(action: () => Promise<void>) {
        setBusy(true); setError("");
        try { await action(); await refresh(); } catch (e) { setError(String(e)); } finally { setBusy(false); }
    }
    const chosen = status?.addresses.includes(address) ? address : status?.addresses[0] ?? "";
    return <SettingsPageShell title="设备互联" description="允许已配对的手机通过局域网连接这台电脑。">
        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
        {!status ? <Button variant="outline" onClick={() => void act(refresh)}>重新读取连接状态</Button> :
            <div className="max-w-xl space-y-5">
                {status.listening ? <div className="space-y-3">
                    <p>{status.listening.name} · 正在监听 {status.listening.address}</p>
                    <div className="flex gap-2">
                        <Button disabled={busy || !!status.pending} onClick={() => void act(async () => setQr(await invoke<string>("bridge_invite")))}>配对手机</Button>
                        <Button variant="outline" disabled={busy} onClick={() => void act(() => invoke("bridge_stop"))}>关闭连接</Button>
                    </div>
                </div> : <div className="space-y-3">
                    <Label htmlFor="host-bridge-name">电脑名称</Label><Input id="host-bridge-name" maxLength={64} value={name} onChange={e => setName(e.target.value)} />
                    <Label htmlFor="host-bridge-address">局域网地址</Label>
                    <Select value={chosen} onValueChange={setAddress}>
                        <SelectTrigger id="host-bridge-address" className="bg-background"><SelectValue /></SelectTrigger>
                        <SelectContent>{status.addresses.map(ip => <SelectItem key={ip} value={ip}>{ip}</SelectItem>)}</SelectContent>
                    </Select>
                    {!chosen && <p className="text-sm">请连接局域网后重新检测网卡。</p>}
                    <Button variant="outline" disabled={busy} onClick={() => void act(refresh)}>重新检测</Button>{" "}
                    <Button disabled={busy || !chosen || !name.trim()} onClick={() => void act(async () => {
                        await invoke("bridge_start", { name: name.trim(), address: chosen }); localStorage.setItem("host-bridge-name", name.trim());
                    })}>允许手机连接此电脑</Button>
                </div>}
                {qr && <figure><img className="size-64" alt="一次性设备配对二维码" src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(qr)}`} />
                    <figcaption className="text-sm text-muted-foreground">用手机 AIPP 扫描，五分钟内有效。</figcaption></figure>}
                {status.pending && <PairDecision key={status.pending.id} phone={status.pending} busy={busy}
                    decide={grant => void act(() => invoke("bridge_decide", { id: status.pending!.id, grant }))} />}
                {!!status.devices.length && <section className="space-y-2"><h3 className="font-medium">已配对手机</h3>
                    {status.devices.map(device => <div key={device.id} className="flex items-center justify-between gap-4 border-b py-2">
                        <span>{device.name}</span><label className="flex items-center gap-2"><Checkbox disabled={busy}
                            checked={!!device.access.chat} onCheckedChange={v => { const allowed = v === true;
                                void act(() => invoke("bridge_set_chat_permission", { id:device.id,allowed }));
                            }} />允许对话</label><Button variant="outline" disabled={busy} onClick={() => void act(() => invoke("bridge_revoke", { id: device.id }))}>撤销配对</Button>
                    </div>)}</section>}
            </div>}
    </SettingsPageShell>;
}
