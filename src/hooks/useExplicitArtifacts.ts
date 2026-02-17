import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { CodeArtifact } from "@/components/chat-sidebar/types";

interface ConversationArtifactItem {
    artifact_key: string;
    title: string;
    language: string;
    preview_type: string;
    entry_file: string;
    code: string;
    updated_at: string;
    db_id?: string;
    assistant_id?: number;
}

interface ArtifactManifestUpdatedEvent {
    conversation_id: number;
}

const REFRESH_TOOL_NAMES = new Set(["show_artifact", "write_file", "edit_file"]);

interface UseExplicitArtifactsOptions {
    conversationId: string;
    mcpToolCallStates: Map<number, MCPToolCallUpdateEvent>;
}

export function useExplicitArtifacts({
    conversationId,
    mcpToolCallStates,
}: UseExplicitArtifactsOptions): { artifacts: CodeArtifact[] } {
    const [artifacts, setArtifacts] = useState<CodeArtifact[]>([]);

    const parsedConversationId = useMemo(() => {
        if (!conversationId) return null;
        const parsed = Number.parseInt(conversationId, 10);
        return Number.isNaN(parsed) ? null : parsed;
    }, [conversationId]);

    const refreshSignal = useMemo(() => {
        return Array.from(mcpToolCallStates.values())
            .filter((call) => {
                if (call.status !== "success" || !call.tool_name) return false;
                return REFRESH_TOOL_NAMES.has(call.tool_name.toLowerCase());
            })
            .map((call) => call.call_id)
            .sort((a, b) => a - b)
            .join(",");
    }, [mcpToolCallStates]);

    const loadArtifacts = useCallback(async (targetConversationId: number) => {
        const result = await invoke<ConversationArtifactItem[]>("list_conversation_artifacts", {
            conversationId: targetConversationId,
        });
        const mapped = result.map((item) => ({
            id: `explicit-${item.artifact_key}`,
            language: item.language || item.preview_type,
            code: item.code,
            title: item.title || item.artifact_key,
            source: "manifest" as const,
            artifactKey: item.artifact_key,
            entryFile: item.entry_file,
            dbId: item.db_id,
            assistantId: item.assistant_id,
            updatedAt: item.updated_at,
        }));
        setArtifacts(mapped);
    }, []);

    useEffect(() => {
        if (parsedConversationId === null) {
            setArtifacts([]);
            return;
        }
        loadArtifacts(parsedConversationId).catch((error) => {
            console.error("[useExplicitArtifacts] Failed to load explicit artifacts:", error);
            setArtifacts([]);
        });
    }, [parsedConversationId, loadArtifacts, refreshSignal]);

    useEffect(() => {
        if (parsedConversationId === null) return;
        const unlistenPromise = listen<ArtifactManifestUpdatedEvent>(
            "artifact-manifest-updated",
            (event) => {
                if (event.payload.conversation_id !== parsedConversationId) return;
                loadArtifacts(parsedConversationId).catch((error) => {
                    console.error("[useExplicitArtifacts] Failed to refresh explicit artifacts:", error);
                });
            },
        );
        return () => {
            unlistenPromise.then((unlisten) => unlisten());
        };
    }, [parsedConversationId, loadArtifacts]);

    return { artifacts };
}
