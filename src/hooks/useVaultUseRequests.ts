import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import type { VaultUseDecision, VaultUseRequest } from "@/data/Vault";

interface ResolvedPayload {
    requestId: string;
    conversationId: number | null;
}

/**
 * 保险箱「首次访问网站」确认请求。所有聊天界面共用：传入当前展示的会话 id（群聊传多个成员会话）。
 * 请求只存在于后端内存；重新打开窗口时通过 vault_list_use_requests 恢复。
 */
export function useVaultUseRequests(conversationIds: number | number[] | null | undefined) {
    const idsKey = JSON.stringify(Array.isArray(conversationIds) ? [...conversationIds].sort() : conversationIds ?? null);
    const ids = useMemo(() => {
        const parsed = JSON.parse(idsKey) as number | number[] | null;
        return parsed === null ? [] : Array.isArray(parsed) ? parsed : [parsed];
    }, [idsKey]);
    const [requests, setRequests] = useState<VaultUseRequest[]>([]);

    useEffect(() => {
        if (ids.length === 0) {
            setRequests([]);
            return;
        }
        let disposed = false;
        const matches = (request: VaultUseRequest) => request.conversationId !== null && ids.includes(request.conversationId);
        invoke<VaultUseRequest[]>("vault_list_use_requests", { conversationId: null })
            .then((pending) => { if (!disposed) setRequests(pending.filter(matches)); })
            .catch((error) => console.warn("Failed to load vault use requests", error));
        const offRequest = listen<VaultUseRequest>("vault-use-request", (event) => {
            if (!matches(event.payload)) return;
            setRequests((current) => current.some((r) => r.requestId === event.payload.requestId) ? current : [...current, event.payload]);
        });
        const offResolved = listen<ResolvedPayload>("vault-use-resolved", (event) => {
            setRequests((current) => current.filter((r) => r.requestId !== event.payload.requestId));
        });
        return () => {
            disposed = true;
            void offRequest.then((off) => off());
            void offResolved.then((off) => off());
        };
    }, [ids]);

    const respond = useCallback(async (requestId: string, decision: VaultUseDecision) => {
        try {
            await invoke("vault_respond_use_request", { requestId, decision });
        } catch (error) {
            toast.error(`处理凭据确认失败：${String(error)}`);
            // 已经失效的请求不再显示。
            setRequests((current) => current.filter((r) => r.requestId !== requestId));
        }
    }, []);

    return { requests, respond };
}
