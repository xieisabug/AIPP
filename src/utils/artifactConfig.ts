export interface ArtifactRuntimeConfig {
    db_id?: string;
    assistant_id?: number;
}

const ARTIFACT_RUNTIME_CONFIG_PREFIX = 'artifact_config_';
// Legacy key for migration
const LEGACY_ARTIFACT_RUNTIME_CONFIG_KEY = 'artifact_preview_config';

export function generateRandomDbId(): string {
    const timestamp = Date.now().toString(36);
    const randomPart = Math.random().toString(36).slice(2, 8);
    return `artifact-${timestamp}-${randomPart}`;
}

export function normalizeDbId(value: string): string {
    return value.replace(/[^a-zA-Z0-9_-]/g, '').slice(0, 64);
}

/**
 * Get the storage key for a specific conversation
 */
function getConfigKey(conversationId?: string | number): string {
    if (!conversationId) {
        return LEGACY_ARTIFACT_RUNTIME_CONFIG_KEY;
    }
    return `${ARTIFACT_RUNTIME_CONFIG_PREFIX}${conversationId}`;
}

/**
 * Load artifact runtime config for a specific conversation.
 * If no conversationId is provided, falls back to legacy global config.
 */
export function loadArtifactRuntimeConfig(conversationId?: string | number): ArtifactRuntimeConfig {
    if (typeof window === 'undefined') {
        return {};
    }

    const key = getConfigKey(conversationId);

    try {
        const stored = window.localStorage.getItem(key);
        if (stored) {
            const parsed = JSON.parse(stored) as ArtifactRuntimeConfig;
            const dbId = parsed.db_id ? normalizeDbId(parsed.db_id) : '';
            const assistantId = typeof parsed.assistant_id === 'number' ? parsed.assistant_id : undefined;
            const normalized = {
                db_id: dbId || generateRandomDbId(),
                assistant_id: assistantId,
            };
            if (dbId !== parsed.db_id) {
                persistArtifactRuntimeConfig(normalized, conversationId);
            }
            return normalized;
        }
    } catch {
        // ignore invalid storage entries
    }

    // Generate new config for this conversation
    const fallback = { db_id: generateRandomDbId() };
    persistArtifactRuntimeConfig(fallback, conversationId);
    return fallback;
}

/**
 * Persist artifact runtime config for a specific conversation.
 */
export function persistArtifactRuntimeConfig(config: ArtifactRuntimeConfig, conversationId?: string | number): void {
    if (typeof window === 'undefined') {
        return;
    }

    const key = getConfigKey(conversationId);

    try {
        const payload: ArtifactRuntimeConfig = {};
        const dbId = config.db_id ? normalizeDbId(config.db_id) : '';
        if (dbId) {
            payload.db_id = dbId;
        }
        if (typeof config.assistant_id === 'number') {
            payload.assistant_id = config.assistant_id;
        }
        window.localStorage.setItem(key, JSON.stringify(payload));
    } catch {
        // ignore storage errors
    }
}

/**
 * Clear artifact config for a specific conversation (e.g., when conversation is deleted)
 */
export function clearArtifactRuntimeConfig(conversationId: string | number): void {
    if (typeof window === 'undefined') {
        return;
    }

    const key = getConfigKey(conversationId);
    try {
        window.localStorage.removeItem(key);
    } catch {
        // ignore errors
    }
}
