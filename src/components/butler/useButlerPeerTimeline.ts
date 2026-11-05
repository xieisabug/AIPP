import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { PeerPost } from "./butlerTimeline";

export function useButlerPeerTimeline(id: number, enabled: boolean) {
    const [snapshot, setSnapshot] = useState<{ id: number; posts: PeerPost[]; error?: string }>();
    useEffect(() => {
        if (!enabled || !id) return;
        let active = true;
        let revision = 0;
        const refresh = async () => {
            const request = ++revision;
            try {
                const posts = await invoke<PeerPost[]>("list_butler_peer_posts", { conversationId: id });
                if (active && request === revision) setSnapshot({id, posts});
            } catch (e) {
                console.error("[Butler] timeline refresh", {conversationId: id, error: e});
                if (active && request === revision) setSnapshot(last => ({id, posts: last?.id === id ? last.posts : [], error: String(e)}));
            }
        };
        const subscriptions = ["butler_peer_changed", "butler_task_created", "butler_task_updated", "butler_task_completed"]
            .map(event => listen(event, refresh));
        void Promise.all(subscriptions).then(refresh).catch(e => { if (active) setSnapshot({id,posts:[],error:String(e)}); });
        window.addEventListener("focus", refresh);
        return () => {
            active = false;
            window.removeEventListener("focus", refresh);
            subscriptions.forEach(subscription => void subscription.then(stop => stop()).catch(console.warn));
        };
    }, [id, enabled]);
    return snapshot?.id === id && enabled ? snapshot : undefined;
}
