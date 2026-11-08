import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ConversationRuntimeState } from "@/data/Conversation";
import type { AvatarState } from "./avatarDefinition";
import { BUTLER_PROGRESS_EVENT, type ButlerProgressSummary } from "@/components/butler/butlerProgress";
import type { ContactPreview } from "@/components/butler/ButlerContactList";
import { BUSY_STATES, contactVisual, successful, terminal, type ContactActivity, type ContactDetail, type ContactFeedback, type ContactVisual } from "./contactActivity";

// 运行态在工具轮次之间的短暂空闲窗口，见 ConversationActivityManager::recompute_focus。
const IDLE_GAP_MS = 1200;
// 干活状态之间（读文件→搜索→输出…）每个至少停留这么久，连续的快速工具调用不会让头像来回抽搐。
const MIN_DWELL_MS = 1200;
// 只桥接"运行中"之间的空档；完成/失败/新消息这类短反馈按原时长结束。
// 待审批/等你回答等需要用户行动的提示优先于进度摘要。
const RUNNING_STATES = new Set<AvatarState>(["alerting", "waiting", ...BUSY_STATES]);

interface ToolCallInfo { tool?: string; path?: string; parameters?: string }
const parseJson = (text?: string): Record<string, unknown> | undefined => {
    try { const value: unknown = JSON.parse(text ?? ""); return value && typeof value === "object" ? value as Record<string, unknown> : undefined; }
    catch { return undefined; }
};
const filePathOf = (parameters?: string) => {
    const value = parseJson(parameters);
    const path = value?.file_path ?? value?.path;
    return typeof path === "string" ? path : undefined;
};
/** todo_write 把清单全部标成 completed，视为整体完成。 */
export const todosAllDone = (parameters?: string) => {
    const todos = parseJson(parameters)?.todos;
    return Array.isArray(todos) && todos.length > 0 && todos.every(todo => (todo as { status?: unknown })?.status === "completed");
};
const streamKinds: Record<string, ContactDetail["stream"]> = { reasoning: "reasoning", response: "response", agent_plan: "plan" };

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
        // 会话级细节：已知的工具调用、正在执行的调用、自动审核中的调用、正在流式输出的消息类型。
        const callInfo = new Map<number, Map<number, ToolCallInfo>>();
        const executing = new Map<number, Set<number>>();
        const reviewing = new Map<number, Set<number>>();
        const streams = new Map<number, ContactDetail["stream"]>();
        // todo 清单全部打勾的助手：下一次"完成"反馈改为庆祝。
        const celebrations = new Set<number>();
        const shown = new Map<number, { visual: ContactVisual; since: number }>();
        const clearDetail = (id: number) => { callInfo.delete(id); executing.delete(id); reviewing.delete(id); streams.delete(id); };
        const detailOf = (id: number): ContactDetail => {
            const calls = executing.get(id);
            const current = calls?.size ? callInfo.get(id)?.get(Math.max(...calls)) : undefined;
            return { tool: current?.tool, path: current?.path, reviewing: (reviewing.get(id)?.size ?? 0) > 0, stream: streams.get(id) };
        };
        // 空档桥接与最短停留共用一个定时器，只保留最早的唤醒时间。
        let gapTimer: ReturnType<typeof setTimeout> | undefined;
        let wakeAt = Infinity;
        const wake = (ms: number) => {
            const at = Date.now() + ms;
            if (at >= wakeAt) return;
            clearTimeout(gapTimer); wakeAt = at;
            gapTimer = setTimeout(() => { wakeAt = Infinity; emit(); }, ms + 10);
        };
        // 辅助模型生成的进度摘要：只采用与会话当前运行轮次一致的那条。
        const progress = new Map<number, ButlerProgressSummary>();
        const progressLabel = (group: ContactActivity[]) => group
            .filter(row => row.runtime.is_running || row.status === "running" || row.status === "accepted")
            .map(row => progress.get(row.conversation_id))
            .find(summary => summary && group.some(row => row.conversation_id === summary.conversation_id && row.runtime.epoch === summary.epoch))?.text;
        const emit = () => {
            if (disposed) return;
            const groups = new Map<number, ContactActivity[]>();
            rows.forEach(row => { const group = groups.get(row.assistant_id) ?? []; group.push({ ...row, detail: detailOf(row.conversation_id) }); groups.set(row.assistant_id, group); });
            feedback.forEach((_, id) => { if (!groups.has(id)) groups.set(id, []); });
            const now = Date.now();
            const pick = (id: number, group: ContactActivity[]): ContactVisual => {
                const visual = contactVisual(group, feedback.get(id), failures.has(id), now);
                if (RUNNING_STATES.has(visual.state)) {
                    const busy = BUSY_STATES.has(visual.state) ? { ...visual, label: progressLabel(group) ?? visual.label } : visual;
                    lastBusy.set(id, { visual: busy, idleSince: undefined });
                    return busy;
                }
                if (visual.state !== "idle" || visual.label) { lastBusy.delete(id); return visual; }
                // 工具轮次之间运行态会短暂回落为空闲；短时间内保持上一个活动状态，避免头像停住又动起来。
                const busy = lastBusy.get(id);
                if (busy) {
                    busy.idleSince ??= now;
                    const left = IDLE_GAP_MS - (now - busy.idleSince);
                    if (left > 0) { wake(left); return busy.visual; }
                    lastBusy.delete(id);
                }
                return visual;
            };
            const next = new Map([...groups].map(([id, group]) => {
                const visual = pick(id, group);
                const previous = shown.get(id);
                // 干活状态之间切换时，上一个状态没演够最短时长就先保持；需要用户行动的提示与收尾反馈不受限制。
                if (previous && previous.visual.state !== visual.state && BUSY_STATES.has(previous.visual.state) && BUSY_STATES.has(visual.state)) {
                    const left = MIN_DWELL_MS - (now - previous.since);
                    if (left > 0) { wake(left); return [id, previous.visual]; }
                }
                shown.set(id, { visual, since: previous?.visual.state === visual.state ? previous.since : now });
                return [id, visual];
            }));
            setVisuals(old => old.size === next.size && [...next].every(([id, value]) => old.get(id)?.state === value.state && old.get(id)?.label === value.label) ? old : next);
        };
        const pulse = (id: number, state: ContactFeedback["state"]) => {
            // A new unread preview must not replace an unhandled error.
            if (state === "notifying" && (feedback.get(id)?.until ?? 0) > Date.now()) return;
            if (state === "finished" && celebrations.delete(id)) state = "celebrate";
            if (state === "failed" || state === "blocked") celebrations.delete(id);
            feedback.set(id, { state, until: Date.now() + 3500 });
            if (state === "failed") failures.add(id);
            emit();
        };
        setController({ unread: id => pulse(id, "notifying"), acknowledge: id => {
            failures.delete(id); lastBusy.delete(id);
            if (feedback.get(id)?.state === "failed") feedback.delete(id);
            emit();
        } });
        type Event = { type: string; data: {
            state?: ConversationRuntimeState; message_type?: string; has_response?: boolean; has_reasoning?: boolean;
            call_id?: number; status?: string; tool_name?: string; parameters?: string; phase?: string;
        } };
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
                        if (next.epoch > old.epoch) clearDetail(id);
                    }
                    if (!next.is_running) { executing.delete(id); reviewing.delete(id); streams.delete(id); }
                    current.runtime = next;
                } else if (event.type === "mcp_tool_call_update" && typeof event.data.call_id === "number") {
                    // 后续更新可能不再带工具名/参数，按 call_id 合并已知信息。
                    const callId = event.data.call_id;
                    const known = callInfo.get(id) ?? new Map<number, ToolCallInfo>();
                    const old = known.get(callId) ?? {};
                    const parameters = event.data.parameters ?? old.parameters;
                    const info = { tool: event.data.tool_name ?? old.tool, parameters, path: filePathOf(parameters) ?? old.path };
                    known.set(callId, info); callInfo.set(id, known);
                    const running = executing.get(id) ?? new Set<number>();
                    if (event.data.status === "executing") running.add(callId); else if (event.data.status !== "pending") running.delete(callId);
                    executing.set(id, running);
                    if (event.data.status === "success" && info.tool === "todo_write" && todosAllDone(info.parameters)) celebrations.add(current.assistant_id);
                } else if (event.type === "tool_review_update" && typeof event.data.call_id === "number") {
                    const set = reviewing.get(id) ?? new Set<number>();
                    if (event.data.phase === "reviewing") set.add(event.data.call_id); else set.delete(event.data.call_id);
                    reviewing.set(id, set);
                } else if (["message_add", "message_update"].includes(event.type) && event.data.message_type === "error") {
                    if (!cancelled.has(id)) pulse(current.assistant_id, "failed");
                    cancelled.add(id);
                } else if (["message_add", "message_update"].includes(event.type) && event.data.message_type && streamKinds[event.data.message_type]) {
                    streams.set(id, streamKinds[event.data.message_type]);
                } else if (event.type === "conversation_cancel") {
                    cancelled.add(id); feedback.delete(current.assistant_id); celebrations.delete(current.assistant_id); clearDetail(id);
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
                        if (next.status === "interrupted") pulse(next.assistant_id, "blocked");
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
