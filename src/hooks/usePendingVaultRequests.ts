import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface PendingRequest {
    requestId: string;
    conversationId: number | null;
}

interface ResolvedPayload {
    requestId: string;
    conversationId: number | null;
}

interface Channels {
    /** 列出后端内存里仍在等待的请求（重新打开窗口时恢复卡片）。 */
    list: string;
    request: string;
    resolved: string;
}

/**
 * 保险箱等待用户处理的请求（使用确认、填写）。所有聊天界面共用：传入当前展示的会话 id（群聊传多个成员会话）。
 * 请求只存在于后端内存，处理完或调用结束时后端广播 resolved，卡片随之消失。
 */
export function usePendingVaultRequests<T extends PendingRequest>(
    conversationIds: number | number[] | null | undefined,
    channels: Channels,
) {
    const idsKey = JSON.stringify(Array.isArray(conversationIds) ? [...conversationIds].sort() : conversationIds ?? null);
    const ids = useMemo(() => {
        const parsed = JSON.parse(idsKey) as number | number[] | null;
        return parsed === null ? [] : Array.isArray(parsed) ? parsed : [parsed];
    }, [idsKey]);
    const [requests, setRequests] = useState<T[]>([]);
    const { list, request, resolved } = channels;

    useEffect(() => {
        if (ids.length === 0) {
            setRequests([]);
            return;
        }
        let disposed = false;
        const matches = (item: T) => item.conversationId !== null && ids.includes(item.conversationId);
        invoke<T[]>(list, { conversationId: null })
            .then((pending) => { if (!disposed) setRequests(pending.filter(matches)); })
            .catch((error) => console.warn(`Failed to load ${list}`, error));
        const offRequest = listen<T>(request, (event) => {
            if (!matches(event.payload)) return;
            setRequests((current) => current.some((r) => r.requestId === event.payload.requestId) ? current : [...current, event.payload]);
        });
        const offResolved = listen<ResolvedPayload>(resolved, (event) => {
            setRequests((current) => current.filter((r) => r.requestId !== event.payload.requestId));
        });
        return () => {
            disposed = true;
            void offRequest.then((off) => off());
            void offResolved.then((off) => off());
        };
    }, [ids, list, request, resolved]);

    return [requests, setRequests] as const;
}
