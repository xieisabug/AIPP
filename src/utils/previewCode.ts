export type PreviewCodeInteractionMode = "none" | "submit_once";

export interface PreviewCodeMetadata {
    origin?: string;
}

export interface PreviewCodeRequest {
    title: string;
    renderer: string;
    code: string;
    loadingMessages: string[];
    interactionMode: PreviewCodeInteractionMode;
    metadata?: PreviewCodeMetadata;
}

export interface PreviewCodeRequestEvent extends PreviewCodeRequest {
    request_id: string;
    conversation_id?: number;
}

export interface PreviewCodeToolResult {
    status: string;
    request_id?: string;
    payload?: unknown;
}

function safeParseJson(value: string): unknown {
    try {
        return JSON.parse(value);
    } catch {
        return null;
    }
}

function decodeJsonStringFragment(raw: string): string {
    try {
        return JSON.parse(`"${raw}"`);
    } catch {
        return raw
            .replace(/\\n/g, "\n")
            .replace(/\\r/g, "\r")
            .replace(/\\t/g, "\t")
            .replace(/\\"/g, "\"")
            .replace(/\\\\/g, "\\");
    }
}

function normalizeInteractionMode(raw: unknown): PreviewCodeInteractionMode {
    return raw === "none" ? "none" : "submit_once";
}

function extractStringField(raw: string, fieldNames: string[]): string | undefined {
    for (const fieldName of fieldNames) {
        const exact = raw.match(new RegExp(`"${fieldName}"\\s*:\\s*"((?:\\\\.|[^"\\\\])*)"`, "s"));
        if (exact) {
            return decodeJsonStringFragment(exact[1]);
        }

        const partial = raw.match(new RegExp(`"${fieldName}"\\s*:\\s*"([\\s\\S]*)$`, "s"));
        if (partial) {
            return decodeJsonStringFragment(partial[1]);
        }
    }
    return undefined;
}

function extractStringArrayField(raw: string, fieldNames: string[]): string[] {
    for (const fieldName of fieldNames) {
        const match = raw.match(new RegExp(`"${fieldName}"\\s*:\\s*\\[([\\s\\S]*?)(?:\\]|$)`, "s"));
        if (!match) {
            continue;
        }
        const items: string[] = [];
        const itemRegex = /"((?:\\.|[^"\\])*)"/g;
        let itemMatch: RegExpExecArray | null;
        while ((itemMatch = itemRegex.exec(match[1])) !== null) {
            items.push(decodeJsonStringFragment(itemMatch[1]));
        }
        return items;
    }
    return [];
}

function normalizePreviewCodeRecord(record: Record<string, unknown>): PreviewCodeRequest | null {
    const title = typeof record.title === "string" ? record.title : "";
    const renderer = typeof record.renderer === "string" ? record.renderer : "";
    const code = typeof record.code === "string" ? record.code : "";
    if (!title && !renderer && !code) {
        return null;
    }

    const loadingMessagesRaw =
        Array.isArray(record.loading_messages)
            ? record.loading_messages
            : Array.isArray(record.loadingMessages)
              ? record.loadingMessages
              : [];
    const loadingMessages = loadingMessagesRaw.filter(
        (item): item is string => typeof item === "string" && item.trim().length > 0
    );
    const interactionModeRaw =
        typeof record.interaction_mode === "string"
            ? record.interaction_mode
            : typeof record.interactionMode === "string"
              ? record.interactionMode
              : undefined;
    const metadataRecord =
        record.metadata && typeof record.metadata === "object"
            ? (record.metadata as Record<string, unknown>)
            : undefined;

    return {
        title,
        renderer: renderer || "html",
        code,
        loadingMessages,
        interactionMode: normalizeInteractionMode(interactionModeRaw),
        metadata:
            metadataRecord && typeof metadataRecord.origin === "string"
                ? { origin: metadataRecord.origin }
                : undefined,
    };
}

export function parsePreviewCodeRequest(parameters: string): PreviewCodeRequest | null {
    const parsed = safeParseJson(parameters);
    if (!parsed || typeof parsed !== "object") {
        return null;
    }
    return normalizePreviewCodeRecord(parsed as Record<string, unknown>);
}

export function parsePreviewCodeRequestLoose(parameters: string): PreviewCodeRequest | null {
    const exact = parsePreviewCodeRequest(parameters);
    if (exact) {
        return exact;
    }

    const title = extractStringField(parameters, ["title"]);
    const renderer = extractStringField(parameters, ["renderer"]) ?? "html";
    const code = extractStringField(parameters, ["code"]) ?? "";
    const interactionMode = normalizeInteractionMode(
        extractStringField(parameters, ["interaction_mode", "interactionMode"])
    );
    const loadingMessages = extractStringArrayField(parameters, [
        "loading_messages",
        "loadingMessages",
    ]);
    const origin = extractStringField(parameters, ["origin"]);

    if (!title && !code && !loadingMessages.length) {
        return null;
    }

    return {
        title: title ?? "inline_preview",
        renderer,
        code,
        loadingMessages,
        interactionMode,
        metadata: origin ? { origin } : undefined,
    };
}

function extractPreviewCodeResultFromNode(node: unknown): PreviewCodeToolResult | null {
    if (!node) {
        return null;
    }
    if (typeof node === "string") {
        const parsed = safeParseJson(node);
        if (!parsed) {
            return null;
        }
        return extractPreviewCodeResultFromNode(parsed);
    }
    if (Array.isArray(node)) {
        for (const item of node) {
            const extracted = extractPreviewCodeResultFromNode(item);
            if (extracted) {
                return extracted;
            }
        }
        return null;
    }
    if (typeof node === "object") {
        const record = node as Record<string, unknown>;
        if (typeof record.status === "string") {
            return {
                status: record.status,
                request_id:
                    typeof record.request_id === "string" ? record.request_id : undefined,
                payload: record.payload,
            };
        }
        if (record.json) {
            const extracted = extractPreviewCodeResultFromNode(record.json);
            if (extracted) {
                return extracted;
            }
        }
        if (record.content) {
            const extracted = extractPreviewCodeResultFromNode(record.content);
            if (extracted) {
                return extracted;
            }
        }
        if (record.result) {
            const extracted = extractPreviewCodeResultFromNode(record.result);
            if (extracted) {
                return extracted;
            }
        }
    }
    return null;
}

export function parsePreviewCodeToolResult(result?: string | null): PreviewCodeToolResult | null {
    if (!result?.trim()) {
        return null;
    }
    const parsed = safeParseJson(result);
    if (!parsed) {
        return null;
    }
    return extractPreviewCodeResultFromNode(parsed);
}

export function buildPreviewCodeSignature(
    request: Pick<PreviewCodeRequest, "title" | "renderer" | "code" | "interactionMode"> | null
): string | null {
    if (!request) {
        return null;
    }
    return JSON.stringify({
        title: request.title,
        renderer: request.renderer,
        code: request.code,
        interactionMode: request.interactionMode,
    });
}

