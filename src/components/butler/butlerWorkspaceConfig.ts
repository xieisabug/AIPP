export interface TrustedWorkspace {
    path: string;
    description: string;
}

export interface ButlerWorkspaceConfig {
    mainWorkspace: TrustedWorkspace | null;
    trustedWorkspaces: TrustedWorkspace[];
    allTrustedWorkspaces: TrustedWorkspace[];
}

export const BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION =
    "ai的全能工作区域，允许ai使用该区域组织所需要使用的任何事物。";

function normalizeWorkspacePath(path: unknown): string {
    return typeof path === "string" ? path.trim() : "";
}

function normalizeWorkspaceDescription(description: unknown, fallback = ""): string {
    const normalized = typeof description === "string" ? description.trim() : "";
    return normalized || fallback;
}

function workspaceKey(path: string): string {
    return path.trim().toLowerCase();
}

function dedupeWorkspaces(workspaces: TrustedWorkspace[]): TrustedWorkspace[] {
    const seen = new Set<string>();
    const normalized: TrustedWorkspace[] = [];

    for (const workspace of workspaces) {
        const path = normalizeWorkspacePath(workspace.path);
        if (!path) {
            continue;
        }
        const key = workspaceKey(path);
        if (seen.has(key)) {
            continue;
        }
        seen.add(key);
        normalized.push({
            path,
            description: normalizeWorkspaceDescription(workspace.description),
        });
    }

    return normalized;
}

export function parseTrustedWorkspaces(raw: string | null | undefined): TrustedWorkspace[] {
    const trimmed = raw?.trim() || "";
    if (!trimmed) {
        return [];
    }

    if (trimmed.startsWith("[")) {
        try {
            const parsed = JSON.parse(trimmed) as unknown;
            if (Array.isArray(parsed)) {
                return dedupeWorkspaces(
                    parsed
                        .filter((item): item is { path?: unknown; description?: unknown } =>
                            typeof item === "object" && item !== null
                        )
                        .map((item) => ({
                            path: normalizeWorkspacePath(item.path),
                            description: normalizeWorkspaceDescription(item.description),
                        }))
                );
            }
        } catch {
            // Fall through to legacy newline parsing.
        }
    }

    return dedupeWorkspaces(
        trimmed
            .split("\n")
            .map((line) => normalizeWorkspacePath(line))
            .filter(Boolean)
            .map((path) => ({ path, description: "" }))
    );
}

export function buildButlerWorkspaceConfig(params: {
    mainWorkspacePath?: string | null;
    mainWorkspaceDescription?: string | null;
    trustedWorkspacesRaw?: string | null;
}): ButlerWorkspaceConfig {
    const parsedTrustedWorkspaces = parseTrustedWorkspaces(params.trustedWorkspacesRaw);
    const explicitMainWorkspacePath = normalizeWorkspacePath(params.mainWorkspacePath);

    const mainWorkspace =
        explicitMainWorkspacePath
            ? {
                path: explicitMainWorkspacePath,
                description: normalizeWorkspaceDescription(
                    params.mainWorkspaceDescription,
                    BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION
                ),
            }
            : parsedTrustedWorkspaces.length > 0
                ? {
                    path: parsedTrustedWorkspaces[0].path,
                    description: normalizeWorkspaceDescription(
                        parsedTrustedWorkspaces[0].description,
                        BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION
                    ),
                }
                : null;

    const mainWorkspaceKey = mainWorkspace ? workspaceKey(mainWorkspace.path) : null;
    const trustedWorkspaceCandidates = explicitMainWorkspacePath
        ? parsedTrustedWorkspaces
        : parsedTrustedWorkspaces.slice(1);
    const trustedWorkspaces = trustedWorkspaceCandidates.filter((workspace) => {
        if (!mainWorkspaceKey) {
            return true;
        }
        return workspaceKey(workspace.path) !== mainWorkspaceKey;
    });

    return {
        mainWorkspace,
        trustedWorkspaces,
        allTrustedWorkspaces: mainWorkspace
            ? [mainWorkspace, ...trustedWorkspaces]
            : trustedWorkspaces,
    };
}

export function serializeButlerWorkspaceConfig(config: {
    mainWorkspacePath?: string | null;
    mainWorkspaceDescription?: string | null;
    trustedWorkspaces?: TrustedWorkspace[];
}) {
    const mainWorkspacePath = normalizeWorkspacePath(config.mainWorkspacePath);
    const trustedWorkspaces = dedupeWorkspaces(config.trustedWorkspaces || []).filter(
        (workspace) => !mainWorkspacePath || workspaceKey(workspace.path) !== workspaceKey(mainWorkspacePath)
    );
    const mainWorkspaceDescription = mainWorkspacePath
        ? normalizeWorkspaceDescription(
            config.mainWorkspaceDescription,
            BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION
        )
        : "";

    return {
        mainWorkspacePath,
        mainWorkspaceDescription,
        trustedWorkspacesRaw:
            trustedWorkspaces.length > 0 ? JSON.stringify(trustedWorkspaces) : "",
        allTrustedWorkspaces: mainWorkspacePath
            ? [
                {
                    path: mainWorkspacePath,
                    description: mainWorkspaceDescription,
                },
                ...trustedWorkspaces.filter(
                    (workspace) => workspaceKey(workspace.path) !== workspaceKey(mainWorkspacePath)
                ),
            ]
            : trustedWorkspaces,
    };
}
