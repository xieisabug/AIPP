import type { Message } from "@/data/Conversation";

export type StreamRetryStatus = "interrupted" | "completed" | string;

export interface StreamRetryMeta {
    sessionId: string;
    attempt: number;
    status: StreamRetryStatus;
    error?: string;
}

export interface StreamRetryAttemptBucket {
    /** null when messages have no stream_retry metadata */
    attempt: number | null;
    status: StreamRetryStatus | null;
    sessionId: string | null;
    error?: string;
    messages: Message[];
}

function asFinitePositiveInt(value: unknown): number | null {
    const num = typeof value === "number" ? value : typeof value === "string" ? Number(value) : NaN;
    if (!Number.isFinite(num) || num <= 0) {
        return null;
    }
    return Math.floor(num);
}

/**
 * Parsed by metadata string. The list re-reads every message's metadata on each
 * transcript update, and agent messages carry tens of KB of activities there.
 * Identical strings hit the cache; most messages are rejected by the substring check.
 */
const streamRetryMetaCache = new Map<string, StreamRetryMeta | null>();
const STREAM_RETRY_META_CACHE_LIMIT = 500;

export function parseStreamRetryMeta(message: Message | null | undefined): StreamRetryMeta | null {
    const raw = message?.metadata_json;
    if (!raw || !raw.includes("stream_retry")) {
        return null;
    }
    const cached = streamRetryMetaCache.get(raw);
    if (cached !== undefined) {
        return cached;
    }
    const meta = parseStreamRetryMetaJson(raw);
    if (streamRetryMetaCache.size >= STREAM_RETRY_META_CACHE_LIMIT) {
        streamRetryMetaCache.clear();
    }
    streamRetryMetaCache.set(raw, meta);
    return meta;
}

function parseStreamRetryMetaJson(raw: string): StreamRetryMeta | null {
    try {
        const parsed = JSON.parse(raw) as {
            stream_retry?: {
                session_id?: unknown;
                attempt?: unknown;
                status?: unknown;
                error?: unknown;
            };
        };
        const retry = parsed.stream_retry;
        if (!retry || typeof retry !== "object") {
            return null;
        }
        const sessionId = typeof retry.session_id === "string" ? retry.session_id.trim() : "";
        const attempt = asFinitePositiveInt(retry.attempt);
        const status = typeof retry.status === "string" ? retry.status.trim() : "";
        if (!sessionId || attempt == null || !status) {
            return null;
        }
        const error = typeof retry.error === "string" && retry.error.trim()
            ? retry.error.trim()
            : undefined;
        return { sessionId, attempt, status, error };
    } catch {
        return null;
    }
}

/**
 * Group consecutive messages that share the same stream_retry attempt
 * (or lack metadata) so the chat UI can collapse interrupted partials.
 */
export function groupMessagesByStreamRetryAttempt(
    messages: Message[],
): StreamRetryAttemptBucket[] {
    const buckets: StreamRetryAttemptBucket[] = [];
    for (const message of messages) {
        const meta = parseStreamRetryMeta(message);
        const last = buckets[buckets.length - 1];
        const sameAttempt =
            last != null
            && last.attempt === (meta?.attempt ?? null)
            && last.sessionId === (meta?.sessionId ?? null)
            && last.status === (meta?.status ?? null);
        if (sameAttempt && last) {
            last.messages.push(message);
            if (!last.error && meta?.error) {
                last.error = meta.error;
            }
            continue;
        }
        buckets.push({
            attempt: meta?.attempt ?? null,
            status: meta?.status ?? null,
            sessionId: meta?.sessionId ?? null,
            error: meta?.error,
            messages: [message],
        });
    }
    return buckets;
}

/** Show attempt chrome when there is an interrupted partial or multiple attempts. */
export function shouldShowStreamRetryGrouping(
    buckets: StreamRetryAttemptBucket[],
): boolean {
    const tagged = buckets.filter((bucket) => bucket.attempt != null);
    if (tagged.length === 0) {
        return false;
    }
    if (tagged.some((bucket) => bucket.status === "interrupted")) {
        return true;
    }
    const attempts = new Set(tagged.map((bucket) => bucket.attempt));
    return attempts.size > 1;
}

export function isInterruptedStreamRetryBucket(
    bucket: StreamRetryAttemptBucket,
): boolean {
    return bucket.status === "interrupted";
}

/** Prefer the latest non-interrupted response content for the merged toolbar copy. */
export function collectPreferredRetryCopyContent(
    messages: Message[],
): string {
    const buckets = groupMessagesByStreamRetryAttempt(messages);
    if (!shouldShowStreamRetryGrouping(buckets)) {
        return messages
            .filter((message) => message.message_type === "response")
            .map((message) => message.content)
            .filter(Boolean)
            .join("\n");
    }

    const preferred =
        [...buckets].reverse().find((bucket) => bucket.status !== "interrupted")
        ?? buckets[buckets.length - 1];

    const fromPreferred = (preferred?.messages ?? [])
        .filter((message) => message.message_type === "response")
        .map((message) => message.content)
        .filter(Boolean)
        .join("\n");
    if (fromPreferred) {
        return fromPreferred;
    }
    return messages
        .filter((message) => message.message_type === "response")
        .map((message) => message.content)
        .filter(Boolean)
        .join("\n");
}
