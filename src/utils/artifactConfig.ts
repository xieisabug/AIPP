export interface ArtifactRuntimeConfig {
    db_id?: string;
    assistant_id?: number;
}

const ARTIFACT_RUNTIME_CONFIG_KEY = 'artifact_preview_config';

export function generateRandomDbId(): string {
    const timestamp = Date.now().toString(36);
    const randomPart = Math.random().toString(36).slice(2, 8);
    return `artifact-${timestamp}-${randomPart}`;
}

export function normalizeDbId(value: string): string {
    return value.replace(/[^a-zA-Z0-9_-]/g, '').slice(0, 64);
}

export function loadArtifactRuntimeConfig(): ArtifactRuntimeConfig {
    if (typeof window === 'undefined') {
        return {};
    }

    try {
        const stored = window.localStorage.getItem(ARTIFACT_RUNTIME_CONFIG_KEY);
        if (stored) {
            const parsed = JSON.parse(stored) as ArtifactRuntimeConfig;
            const dbId = parsed.db_id ? normalizeDbId(parsed.db_id) : '';
            const assistantId = typeof parsed.assistant_id === 'number' ? parsed.assistant_id : undefined;
            const normalized = {
                db_id: dbId || generateRandomDbId(),
                assistant_id: assistantId,
            };
            if (dbId !== parsed.db_id) {
                persistArtifactRuntimeConfig(normalized);
            }
            return normalized;
        }
    } catch {
        // ignore invalid storage entries
    }

    const fallback = { db_id: generateRandomDbId() };
    persistArtifactRuntimeConfig(fallback);
    return fallback;
}

export function persistArtifactRuntimeConfig(config: ArtifactRuntimeConfig): void {
    if (typeof window === 'undefined') {
        return;
    }

    try {
        const payload: ArtifactRuntimeConfig = {};
        const dbId = config.db_id ? normalizeDbId(config.db_id) : '';
        if (dbId) {
            payload.db_id = dbId;
        }
        if (typeof config.assistant_id === 'number') {
            payload.assistant_id = config.assistant_id;
        }
        window.localStorage.setItem(ARTIFACT_RUNTIME_CONFIG_KEY, JSON.stringify(payload));
    } catch {
        // ignore storage errors
    }
}
