import React, { useSyncExternalStore } from "react";

export const AUTO_MCP_TOOL_COMPONENT_ID = "auto";
export const DEFAULT_MCP_TOOL_COMPONENT_ID = "builtin.default";

export type McpToolCallStatus = "pending" | "executing" | "success" | "failed" | "unknown";

export interface McpToolComponentToolCall {
    id?: number;
    call_id: number;
    conversation_id: number;
    message_id?: number;
    status: McpToolCallStatus;
    llm_call_id?: string;
    server_name?: string;
    tool_name?: string;
    parameters?: string;
    result?: string;
    error?: string;
    started_time?: Date;
    finished_time?: Date;
}

export interface McpToolComponentProps {
    serverName?: string;
    toolName?: string;
    parameters?: string;
    llmCallId?: string;
    status?: McpToolCallStatus;
    error?: string;
    conversationId?: number;
    messageId?: number;
    callId?: number;
    currentToolCall?: McpToolComponentToolCall;
    mcpToolCallStates?: Map<number, McpToolComponentToolCall>;
    shiningMcpCallId?: number | null;
    isLastCall?: boolean;
    isStreaming?: boolean;
    isLastMessage?: boolean;
    streamingPreviewState?: unknown;
}

export interface McpToolComponentMatcher {
    serverName?: string;
    toolName?: string;
}

export type McpToolComponentRenderer = (props: McpToolComponentProps) => React.ReactNode;

export interface McpToolComponentRegistration {
    id: string;
    label: string;
    description?: string | null;
    match?: McpToolComponentMatcher[];
    priority?: number;
    render: McpToolComponentRenderer;
    shouldRender?: (props: McpToolComponentProps) => boolean;
}

export interface RegisteredMcpToolComponent extends McpToolComponentRegistration {
    id: string;
    label: string;
    match: McpToolComponentMatcher[];
    ownerCode: string;
    priority: number;
}

class McpToolComponentRegistry {
    private components = new Map<string, RegisteredMcpToolComponent>();
    private listeners = new Set<() => void>();
    private snapshot: RegisteredMcpToolComponent[] = [];

    subscribe(listener: () => void): () => void {
        this.listeners.add(listener);
        return () => {
            this.listeners.delete(listener);
        };
    }

    getSnapshot(): RegisteredMcpToolComponent[] {
        return this.snapshot;
    }

    listComponents(): RegisteredMcpToolComponent[] {
        return this.snapshot;
    }

    register(ownerCode: string, registration: McpToolComponentRegistration): void {
        const componentId = this.normalizeComponentId(registration.id);
        if (!componentId) {
            throw new Error("mcp tool component id is required");
        }
        if (typeof registration.render !== "function") {
            throw new Error(`mcp tool component '${componentId}' must provide a render function`);
        }

        const existing = this.components.get(componentId);
        if (existing && existing.ownerCode !== ownerCode) {
            throw new Error(`mcp tool component '${componentId}' is already registered by '${existing.ownerCode}'`);
        }

        this.components.set(componentId, {
            ...registration,
            id: componentId,
            label: String(registration.label || "").trim() || componentId,
            description: registration.description ? String(registration.description).trim() : undefined,
            match: this.normalizeMatchers(registration.match),
            ownerCode,
            priority: Number.isFinite(registration.priority) ? Number(registration.priority) : 0,
        });
        this.emitChange();
    }

    unregister(ownerCode: string, componentId: string): void {
        const normalizedId = this.normalizeComponentId(componentId);
        const existing = this.components.get(normalizedId);
        if (!existing || existing.ownerCode !== ownerCode) {
            return;
        }
        this.components.delete(normalizedId);
        this.emitChange();
    }

    clearForOwner(ownerCode: string): void {
        let changed = false;
        this.components.forEach((component, componentId) => {
            if (component.ownerCode !== ownerCode) {
                return;
            }
            this.components.delete(componentId);
            changed = true;
        });
        if (changed) {
            this.emitChange();
        }
    }

    clearStaleOwners(activeOwnerCodes: Set<string>): void {
        let changed = false;
        this.components.forEach((component, componentId) => {
            if (component.ownerCode === "builtin" || activeOwnerCodes.has(component.ownerCode)) {
                return;
            }
            this.components.delete(componentId);
            changed = true;
        });
        if (changed) {
            this.emitChange();
        }
    }

    resolve(
        props: McpToolComponentProps,
        selectedComponentId = AUTO_MCP_TOOL_COMPONENT_ID,
    ): RegisteredMcpToolComponent | null {
        const normalizedSelectedId = this.normalizeComponentId(selectedComponentId);
        if (!normalizedSelectedId || normalizedSelectedId === DEFAULT_MCP_TOOL_COMPONENT_ID) {
            return null;
        }

        if (normalizedSelectedId !== AUTO_MCP_TOOL_COMPONENT_ID) {
            const selected = this.components.get(normalizedSelectedId);
            if (!selected || !this.matches(selected, props)) {
                return null;
            }
            return this.canRender(selected, props) ? selected : null;
        }

        return this.snapshot.find((component) => (
            this.matches(component, props) && this.canRender(component, props)
        )) ?? null;
    }

    private canRender(component: RegisteredMcpToolComponent, props: McpToolComponentProps): boolean {
        if (typeof component.shouldRender !== "function") {
            return true;
        }
        try {
            return component.shouldRender(props);
        } catch (error) {
            console.warn(`[McpToolComponentRegistry] shouldRender failed for '${component.id}'`, error);
            return false;
        }
    }

    private matches(component: RegisteredMcpToolComponent, props: McpToolComponentProps): boolean {
        if (component.match.length === 0) {
            return true;
        }

        const serverName = this.normalizeMatcherValue(props.serverName);
        const toolName = this.normalizeMatcherValue(props.toolName);
        return component.match.some((matcher) => {
            const expectedServer = this.normalizeMatcherValue(matcher.serverName);
            const expectedTool = this.normalizeMatcherValue(matcher.toolName);
            return (!expectedServer || expectedServer === serverName)
                && (!expectedTool || expectedTool === toolName);
        });
    }

    private normalizeComponentId(componentId: string): string {
        return String(componentId || "").trim().toLowerCase();
    }

    private normalizeMatchers(matchers?: McpToolComponentMatcher[]): McpToolComponentMatcher[] {
        if (!Array.isArray(matchers)) {
            return [];
        }
        return matchers
            .map((matcher) => ({
                serverName: this.normalizeMatcherValue(matcher.serverName),
                toolName: this.normalizeMatcherValue(matcher.toolName),
            }))
            .filter((matcher) => matcher.serverName || matcher.toolName);
    }

    private normalizeMatcherValue(value?: string): string {
        return String(value || "").trim().toLowerCase();
    }

    private emitChange(): void {
        this.snapshot = [...this.components.values()].sort((a, b) => {
            if (b.priority !== a.priority) {
                return b.priority - a.priority;
            }
            return a.label.localeCompare(b.label);
        });
        this.listeners.forEach((listener) => listener());
    }
}

export const mcpToolComponentRegistry = new McpToolComponentRegistry();

export function useMcpToolComponentRegistrySnapshot(): RegisteredMcpToolComponent[] {
    return useSyncExternalStore(
        (listener) => mcpToolComponentRegistry.subscribe(listener),
        () => mcpToolComponentRegistry.getSnapshot(),
        () => mcpToolComponentRegistry.getSnapshot(),
    );
}
