import { useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/button";

export default function AntigravityLogin({ providerId, settings }: {
    providerId: string;
    settings: () => Record<string, string>;
}) {
    const [busy, setBusy] = useState(false);
    const [url, setUrl] = useState("");
    const [message, setMessage] = useState("");
    const active = useRef(false);
    useEffect(() => () => {
        if (active.current) void invoke("cancel_antigravity_login", { providerId: Number(providerId) }).catch(() => {});
        active.current = false;
    }, [providerId]);
    const login = async () => {
        if (active.current) return;
        active.current = true; setBusy(true); setUrl(""); setMessage("");
        const onUrl = new Channel<string>();
        onUrl.onmessage = value => { if (active.current) setUrl(value); };
        try {
            await invoke("login_antigravity", { providerId: Number(providerId), settings: settings(), onUrl });
            if (active.current) setMessage("Antigravity 已确认认证成功，可返回助手配置刷新模型。");
        } catch (error) { if (active.current) setMessage(String(error)); }
        finally { if (active.current) { active.current = false; setBusy(false); setUrl(""); } }
    };
    return <div className="flex flex-col gap-2">
        <div className="flex flex-wrap gap-2">
            <Button type="button" disabled={busy} onClick={() => void login()}>登录 Antigravity</Button>
            {busy && <Button type="button" variant="outline" onClick={() => void invoke("cancel_antigravity_login", { providerId: Number(providerId) }).catch(error => setMessage(String(error)))}>取消登录</Button>}
            {url && <Button type="button" variant="outline" onClick={() => void openUrl(url).catch(error => setMessage(String(error)))}>在浏览器中继续登录</Button>}
        </div>
        <p className="text-xs text-muted-foreground">使用当前填写的配置。账号保存在此提供商的独立目录，与 Antigravity IDE、其他提供商分开。登录等待最多 5 分钟。</p>
        {busy && <p role="status" className="text-sm">{url ? "请在浏览器完成认证，完成后此处会显示结果。" : "正在等待 Antigravity 认证…"}</p>}
        {message && <p role="status" className="text-sm whitespace-pre-wrap break-words">{message}</p>}
    </div>;
}
