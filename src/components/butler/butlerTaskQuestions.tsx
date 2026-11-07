import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { AskUserQuestionCard, type AskUserQuestionRequest } from "@/components/InlineInteractionCards";
import type { InlineInteractionItem } from "@/components/ConversationUI";

/** 执行助手在独立任务里向用户提出、尚未回答的问题（后台 list_butler_contact_task_questions）。 */
export interface ButlerTaskQuestion {
    task_conversation_id: number;
    task_title: string;
    request_id: string | null;
    call_id: number | null;
    /** 原执行已中断（如应用重启）：回答会作为续接要求送回这项任务。 */
    stale: boolean;
    questions: AskUserQuestionRequest["questions"];
    metadata?: AskUserQuestionRequest["metadata"];
    history_key?: string | null;
    answers?: Record<string, string> | null;
    cancelled?: boolean;
    delivery_status?: "pending" | "delivered" | null;
}

const REFRESH_EVENTS = ["butler_task_question", "ask-user-question-request", "butler_task_updated", "butler_task_completed", "butler_peer_changed"];

export function useButlerTaskQuestions(contactConversationId: number | null) {
    const currentContact = useRef(contactConversationId);
    currentContact.current = contactConversationId;
    const generation = useRef(0);
    const mounted = useRef(true);
    const inFlight = useRef(new Set<string>());
    const [snapshot, setSnapshot] = useState<{ contact: number | null; items: ButlerTaskQuestion[] }>({ contact: null, items: [] });
    // Gate during render too: effects run after rendering the new contact.
    const questions = snapshot.contact === contactConversationId ? snapshot.items : [];
    const visible = useRef(questions);
    visible.current = questions;
    const refresh = useCallback(async () => {
        const contact = contactConversationId;
        if (currentContact.current !== contact) return;
        const requestGeneration = ++generation.current;
        if (contact === null) {
            setSnapshot({ contact, items: [] });
            return;
        }
        try {
            const items = await invoke<ButlerTaskQuestion[]>("list_butler_contact_task_questions", { conversationId: contact });
            if (mounted.current && currentContact.current === contact && generation.current === requestGeneration) {
                setSnapshot({ contact, items });
            }
        } catch (error) {
            console.error("[ButlerTaskQuestions] list", { contact, error });
        }
    }, [contactConversationId]);
    useEffect(() => {
        mounted.current = true;
        setSnapshot({ contact: contactConversationId, items: [] });
        void refresh();
        const listeners = contactConversationId === null ? [] : REFRESH_EVENTS.map(event => listen(event, () => void refresh()));
        return () => {
            mounted.current = false;
            ++generation.current;
            listeners.forEach(listener => void listener.then(stop => stop()).catch(console.warn));
        };
    }, [contactConversationId, refresh]);

    const respond = useCallback(async (question: ButlerTaskQuestion, answers: Record<string, string> | null, cancelled: boolean) => {
        const contact = contactConversationId;
        // Captured callbacks/cards from a previous contact must not submit, even before refresh completes.
        if (contact === null || contact !== currentContact.current || !visible.current.includes(question)
            || question.history_key || question.answers != null) return;
        const key = `${contact}:${question.task_conversation_id}:${question.request_id ?? question.call_id}`;
        if (inFlight.current.has(key)) return;
        inFlight.current.add(key);
        try {
            await invoke("respond_butler_contact_task_question", {
                conversationId: contact, taskConversationId: question.task_conversation_id,
                requestId: question.request_id, callId: question.call_id, answers, cancelled,
            });
            if (question.stale && !cancelled && currentContact.current === contact) toast.success("已回答，答案会送回这项任务继续执行");
            // Hide resolved controls before a delayed list refresh can permit duplicate submits.
            if (mounted.current && currentContact.current === contact) {
                setSnapshot(old => old.contact === contact ? { ...old, items: old.items.filter(item => item !== question) } : old);
            }
        } catch (error) {
            if (mounted.current && currentContact.current === contact) toast.error(`处理任务《${question.task_title}》的问题失败：${error}`);
        } finally {
            inFlight.current.delete(key);
            if (mounted.current && currentContact.current === contact) void refresh();
        }
    }, [contactConversationId, refresh]);
    const submit = useCallback((question: ButlerTaskQuestion, answers: Record<string, string>) => respond(question, answers, false), [respond]);
    const cancel = useCallback((question: ButlerTaskQuestion) => respond(question, null, true), [respond]);
    return { questions, submit, cancel };
}

/** 把任务提问渲染成联系人窗口底部的内联卡片，标明属于哪项任务。 */
export function taskQuestionItems(
    questions: ButlerTaskQuestion[],
    assistantName: string,
    submit: (question: ButlerTaskQuestion, answers: Record<string, string>) => void,
    cancel: (question: ButlerTaskQuestion) => void,
): InlineInteractionItem[] {
    return questions.map(question => {
        const key = question.history_key ?? question.request_id ?? `call-${question.call_id}`;
        const completed = question.answers != null;
        const request: AskUserQuestionRequest = {
            request_id: key,
            conversation_id: question.task_conversation_id,
            questions: question.questions ?? [],
            metadata: question.metadata ?? undefined,
        };
        return {
            key: `butler-task-question-${question.task_conversation_id}-${key}`,
            callId: null,
            messageId: null,
            content: <div className="space-y-1.5" data-aipp-slot="butler-task-question">
                <p className="text-xs text-muted-foreground">
                    {assistantName} 在任务《{question.task_title}》中向你提问
                    {completed ? (question.delivery_status === "pending" ? "（送达状态未确认，请核对原工作，不要重复提交）" : question.cancelled ? "（已取消回答）" : "（已回答）") : question.stale ? "（原执行已中断，回答后会送回这项任务继续）" : "，回答后这项任务会继续"}
                </p>
                <AskUserQuestionCard request={request} isOpen viewMode={completed ? "summary" : "questionnaire"}
                    completedAnswers={question.answers} readOnly={completed}
                    onSubmit={(_, answers) => submit(question, answers)} onCancel={() => cancel(question)} />
            </div>,
        };
    });
}
