import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { PasswordInput } from "@/components/ui/password-input";
import { getErrorMessage } from "@/utils/error";

interface Inspection {
    mode: "migration" | "new" | null;
    message: string;
    needs_choice: boolean;
    key_known: boolean;
    has_remote_key: boolean;
}
interface Props {
    id: string;
    oldUrl: string;
    serverUrl: string;
    initialToken: string;
    initialChoice?: "new" | "credentials";
    onCancel: () => void;
    onComplete: (status: any) => void;
}

export function SyncServerSwitchDialog({ id, oldUrl, serverUrl, initialToken, initialChoice, onCancel, onComplete }: Props) {
    const [choice, setChoice] = useState(initialChoice || "");
    const [token, setToken] = useState(initialToken);
    const [result, setResult] = useState<Inspection | null>(null);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState("");
    const [changePhrase, setChangePhrase] = useState(false);
    const [phrase, setPhrase] = useState("");
    const finished = useRef(false);
    const mounted = useRef(false);
    // Navigation/unmount must release the paused old connection as well.
    useEffect(() => {
        mounted.current = true;
        return () => {
            mounted.current = false;
            // StrictMode replays setup/cleanup in development; only cancel on a real unmount.
            queueMicrotask(() => {
                if (!mounted.current && !finished.current) {
                    void Promise.resolve(invoke("cancel_sync_server_switch", { id })).catch(() => {});
                }
            });
        };
    }, [id]);
    const cancel = async () => {
        if (busy) return;
        setBusy(true);
        try {
            await invoke("cancel_sync_server_switch", { id });
            finished.current = true;
            onCancel();
        } catch (e) { setError(getErrorMessage(e)); }
        finally { setBusy(false); }
    };
    const inspect = async () => {
        setBusy(true); setError("");
        try {
            const next = await invoke<Inspection>("inspect_sync_server_switch", {
                request: { id, server_url: serverUrl, token, choice },
            });
            setResult(next);
            if (next.needs_choice) setChoice("");
        } catch (e) { setError(getErrorMessage(e)); }
        finally { setBusy(false); }
    };
    const commit = async () => {
        setBusy(true); setError("");
        try {
            const status = await invoke("commit_sync_server_switch", { id, passphrase: changePhrase ? phrase : null });
            finished.current = true;
            onComplete(status);
        } catch (e) { setError(getErrorMessage(e)); }
        finally { setBusy(false); }
    };
    const confirming = Boolean(result?.mode);
    return <Dialog open onOpenChange={open => { if (!open) void cancel(); }}>
        <DialogContent className="max-h-[90vh] overflow-y-auto" onEscapeKeyDown={e => { if (busy) e.preventDefault(); }} onInteractOutside={e => e.preventDefault()}>
            <DialogHeader>
                <DialogTitle>{confirming ? "确认连接同步服务器" : initialChoice === "new" ? "连接同步服务器" : "正在更换同步服务器"}</DialogTitle>
                <DialogDescription className="break-all">
                    {oldUrl ? `原地址：${oldUrl}。` : ""}新地址：{serverUrl}
                </DialogDescription>
            </DialogHeader>
            {!confirming && <>
                {initialChoice !== "new" && <fieldset disabled={busy} className="space-y-3">
                    <legend className="mb-2 text-sm">请选择连接方式</legend>
                    {[
                        ["migration", "服务器迁移", "原服务器数据已完整搬到新地址，保留同步进度。"],
                        ["new", "新同步服务器", "与服务器已有数据合并，可能产生冲突或重复。"],
                        ["auto", "不知道，帮我检查", "比较同步位置并抽样核对最多 10 条记录。"],
                    ].map(([value, title, help]) => <label key={value} className="flex gap-2 rounded-md border p-3">
                        <input type="radio" name="sync-server-choice" value={value} checked={choice === value} onChange={() => { setChoice(value); setResult(null); }} />
                        <span><span className="text-sm font-medium">{title}</span><span className="block text-sm text-muted-foreground">{help}</span></span>
                    </label>)}
                    {choice === "credentials" && <p className="text-sm">先验证新 Token 是否仍对应原账号。</p>}
                </fieldset>}
                {initialChoice === "new" && <p className="text-sm">将本地数据与服务器同步；已有数据会合并，可能产生冲突或重复。</p>}
                <label className="space-y-2 text-sm">访问 Token
                    <PasswordInput aria-label="访问 Token" value={token} disabled={busy} onChange={e => { setToken(e.target.value); setResult(null); }} placeholder="留空沿用当前 Token" />
                </label>
            </>}
            {result && <p role="status" className="text-sm whitespace-pre-wrap">{result.message}</p>}
            {confirming && <>
                <p className="text-sm font-medium">{result?.mode === "migration" ? "保留现有同步进度，继续同步。" : "重新同步本地与服务器的数据，保留本地现有内容。"}</p>
                <fieldset disabled={busy} className="space-y-3 text-sm">
                    <legend className="mb-2">API Key 同步加密口令</legend>
                    <label className="flex gap-2"><input type="radio" name="sync-phrase" checked={!changePhrase} onChange={() => setChangePhrase(false)} />沿用当前口令（未设置则保持未设置）</label>
                    <label className="flex gap-2"><input type="radio" name="sync-phrase" checked={changePhrase} disabled={!result?.key_known} onChange={() => setChangePhrase(true)} />{result?.has_remote_key ? "输入目标服务器的口令" : "输入口令（未设置时可启用加密同步）"}</label>
                    {changePhrase && <PasswordInput aria-label="目标服务器加密口令" value={phrase} onChange={e => setPhrase(e.target.value)} autoComplete="new-password" />}
                    {!result?.key_known && <p className="text-muted-foreground">此服务器不支持口令预检查，将沿用当前设置；如不匹配，同步状态会提示输入目标口令。</p>}
                </fieldset>
            </>}
            {error && <p role="alert" className="text-sm text-destructive whitespace-pre-wrap">{error}</p>}
            <DialogFooter>
                <Button variant="outline" disabled={busy} onClick={() => void cancel()}>取消</Button>
                {result && <Button variant="outline" disabled={busy} onClick={() => { setResult(null); setError(""); }}>返回选择</Button>}
                {confirming
                    ? <Button disabled={busy || (changePhrase && !phrase)} onClick={() => void commit()}>{busy ? "正在连接…" : "保存并同步"}</Button>
                    : <Button disabled={busy || !choice} onClick={() => void inspect()}>{busy ? "检查中…" : "下一步"}</Button>}
            </DialogFooter>
        </DialogContent>
    </Dialog>;
}
