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
        setRequests([]);
        if (ids.length === 0) {
            return;
        }
        let disposed = false;
        const unlisten: (() => void)[] = [];
        const resolvedIds = new Set<string>();
        const matches = (item: T) => item.conversationId !== null && ids.includes(item.conversationId);
        const register = async (subscription: Promise<() => void>) => {
            const off = await subscription;
            if (disposed) { off(); return false; }
            unlisten.push(off);
            return true;
        };
        const load = async () => {
            // Observe resolutions first so no captured request can miss its closing event.
            if (!await register(listen<ResolvedPayload>(resolved, (event) => {
                if (disposed) return;
                resolvedIds.add(event.payload.requestId);
                setRequests((current) => current.filter((r) => r.requestId !== event.payload.requestId));
            }))) return;
            if (!await register(listen<T>(request, (event) => {
                if (disposed || !matches(event.payload) || resolvedIds.has(event.payload.requestId)) return;
                setRequests((current) => current.some((r) => r.requestId === event.payload.requestId) ? current : [...current, event.payload]);
            }))) return;
            // Register both listeners before taking the snapshot. Events while it is in flight
            // win over stale snapshot rows, including resolutions that must not be resurrected.
            const pending = await invoke<T[]>(list, { conversationId: null });
            if (disposed) return;
            setRequests((current) => [...new Map(
                [...pending.filter(matches), ...current]
                    .filter((item) => !resolvedIds.has(item.requestId))
                    .map((item) => [item.requestId, item]),
            ).values()]);
        };
        void load().catch((error) => { if (!disposed) console.warn(`Failed to load ${list}`, error); });
        return () => {
            disposed = true;
            unlisten.forEach((off) => off());
        };
    }, [ids, list, request, resolved]);

    return [requests, setRequests] as const;
}
