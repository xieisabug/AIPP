import { useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/button";

export default function AntigravityLogin({ providerId, settings }: {
    providerId: string;
    settings: () => Record<string, string>;
}) {
    const snapshot = settings();
    const apiKeyMode = snapshot.antigravity_auth_method === "gemini-api-key";
    const hasKey = Boolean(snapshot.antigravity_api_key?.trim());
    const scope = JSON.stringify([providerId, apiKeyMode, snapshot.antigravity_runtime, snapshot.antigravity_harness,
        snapshot.agent_cli_command, snapshot.agent_node_command, snapshot.acp_working_directory, snapshot.acp_env_vars, snapshot.proxy_enabled]);
    const [status, setStatus] = useState<"checking" | "signedIn" | "signedOut" | "unknown">("checking");
    const [checkError, setCheckError] = useState("");
    const [revision, setRevision] = useState(0);
    const [operation, setOperation] = useState<"login" | "logout" | "refresh" | null>(null);
    const busy = operation !== null;
    const [url, setUrl] = useState("");
    const [message, setMessage] = useState("");
    const active = useRef(false);
    const generation = useRef(0);
    useEffect(() => {
        setUrl(""); setMessage(""); setOperation(null);
        return () => {
            generation.current++;
            if (active.current) void invoke("cancel_antigravity_login", { providerId: Number(providerId) }).catch(() => {});
            active.current = false;
        };
    }, [scope, providerId]);
    useEffect(() => {
        if (apiKeyMode) return;
        let current = true;
        setStatus("checking"); setCheckError("");
        const timer = setTimeout(async () => {
            try {
                const signedIn = await invoke<boolean>("get_antigravity_login_status", { providerId: Number(providerId), settings: settings() });
                if (current) setStatus(signedIn ? "signedIn" : "signedOut");
            } catch (error) {
                if (current) { setStatus("unknown"); setCheckError(String(error)); }
            }
        }, 250);
        return () => { current = false; clearTimeout(timer); };
    }, [scope, providerId, apiKeyMode, revision, settings]);
    const run = async (action: "login" | "logout" | "refresh") => {
        if (active.current) return;
        const request = generation.current;
        const isCurrent = () => request === generation.current && active.current;
        active.current = true; setOperation(action); setUrl(""); setMessage("");
        const onUrl = new Channel<string>();
        onUrl.onmessage = value => { if (isCurrent()) setUrl(value); };
        try {
            await invoke("login_antigravity", { providerId: Number(providerId), settings: settings(), onUrl, action });
            if (isCurrent()) {
                setStatus(action === "logout" ? "signedOut" : "signedIn"); setCheckError("");
                setMessage(action === "logout" ? "模型缓存已清除。" : "模型列表已更新。");
            }
        } catch (error) {
            if (isCurrent()) {
                setMessage(String(error));
                setRevision(value => value + 1);
            }
        } finally { if (isCurrent()) { active.current = false; setOperation(null); setUrl(""); } }
    };
    return <div className="flex flex-col gap-2">
        <p role="status" className="text-sm font-medium">{apiKeyMode
            ? (hasKey ? "已填写 Gemini API Key" : "尚未填写 Gemini API Key")
            : ({ checking: "正在检测 Google 账号登录状态…", signedIn: "Google 账号已登录",
                signedOut: "Google 账号未登录或登录已过期", unknown: "无法确认 Google 账号登录状态" }[status])}</p>
        {!apiKeyMode && checkError && <p role="alert" className="text-xs text-destructive whitespace-pre-wrap break-words">{checkError}</p>}
        <div className="flex flex-wrap gap-2">
            {!apiKeyMode && !busy && (status === "signedOut" || status === "unknown") &&
                <Button type="button" onClick={() => void run("login")}>登录 Antigravity</Button>}
            {(apiKeyMode || status === "signedIn") &&
                <Button type="button" variant="outline" disabled={busy || (apiKeyMode && !hasKey)} onClick={() => void run("refresh")}>刷新模型</Button>}
            {!apiKeyMode && status === "signedIn" && <Button type="button" variant="outline" disabled={busy} onClick={() => void run("logout")}>退出登录</Button>}
            {!apiKeyMode && !busy && status !== "checking" &&
                <Button type="button" variant="outline" onClick={() => setRevision(value => value + 1)}>重新检测</Button>}
            {operation === "login" && <Button type="button" variant="outline" onClick={() => void invoke("cancel_antigravity_login", { providerId: Number(providerId) }).catch(error => setMessage(String(error)))}>取消登录</Button>}
            {url && <Button type="button" variant="outline" onClick={() => void openUrl(url).catch(error => setMessage(String(error)))}>在浏览器中继续登录</Button>}
        </div>
        {operation === "login" && <p role="status" className="text-sm">{url ? "请在浏览器完成认证，完成后此处会显示结果。" : "正在等待 Antigravity 认证…"}</p>}
        {operation === "refresh" && <p role="status" className="text-sm">正在读取账号可用模型…</p>}
        {operation === "logout" && <p role="status" className="text-sm">正在退出登录…</p>}
        {message && <p role="status" className="text-sm whitespace-pre-wrap break-words">{message}</p>}
    </div>;
}
