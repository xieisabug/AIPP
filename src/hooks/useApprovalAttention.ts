import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** 后端 `approval_attention::APPROVAL_ATTENTION_EVENT`：某个会话的待审批状态可能变化。 */
export const APPROVAL_ATTENTION_EVENT = "approval_attention_changed";

function sameIds(a: Set<number>, b: Set<number>) {
    return a.size === b.size && [...a].every((id) => b.has(id));
}

const validIds = (ids: readonly number[]) => ids.filter((id) => Number.isFinite(id) && id > 0);

/**
 * 等待用户审批的会话：操作/Agent 权限请求，以及没有自动执行、停下来等确认的工具调用
 * （未开启自动审核或自动审核未通过）。会话列表的待审批黄点从这里读。
 */
export function usePendingApprovalConversations(): Set<number> {
    const [ids, setIds] = useState<Set<number>>(() => new Set());
    useEffect(() => {
        let disposed = false;
        let request = 0;
        const refresh = () => {
            const current = ++request;
            invoke<number[]>("list_pending_approval_conversations")
                .then((list) => {
                    if (disposed || current !== request) return;
                    const next = new Set(list ?? []);
                    setIds((prev) => (sameIds(prev, next) ? prev : next));
                })
                .catch((error) => console.warn("[approval attention] 读取待审批会话失败", error));
        };
        const unlisteners = [APPROVAL_ATTENTION_EVENT, "conversation_deleted"].map((name) => listen(name, refresh));
        unlisteners.forEach((promise) => void promise.catch((error) => console.warn("[approval attention] 订阅失败", error)));
        window.addEventListener("focus", refresh);
        refresh();
        return () => {
            disposed = true;
            window.removeEventListener("focus", refresh);
            unlisteners.forEach((promise) => void promise.then((off) => off()).catch(() => {}));
        };
    }, []);
    return ids;
}

/**
 * 告诉后端本窗口正在展示哪些会话的审批：聚焦窗口里已经看得到的审批不再弹系统通知。
 * - `conversationIds`：消息流正在显示的会话（工具调用确认卡片在这里）；
 * - `permissionConversationIds`：本窗口会弹出审批弹窗的会话，缺省与 `conversationIds` 相同。
 */
export function useApprovalSurface(conversationIds: readonly number[], permissionConversationIds?: readonly number[]) {
    // 调用方每次渲染都可能传新数组，按内容比较，避免重复上报。
    const key = JSON.stringify([validIds(conversationIds), permissionConversationIds ? validIds(permissionConversationIds) : null]);
    useEffect(() => {
        const [conversations, permissions] = JSON.parse(key) as [number[], number[] | null];
        invoke("set_approval_surface", { conversationIds: conversations, permissionConversationIds: permissions })
            .catch((error) => console.warn("[approval attention] 上报审批展示范围失败", error));
    }, [key]);
    useEffect(() => () => {
        invoke("set_approval_surface", { conversationIds: [], permissionConversationIds: null }).catch(() => {});
    }, []);
}
