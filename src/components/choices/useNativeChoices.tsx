import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { InlineInteractionItem } from "@/components/ConversationUI";
import InlineChoicesCard from "./InlineChoicesCard";
export function useNativeChoices(conversationId: number, enabled: boolean) {
    const [items, setItems] = useState<{ cid: number; items: InlineInteractionItem[] }>();
    useEffect(() => {
        if (!conversationId || !enabled) return;
        let live = true,
            revision = 0;
        async function refresh() {
            const ticket = ++revision;
            const rows = await invoke<[string, number | null][]>("list_native_choices", { conversationId });
            if (live && ticket === revision)
                setItems({
                    cid: conversationId,
                    items: rows.map(([id, messageId]) => ({
                        key: id,
                        requestId: id,
                        messageId,
                        content: <InlineChoicesCard key={id} conversationId={conversationId} requestId={id} />,
                    })),
                });
        }
        const off = listen<number>("present-choices-changed", (e) => {
            if (e.payload === conversationId) void refresh().catch(console.error);
        });
        void off.then(refresh).catch(console.error);
        return () => {
            live = false;
            void off.then((fn) => fn()).catch(console.error);
        };
    }, [conversationId, enabled]);
    return enabled && items?.cid === conversationId ? items.items : undefined;
}
