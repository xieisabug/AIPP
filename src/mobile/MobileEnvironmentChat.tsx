import { lazy, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger } from "@/components/ui/sheet";
import RemoteConversations from "./RemoteConversations";

const Chat = lazy(() => import("../windows/ChatUIWindow"));
export interface PairedHost { id: string; name: string }
const selectionKey = "aipp.mobile.environment";

function restoredHost(): PairedHost | null {
    try {
        const value = JSON.parse(localStorage.getItem(selectionKey) ?? "null") as PairedHost | null;
        return value && /^[0-9a-f]{64}$/.test(value.id) && typeof value.name === "string" ? value : null;
    } catch { return null; }
}

export default function MobileEnvironmentChat() {
    const [active, setActive] = useState(restoredHost);
    const [open, setOpen] = useState(false);
    const [hosts, setHosts] = useState<PairedHost[]>([]);
    const [name, setName] = useState(() => localStorage.getItem("aipp.phone.name") ?? "我的手机");
    const [pairing, setPairing] = useState(false);
    const [error, setError] = useState("");
    useEffect(() => {
        if (!open) return;
        let live = true;
        setError("");
        invoke<{ hosts: PairedHost[] }>("bridge_environments").then(data => { if (live) setHosts(data.hosts); })
            .catch(e => { if (live) setError(String(e)); });
        return () => { live = false; };
    }, [open]);
    function choose(host: PairedHost | null) {
        setActive(host); setOpen(false);
        if (host) localStorage.setItem(selectionKey, JSON.stringify(host)); else localStorage.removeItem(selectionKey);
    }
    async function pair() {
        setError(""); setPairing(true);
        try {
            const host = await invoke<PairedHost | null>("bridge_pair_phone", { name: name.trim() });
            if (host) {
                localStorage.setItem("aipp.phone.name", name.trim());
                choose(host);
            }
        } catch (e) { setError(String(e)); } finally { setPairing(false); }
    }
    const switcher = <Sheet open={open} onOpenChange={next => { if (!pairing) setOpen(next); }}>
        <SheetTrigger asChild><Button variant="ghost" size="sm" className="min-h-11 max-w-32 gap-1"
            aria-label={`切换运行环境，当前为${active?.name ?? "该手机"}`}>
            <span className="truncate">{active?.name ?? "该手机"}</span><ChevronDown className="size-3 shrink-0" />
        </Button></SheetTrigger>
        <SheetContent side="bottom" className="safe-bottom max-h-[85dvh] overflow-y-auto p-4" aria-describedby={undefined}>
            <SheetHeader><SheetTitle>运行环境</SheetTitle></SheetHeader>
            <div className="grid gap-1">
                {[null, ...hosts].map(host => <Button key={host?.id ?? "local"} disabled={pairing}
                    variant={host?.id === active?.id ? "secondary" : "ghost"} aria-pressed={host?.id === active?.id}
                    className="min-h-11 justify-start" onClick={() => choose(host)}>{host?.name ?? "该手机"}</Button>)}
            </div>
            <div className="mt-4 space-y-2 border-t pt-3">
                <label htmlFor="phone-name" className="text-sm">配对时显示的手机名称</label>
                <Input id="phone-name" maxLength={64} value={name} disabled={pairing} onChange={e => setName(e.target.value)} />
                <Button disabled={pairing || !name.trim()} onClick={() => void pair()}>{pairing ? "等待扫码和电脑确认…" : "扫码配对电脑"}</Button>
                {error && <p role="alert" className="break-words text-sm text-destructive">{error}</p>}
            </div>
        </SheetContent>
    </Sheet>;
    return active ? <RemoteConversations key={active.id} host={active} switcher={switcher} /> :
        <Chat mobileEnvironmentSwitch={switcher} />;
}
