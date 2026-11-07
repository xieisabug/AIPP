import { useCallback, useEffect, useState } from "react";
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
}

const REFRESH_EVENTS = ["butler_task_question", "ask-user-question-request", "butler_task_updated", "butler_task_completed", "butler_peer_changed"];

export function useButlerTaskQuestions(contactConversationId: number | null) {
    const [questions, setQuestions] = useState<ButlerTaskQuestion[]>([]);
    const refresh = useCallback(async () => {
        if (contactConversationId === null) {
            setQuestions([]);
            return;
        }
        try {
            setQuestions(await invoke<ButlerTaskQuestion[]>("list_butler_contact_task_questions", { conversationId: contactConversationId }));
        } catch (error) {
            console.error("[ButlerTaskQuestions] list", { contactConversationId, error });
        }
    }, [contactConversationId]);
    useEffect(() => {
        void refresh();
        if (contactConversationId === null) return;
        const listeners = REFRESH_EVENTS.map(event => listen(event, () => void refresh()));
        return () => listeners.forEach(listener => void listener.then(stop => stop()).catch(console.warn));
    }, [contactConversationId, refresh]);

    const submit = useCallback(async (question: ButlerTaskQuestion, answers: Record<string, string>) => {
        try {
            if (question.stale && question.call_id !== null) {
                await invoke("answer_butler_task_question", {
                    conversationId: contactConversationId, taskConversationId: question.task_conversation_id,
                    callId: question.call_id, answers,
                });
                toast.success("已回答，答案会送回这项任务继续执行");
            } else if (question.request_id) {
                await invoke("submit_ask_user_question_response", { requestId: question.request_id, answers, cancelled: false });
            }
        } catch (error) {
            toast.error(`回答任务《${question.task_title}》的问题失败：${error}`);
        } finally {
            void refresh();
        }
    }, [contactConversationId, refresh]);

    const cancel = useCallback(async (question: ButlerTaskQuestion) => {
        try {
            if (question.stale && question.call_id !== null) {
                await invoke("stop_mcp_tool_call", { callId: question.call_id });
            } else if (question.request_id) {
                await invoke("submit_ask_user_question_response", { requestId: question.request_id, answers: null, cancelled: true });
            }
        } catch (error) {
            toast.error(`取消任务《${question.task_title}》的问题失败：${error}`);
        } finally {
            void refresh();
        }
    }, [refresh]);

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
        const key = question.request_id ?? `call-${question.call_id}`;
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
                    {question.stale ? "（原执行已中断，回答后会送回这项任务继续）" : "，回答后这项任务会继续"}
                </p>
                <AskUserQuestionCard request={request} isOpen viewMode="questionnaire"
                    onSubmit={(_, answers) => submit(question, answers)} onCancel={() => cancel(question)} />
            </div>,
        };
    });
}
