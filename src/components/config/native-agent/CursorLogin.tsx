import React, { useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { CheckCircle2, ExternalLink, Loader2, LogIn } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { NativeAgentEnvironmentState } from "@/hooks/feature/useNativeAgentEnvironment";

const formatDate = (ms: number) => new Date(ms).toLocaleDateString();

/**
 * Browser sign-in for a Cursor provider (the SDK's `Cursor.auth.login()`), like T3 Code:
 * no API key to copy; the minted key is stored per provider and never shown.
 */
export const CursorLogin: React.FC<{
    providerId: string;
    state: NativeAgentEnvironmentState;
    settings: () => Record<string, string>;
}> = ({ providerId, state, settings }) => {
    const [busy, setBusy] = useState(false);
    const [url, setUrl] = useState("");
    const [message, setMessage] = useState("");
    const active = useRef(false);
    useEffect(() => () => {
        if (active.current) void invoke("cancel_native_agent_login", { providerId: Number(providerId) }).catch(() => {});
        active.current = false;
    }, [providerId]);

    const tool = state.environment?.tool;
    const account = tool?.account ?? null;
    const sdkReady = Boolean(tool?.selected?.ok);

    const login = async () => {
        if (active.current) return;
        active.current = true; setBusy(true); setUrl(""); setMessage("");
        const onUrl = new Channel<string>();
        onUrl.onmessage = (value) => { if (active.current) setUrl(value); };
        try {
            await invoke("login_cursor", { providerId: Number(providerId), settings: settings(), onUrl });
            if (active.current) { toast.success("已登录 Cursor"); await state.detect(); }
        } catch (error) {
            if (active.current) setMessage(String(error));
        } finally {
            if (active.current) { active.current = false; setBusy(false); setUrl(""); }
        }
    };
    const logout = async () => {
        try {
            await invoke("logout_cursor", { providerId: Number(providerId) });
            toast.success("已退出 Cursor 登录");
            setMessage("");
            await state.detect();
        } catch (error) {
            setMessage(String(error));
        }
    };

    if (tool?.uses_api_key) {
        return <p className="text-xs text-muted-foreground">已配置 Cursor API Key，将优先使用它；清空 API Key 与 CURSOR_API_KEY 环境变量后可改用浏览器登录。</p>;
    }
    return (
        <div className="flex flex-col gap-2">
            {account?.signed_in && !busy && (
                <p className="flex flex-wrap items-center gap-1.5 text-sm">
                    <CheckCircle2 className="h-4 w-4 text-muted-foreground" />
                    已登录{account.email ? ` ${account.email}` : ""}
                    {account.expires_at_ms ? <span className="text-xs text-muted-foreground">（有效期至 {formatDate(account.expires_at_ms)}）</span> : null}
                </p>
            )}
            <div className="flex flex-wrap gap-2">
                {!busy && (
                    <Button type="button" size="sm" variant={account?.signed_in ? "outline" : "default"} disabled={!sdkReady} onClick={() => void login()}>
                        <LogIn className="mr-1 h-4 w-4" />{account?.signed_in ? "切换账号" : "登录 Cursor"}
                    </Button>
                )}
                {!busy && account?.signed_in && (
                    <Button type="button" size="sm" variant="outline" onClick={() => void logout()}>退出登录</Button>
                )}
                {busy && url && (
                    <Button type="button" size="sm" onClick={() => void openUrl(url).catch((error) => setMessage(String(error)))}>
                        <ExternalLink className="mr-1 h-4 w-4" />在浏览器中继续登录
                    </Button>
                )}
                {busy && (
                    <Button type="button" size="sm" variant="outline"
                        onClick={() => void invoke("cancel_native_agent_login", { providerId: Number(providerId) }).catch((error) => setMessage(String(error)))}>
                        取消登录
                    </Button>
                )}
            </div>
            {busy && (
                <p role="status" className="flex items-center gap-1.5 text-xs text-muted-foreground">
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    {url ? "请在浏览器完成 Cursor 登录，完成后这里会自动更新。" : "正在启动 Cursor 登录…"}
                </p>
            )}
            {!sdkReady && !busy && <p className="text-xs text-muted-foreground">登录需要 Cursor SDK，请先在上方安装。</p>}
            {!busy && !message && (
                <p className="text-xs text-muted-foreground">通过浏览器登录 Cursor 账号，自动生成一个 90 天有效的密钥，仅保存在此提供商的独立目录。Cursor 编辑器与 CLI 的登录不会被读取。</p>
            )}
            {message && <p role="alert" className="whitespace-pre-wrap break-words text-xs text-destructive">{message}</p>}
        </div>
    );
};
