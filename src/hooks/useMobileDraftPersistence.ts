import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useIsMobilePlatform } from "./use-platform";

const writes = new Map<string, Promise<unknown>>();

export function useMobileDraftPersistence(key: string, restore: (content: string) => void) {
    const mobile = useIsMobilePlatform();
    const revisions = useRef(new Map<string, number>());
    const latestRestore = useRef(restore);
    latestRestore.current = restore;
    useEffect(() => {
        if (!mobile) return;
        let disposed = false;
        const revision = revisions.current.get(key) ?? 0;
        void Promise.resolve(writes.get(key)).then(() => invoke<string>("get_mobile_draft", { conversationKey: key }))
            .then(content => {
                if (!disposed && (revisions.current.get(key) ?? 0) === revision) latestRestore.current(content);
            }).catch(error => {
                if (!disposed) toast.error(`恢复本机草稿失败: ${String(error)}`);
            });
        return () => { disposed = true; };
    }, [key, mobile]);
    return useCallback((content: string) => {
        if (!mobile) return;
        revisions.current.set(key, (revisions.current.get(key) ?? 0) + 1);
        // Serialize writes per conversation, including clearing after a successful send.
        const next = Promise.resolve(writes.get(key)).then(() => invoke("save_mobile_draft", { conversationKey: key, content }))
            .catch(error => toast.error(`保存本机草稿失败，当前输入仍保留: ${String(error)}`));
        writes.set(key, next);
        void next.finally(() => { if (writes.get(key) === next) writes.delete(key); });
    }, [key, mobile]);
}
