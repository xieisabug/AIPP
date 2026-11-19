import { useCallback, useSyncExternalStore } from "react";

const hosts = new Map<number, HTMLElement>();
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
}

export function usePermissionHost(conversationId?: number) {
    return useSyncExternalStore(subscribe, () => hosts.get(conversationId ?? 0) ?? null);
}

/** One webview owns its visible composer. A group composer represents each member session. */
export function ComposerPermissionSlot({ conversationIds, placement }: {
    conversationIds: number | readonly number[];
    placement: "top" | "bottom";
}) {
    const ids = typeof conversationIds === "number" ? [conversationIds] : conversationIds;
    const key = ids.filter(id => Number.isSafeInteger(id) && id > 0).join(",");
    const register = useCallback((node: HTMLDivElement | null) => {
        if (!node || !key) return;
        const ids = key.split(",").map(Number);
        const update = () => {
            const visible = !node.parentElement?.closest("[hidden]");
            for (const id of ids) {
                if (visible) hosts.set(id, node);
                else if (hosts.get(id) === node) hosts.delete(id);
            }
            listeners.forEach(notify => notify());
        };
        update();
        // MobileShell keeps pages mounted when navigating to settings or previews.
        const page = node.closest("[data-aipp-mobile-shell] > div");
        const observer = page ? new MutationObserver(update) : null;
        if (page) observer?.observe(page, { attributes: true, attributeFilter: ["hidden"] });
        return () => {
            observer?.disconnect();
            ids.forEach(id => { if (hosts.get(id) === node) hosts.delete(id); });
            listeners.forEach(notify => notify());
        };
    }, [key]);
    return <div ref={register} data-placement={placement} className="composer-permission-slot" />;
}
