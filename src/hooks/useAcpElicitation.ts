import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
    AskUserQuestionItem,
    AskUserQuestionRequest,
} from "@/components/InlineInteractionCards";
import { getErrorMessage } from "@/utils/error";

interface UseAcpElicitationOptions {
    /** 当前会话 ID，用于过滤只处理当前会话的 elicitation 请求 */
    conversationId?: number;
    /** 当前窗口可处理的一组会话 ID */
    conversationIds?: number[];
}

interface AcpElicitationResolvedEvent {
    request_id: string;
    conversation_id?: number;
}

export type AcpElicitationAction = "accept" | "decline" | "cancel";

export interface AcpElicitationEnumOption {
    value: string;
    title: string;
    description?: string;
}

/** ACP ElicitationPropertySchema 的 JSON 形态（serde tag = "type"） */
export interface AcpElicitationFieldSchema {
    type: "string" | "number" | "integer" | "boolean" | "array" | string;
    title?: string;
    description?: string;
    // string
    minLength?: number;
    maxLength?: number;
    pattern?: string;
    format?: string;
    default?: unknown;
    enum?: string[];
    oneOf?: AcpElicitationEnumOption[];
    // number / integer
    minimum?: number;
    maximum?: number;
    // array（多选）
    minItems?: number;
    maxItems?: number;
    items?: {
        type?: string;
        enum?: string[];
        anyOf?: AcpElicitationEnumOption[];
    };
}

/** ACP ElicitationSchema 的 JSON 形态 */
export interface AcpElicitationSchema {
    type?: string;
    title?: string;
    description?: string;
    properties?: Record<string, AcpElicitationFieldSchema>;
    required?: string[];
}

export interface AcpElicitationRequest {
    request_id: string;
    conversation_id?: number;
    agent_kind: string;
    message: string;
    schema: AcpElicitationSchema;
}

function isStaleAcpElicitationError(message: string) {
    return (
        message.includes("ACP elicitation request not found or already resolved") ||
        message.includes("ACP elicitation receiver dropped before resolution")
    );
}

function shouldHandleConversationRequest(
    requestConversationId: number | undefined,
    conversationId?: number,
    conversationIds?: number[]
) {
    if (requestConversationId === undefined) {
        return true;
    }
    if (conversationIds && conversationIds.length > 0) {
        return conversationIds.includes(requestConversationId);
    }
    if (conversationId !== undefined) {
        return requestConversationId === conversationId;
    }
    return true;
}

/**
 * 把 ACP elicitation schema 转成 ask_user_question 卡片的问题列表，
 * 每张问题卡携带 answerKey/valueType/optionValues 元数据，
 * 提交时再按元数据把文本答案还原成类型化值。
 */
function toAskUserQuestionItems(request: AcpElicitationRequest): AskUserQuestionItem[] {
    const requiredSet = new Set(request.schema.required ?? []);
    return Object.entries(request.schema.properties ?? {}).map(([name, field]) => {
        const item: AskUserQuestionItem = {
            question: field.description || field.title || name,
            header: field.title || name,
            options: [],
            multiSelect: false,
            answerKey: name,
            required: requiredSet.has(name),
        };

        if (field.type === "string" && (field.oneOf?.length || field.enum?.length)) {
            const options: AcpElicitationEnumOption[] =
                field.oneOf ??
                (field.enum ?? []).map((value) => ({ value, title: value }));
            item.options = options.map((option) => ({
                label: option.title,
                description: option.description ?? option.value,
            }));
            item.optionValues = Object.fromEntries(
                options.map((option) => [option.title, option.value])
            );
            item.valueType = "string";
            return item;
        }

        if (field.type === "array") {
            const options: AcpElicitationEnumOption[] =
                field.items?.anyOf ??
                (field.items?.enum ?? []).map((value) => ({ value, title: value }));
            item.multiSelect = true;
            item.options = options.map((option) => ({
                label: option.title,
                description: option.description ?? option.value,
            }));
            item.optionValues = Object.fromEntries(
                options.map((option) => [option.title, option.value])
            );
            item.valueType = "stringArray";
            return item;
        }

        if (field.type === "boolean") {
            item.options = [
                { label: "是", description: "" },
                { label: "否", description: "" },
            ];
            item.optionValues = { 是: "true", 否: "false" };
            item.valueType = "boolean";
            return item;
        }

        if (field.type === "number" || field.type === "integer") {
            // 数字走自由文本输入，提交时再做 parse 校验
            item.valueType = field.type;
            return item;
        }

        // 纯 string 及未知类型按自由文本处理
        item.valueType = "string";
        return item;
    });
}

/**
 * ACP elicitation（结构化提问）请求队列。
 *
 * 生命周期与 useAcpPermission 对齐：后端发 `acp-elicitation-request` 事件入队，
 * 前端提交 `confirm_acp_elicitation` 命令，或收到 `acp-elicitation-resolved`
 * （取消/其他窗口处理）时出队。
 *
 * UI 复用 ask_user_question 的 AskUserQuestionCard 内联卡片：
 * `questionRequest` 是转换后的卡片请求，`handleSubmit`/`handleCancel`
 * 分别对应 accept（带类型化 values）与 decline。
 */
export function useAcpElicitation(options: UseAcpElicitationOptions = {}) {
    const { conversationId, conversationIds } = options;
    const [requestQueue, setRequestQueue] = useState<AcpElicitationRequest[]>([]);
    const [decisionError, setDecisionError] = useState<string | null>(null);
    const [isSubmitting, setIsSubmitting] = useState(false);
    const pendingRequest = requestQueue[0] ?? null;

    const questionRequest: AskUserQuestionRequest | null = useMemo(() => {
        if (!pendingRequest) return null;
        return {
            request_id: pendingRequest.request_id,
            conversation_id: pendingRequest.conversation_id,
            questions: toAskUserQuestionItems(pendingRequest),
        };
    }, [pendingRequest]);

    const removeRequestById = useCallback((requestId: string) => {
        setRequestQueue((prev) => {
            const removedIndex = prev.findIndex((item) => item.request_id === requestId);
            if (removedIndex === -1) {
                return prev;
            }
            if (removedIndex === 0) {
                setDecisionError(null);
                setIsSubmitting(false);
            }
            return prev.filter((item) => item.request_id !== requestId);
        });
    }, []);

    useEffect(() => {
        const unsubscribe = listen<AcpElicitationRequest>(
            "acp-elicitation-request",
            (event) => {
                const request = event.payload;
                if (
                    !shouldHandleConversationRequest(
                        request.conversation_id,
                        conversationId,
                        conversationIds
                    )
                ) {
                    return;
                }

                console.log("Received ACP elicitation request:", request);
                setRequestQueue((prev) => {
                    if (prev.some((item) => item.request_id === request.request_id)) {
                        return prev;
                    }
                    if (prev.length === 0) {
                        setDecisionError(null);
                        setIsSubmitting(false);
                    }
                    return [...prev, request];
                });
            }
        );

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [conversationId, conversationIds]);

    useEffect(() => {
        const unsubscribe = listen<AcpElicitationResolvedEvent>(
            "acp-elicitation-resolved",
            (event) => {
                const requestId = event.payload?.request_id;
                if (!requestId) {
                    return;
                }
                console.log("Received ACP elicitation resolution:", event.payload);
                removeRequestById(requestId);
            }
        );

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [removeRequestById]);

    const sendDecision = useCallback(
        async (
            requestId: string,
            action: AcpElicitationAction,
            values?: Record<string, unknown>
        ) => {
            if (!pendingRequest || pendingRequest.request_id !== requestId || isSubmitting) {
                return;
            }
            setIsSubmitting(true);
            try {
                console.log("Sending ACP elicitation decision:", { requestId, action });
                await invoke("confirm_acp_elicitation", {
                    requestId,
                    action,
                    values: action === "accept" ? values ?? {} : null,
                });
                setDecisionError(null);
                removeRequestById(requestId);
            } catch (error) {
                const message = getErrorMessage(error) || "提交结构化提问回复失败";
                console.error("Failed to send ACP elicitation decision:", message);
                if (isStaleAcpElicitationError(message)) {
                    removeRequestById(requestId);
                    setDecisionError(null);
                    return;
                }
                setDecisionError(message);
                setIsSubmitting(false);
            }
        },
        [isSubmitting, pendingRequest, removeRequestById]
    );

    /**
     * 卡片提交（accept）：按问题元数据把文本答案还原为类型化值。
     * 数字解析失败时报错并保留卡片，不出队。
     */
    const handleSubmit = useCallback(
        async (requestId: string, answers: Record<string, string>) => {
            if (!questionRequest || questionRequest.request_id !== requestId) return;
            const values: Record<string, unknown> = {};

            for (const question of questionRequest.questions) {
                const key = question.answerKey ?? question.question;
                const raw = answers[key];
                if (raw === undefined) continue; // 选填未作答，直接省略
                const label = question.header || key;

                switch (question.valueType) {
                    case "number": {
                        const parsed = parseFloat(raw);
                        if (Number.isNaN(parsed)) {
                            setDecisionError(`「${label}」需要填写数字`);
                            return;
                        }
                        values[key] = parsed;
                        break;
                    }
                    case "integer": {
                        const parsed = parseInt(raw, 10);
                        if (Number.isNaN(parsed)) {
                            setDecisionError(`「${label}」需要填写整数`);
                            return;
                        }
                        values[key] = parsed;
                        break;
                    }
                    case "boolean": {
                        const mapped = question.optionValues?.[raw] ?? raw;
                        if (mapped === "true" || mapped === "false") {
                            values[key] = mapped === "true";
                        } else {
                            setDecisionError(`「${label}」需要选择「是」或「否」`);
                            return;
                        }
                        break;
                    }
                    case "stringArray": {
                        // 卡片多选答案以 ", " 连接；label 本身含 ", " 时回拆会错位（已知边界）
                        values[key] = raw
                            .split(", ")
                            .filter((part) => part.length > 0)
                            .map((part) => question.optionValues?.[part] ?? part);
                        break;
                    }
                    default: {
                        values[key] = question.optionValues?.[raw] ?? raw;
                        break;
                    }
                }
            }

            await sendDecision(requestId, "accept", values);
        },
        [questionRequest, sendDecision]
    );

    /** 卡片取消 → decline */
    const handleCancel = useCallback(
        async (requestId: string) => {
            await sendDecision(requestId, "decline");
        },
        [sendDecision]
    );

    return {
        pendingRequest,
        questionRequest,
        decisionError,
        isSubmitting,
        handleSubmit,
        handleCancel,
    };
}
