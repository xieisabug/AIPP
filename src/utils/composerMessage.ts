import type { Message } from "@/data/Conversation";

export interface ComposerReceipt {
    conversationId: number;
    messageId?: number;
    error?: string;
}

let nextTemporaryId = -4_000_000_000_000;

export function newComposerMessage(conversationId: number, content: string): Message {
    return {
        clientRequestId: crypto.randomUUID(), sendStatus: "sending", id: nextTemporaryId--,
        conversation_id: conversationId, message_type: "user", content, created_time: new Date(),
        start_time: null, finish_time: null, llm_model_id: null, token_count: 0,
        input_token_count: 0, output_token_count: 0, regenerate: null,
    };
}

export function composerKey(message: Message): string | undefined {
    if (message.clientRequestId && /^[0-9a-f-]{36}$/i.test(message.clientRequestId)) return message.clientRequestId;
    if (message.message_type !== "user" || !message.metadata_json?.includes('"client_request_id"')) return;
    try {
        const key = JSON.parse(message.metadata_json).client_request_id;
        if (typeof key === "string" && /^[0-9a-f-]{36}$/i.test(key)) return key;
    } catch { /* Legacy metadata. */ }
}

export function rowIdentity(message: Message): number | string {
    return composerKey(message) ?? message.id;
}

export function sendRowSelector(id: number | string): string {
    return typeof id === "number" ? `[data-message-item][data-message-id='${id}']`
        : `[data-message-item][data-send-key='${id}']`;
}

export function upsertPersistedRow(rows: Message[], row: Message): Message[] {
    const index = rows.findIndex(item => item.id === row.id);
    if (index >= 0) return rows.map((item, i) => i === index ? row : item);
    const time = +new Date(row.created_time);
    const next = rows.findIndex(item => +new Date(item.created_time) > time
        || (+new Date(item.created_time) === time && item.id > row.id));
    if (next < 0) return [...rows, row];
    return [...rows.slice(0, next), row, ...rows.slice(next)];
}
