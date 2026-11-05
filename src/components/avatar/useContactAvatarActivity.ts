import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ConversationRuntimeState } from "@/data/Conversation";
import type { ContactPreview } from "@/components/butler/ButlerContactList";
import { contactVisual, successful, terminal, type ContactActivity, type ContactFeedback, type ContactVisual } from "./contactActivity";

export function useContactAvatarActivity(main: string, previews: ContactPreview[], selected?: number) {
    const [visuals, setVisuals] = useState<Map<number, ContactVisual>>(new Map());
    const [error, setError] = useState("");
    useEffect(() => { if (error) console.warn(`[Contact avatar activity] ${error}`); }, [error]);
    const [controller, setController] = useState<{ unread: (id: number) => void; acknowledge: (id: number) => void }>();
    useEffect(() => {
        if (!main) return;
        let disposed = false, busy = false, initialized = false;
        const rows = new Map<number, ContactActivity>();
        const feedback = new Map<number, ContactFeedback>();
        const failures = new Set<number>();
        const cancelled = new Set<number>();
        const listeners = new Map<number, Promise<UnlistenFn>>();
        const emit = () => {
            if (disposed) return;
            const groups = new Map<number, ContactActivity[]>();
            rows.forEach(row => { const group = groups.get(row.assistant_id) ?? []; group.push(row); groups.set(row.assistant_id, group); });
            feedback.forEach((_, id) => { if (!groups.has(id)) groups.set(id, []); });
            const next = new Map([...groups].map(([id, group]) => [id, contactVisual(group, feedback.get(id), failures.has(id), Date.now())]));
            setVisuals(old => old.size === next.size && [...next].every(([id, value]) => old.get(id)?.state === value.state && old.get(id)?.label === value.label) ? old : next);
        };
        const pulse = (id: number, state: ContactFeedback["state"]) => {
            // A new unread preview must not replace an unhandled error.
            if (state === "notifying" && (feedback.get(id)?.until ?? 0) > Date.now()) return;
            feedback.set(id, { state, until: Date.now() + 3500 });
            if (state === "failed") failures.add(id);
            emit();
        };
        setController({ unread: id => pulse(id, "notifying"), acknowledge: id => {
            failures.delete(id);
            if (feedback.get(id)?.state === "failed") feedback.delete(id);
            emit();
        } });
        type Event = { type: string; data: { state?: ConversationRuntimeState; message_type?: string; has_response?: boolean; has_reasoning?: boolean } };
        const subscribe = (row: ContactActivity) => {
            const id = row.conversation_id;
            if (listeners.has(id) || terminal(row.status)) return;
            const promise = listen<Event>(`conversation_event_${id}`, ({ payload: event }) => {
                if (disposed) return;
                const current = rows.get(id);
                if (!current) return;
                if (event.type === "runtime_state_snapshot" && event.data.state) {
                    const next = event.data.state;
                    const old = current.runtime;
                    if (next.epoch < old.epoch || (next.epoch === old.epoch && next.revision <= old.revision)) return;
                    if (next.is_running && (!old.is_running || next.epoch > old.epoch)) {
                        cancelled.delete(id); failures.delete(current.assistant_id); feedback.delete(current.assistant_id);
                    }
                    current.runtime = next;
                } else if (["message_add", "message_update"].includes(event.type) && event.data.message_type === "error") {
                    if (!cancelled.has(id)) pulse(current.assistant_id, "failed");
                    cancelled.add(id);
                } else if (event.type === "conversation_cancel") {
                    cancelled.add(id); feedback.delete(current.assistant_id);
                } else if (event.type === "stream_complete" && !current.status && !cancelled.has(id) && (event.data.has_response || event.data.has_reasoning)) {
                    pulse(current.assistant_id, "finished");
                }
                emit();
            });
            listeners.set(id, promise);
            void promise.catch(e => { if (!disposed) setError(`联系人动画事件订阅失败 (${id}): ${String(e)}`); });
        };
        const refresh = async () => {
            if (busy || disposed || document.hidden) return;
            busy = true;
            try {
                const snapshot = await invoke<ContactActivity[]>("list_butler_contact_activity", { mainConversationId: Number(main) });
                if (disposed) return;
                const present = new Set(snapshot.map(r => r.conversation_id));
                for (const next of snapshot) {
                    const old = rows.get(next.conversation_id);
                    if (old && (old.runtime.epoch > next.runtime.epoch || (old.runtime.epoch === next.runtime.epoch && old.runtime.revision > next.runtime.revision))) next.runtime = old.runtime;
                    if (old && next.runtime.is_running && (!old.runtime.is_running || next.runtime.epoch > old.runtime.epoch)) {
                        cancelled.delete(next.conversation_id); failures.delete(next.assistant_id); feedback.delete(next.assistant_id);
                    }
                    if (initialized && old?.status !== next.status) {
                        if (successful(next.status)) pulse(next.assistant_id, "finished");
                        if (next.status === "failed") pulse(next.assistant_id, "failed");
                    }
                    rows.set(next.conversation_id, next);
                    subscribe(next);
                }
                rows.forEach((_, id) => { if (!present.has(id)) rows.delete(id); });
                listeners.forEach((promise, id) => {
                    if (!present.has(id) || terminal(rows.get(id)?.status ?? null)) {
                        void promise.then(off => off()).catch(() => {}); listeners.delete(id);
                    }
                });
                initialized = true; setError(""); emit();
            } catch (e) {
                if (!disposed) setError(`读取联系人活动失败 (main=${main}): ${String(e)}`);
            } finally { busy = false; }
        };
        const globalListeners = ["butler_task_updated", "butler_peer_changed", "conversation_list_activity"].map(name => listen(name, () => void refresh()));
        globalListeners.forEach(p => void p.catch(e => { if (!disposed) setError(`联系人活动订阅失败: ${String(e)}`); }));
        const timer = setInterval(() => { emit(); void refresh(); }, 2000);
        const onVisible = () => void refresh();
        document.addEventListener("visibilitychange", onVisible);
        void refresh();
        return () => {
            disposed = true; clearInterval(timer); document.removeEventListener("visibilitychange", onVisible);
            [...listeners.values(), ...globalListeners].forEach(p => void p.then(off => off()).catch(() => {}));
        };
    }, [main]);
    useEffect(() => { controller?.acknowledge(selected ?? 0); }, [selected, controller]);
    const [seen] = useState(() => new Map<number, number>());
    useEffect(() => {
        if (!controller) return;
        for (const preview of previews) {
            const message = preview.messageId ?? 0;
            const previous = seen.get(preview.assistantId);
            seen.set(preview.assistantId, message);
            if (previous !== undefined && message > previous && preview.assistantId !== selected && message > Number(localStorage.getItem(`butler-read-${preview.conversationId}`))) controller.unread(preview.assistantId);
        }
    }, [previews, selected, controller, seen]);
    return { visuals, error, acknowledge: controller?.acknowledge };
}
