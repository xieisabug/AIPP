import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ConversationRuntimeState } from "@/data/Conversation";
import type { AvatarState } from "./avatarDefinition";
import { BUTLER_PROGRESS_EVENT, type ButlerProgressSummary } from "@/components/butler/butlerProgress";
import type { ContactPreview } from "@/components/butler/ButlerContactList";
import { contactVisual, successful, terminal, type ContactActivity, type ContactFeedback, type ContactVisual } from "./contactActivity";

// 运行态在工具轮次之间的短暂空闲窗口，见 ConversationActivityManager::recompute_focus。
const IDLE_GAP_MS = 1200;
// 只桥接"运行中"之间的空档；完成/失败/新消息这类短反馈按原时长结束。
// 待审批/等你回答等需要用户行动的提示优先于进度摘要。
const SUMMARY_STATES = new Set<AvatarState>(["working", "streaming", "thinking"]);
const RUNNING_STATES = new Set<AvatarState>(["alerting", "waiting", "working", "streaming", "thinking"]);

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
        const lastBusy = new Map<number, { visual: ContactVisual; idleSince?: number }>();
        let gapTimer: ReturnType<typeof setTimeout> | undefined;
        // 辅助模型生成的进度摘要：只采用与会话当前运行轮次一致的那条。
        const progress = new Map<number, ButlerProgressSummary>();
        const progressLabel = (group: ContactActivity[]) => group
            .filter(row => row.runtime.is_running || row.status === "running" || row.status === "accepted")
            .map(row => progress.get(row.conversation_id))
            .find(summary => summary && group.some(row => row.conversation_id === summary.conversation_id && row.runtime.epoch === summary.epoch))?.text;
        const emit = () => {
            if (disposed) return;
            const groups = new Map<number, ContactActivity[]>();
            rows.forEach(row => { const group = groups.get(row.assistant_id) ?? []; group.push(row); groups.set(row.assistant_id, group); });
            feedback.forEach((_, id) => { if (!groups.has(id)) groups.set(id, []); });
            const now = Date.now();
            const next = new Map([...groups].map(([id, group]) => {
                const visual = contactVisual(group, feedback.get(id), failures.has(id), now);
                if (RUNNING_STATES.has(visual.state)) {
                    const busy = SUMMARY_STATES.has(visual.state) ? { ...visual, label: progressLabel(group) ?? visual.label } : visual;
                    lastBusy.set(id, { visual: busy, idleSince: undefined });
                    return [id, busy];
                }
                if (visual.state !== "idle" || visual.label) { lastBusy.delete(id); return [id, visual]; }
                // 工具轮次之间运行态会短暂回落为空闲；短时间内保持上一个活动状态，避免头像停住又动起来。
                const busy = lastBusy.get(id);
                if (busy) {
                    busy.idleSince ??= now;
                    const left = IDLE_GAP_MS - (now - busy.idleSince);
                    if (left > 0) {
                        clearTimeout(gapTimer); gapTimer = setTimeout(emit, left + 10);
                        return [id, busy.visual];
                    }
                    lastBusy.delete(id);
                }
                return [id, visual];
            }));
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
            failures.delete(id); lastBusy.delete(id);
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
        const progressListener = listen<ButlerProgressSummary>(BUTLER_PROGRESS_EVENT, ({ payload }) => {
            if (disposed) return;
            progress.set(payload.conversation_id, payload);
            emit();
        });
        void progressListener.catch(e => { if (!disposed) setError(`进度摘要订阅失败: ${String(e)}`); });
        const globalListeners = ["butler_task_updated", "butler_peer_changed", "conversation_list_activity"].map(name => listen(name, () => void refresh()));
        globalListeners.forEach(p => void p.catch(e => { if (!disposed) setError(`联系人活动订阅失败: ${String(e)}`); }));
        const timer = setInterval(() => { emit(); void refresh(); }, 2000);
        const onVisible = () => void refresh();
        document.addEventListener("visibilitychange", onVisible);
        void refresh();
        return () => {
            disposed = true; clearInterval(timer); clearTimeout(gapTimer); document.removeEventListener("visibilitychange", onVisible);
            [...listeners.values(), ...globalListeners, progressListener].forEach(p => void p.then(off => off()).catch(() => {}));
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
