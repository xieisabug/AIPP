import React, { useSyncExternalStore } from "react";

export interface MarkdownTagRendererProps {
    node?: unknown;
    children?: React.ReactNode;
    attributes: Record<string, string>;
    props: Record<string, unknown>;
}

export type MarkdownTagRenderer = (props: MarkdownTagRendererProps) => React.ReactNode;

export interface MarkdownTagRegistration {
    tagName: string;
    attributes?: string[];
    render: MarkdownTagRenderer;
}

export interface RegisteredMarkdownTag extends MarkdownTagRegistration {
    ownerCode: string;
    attributes: string[];
}

class MarkdownRegistry {
    private tags = new Map<string, RegisteredMarkdownTag>();
    private listeners = new Set<() => void>();
    private snapshot: RegisteredMarkdownTag[] = [];

    subscribe(listener: () => void): () => void {
        this.listeners.add(listener);
        return () => {
            this.listeners.delete(listener);
        };
    }

    getSnapshot(): RegisteredMarkdownTag[] {
        return this.snapshot;
    }

    listTags(): RegisteredMarkdownTag[] {
        return this.snapshot;
    }

    registerTag(ownerCode: string, registration: MarkdownTagRegistration): void {
        const tagName = this.normalizeTagName(registration.tagName);
        if (!tagName) {
            throw new Error("markdown tagName is required");
        }
        if (typeof registration.render !== "function") {
            throw new Error(`markdown tag '${tagName}' must provide a render function`);
        }

        const existing = this.tags.get(tagName);
        if (existing && existing.ownerCode !== ownerCode) {
            throw new Error(`markdown tag '${tagName}' is already registered by plugin '${existing.ownerCode}'`);
        }

        this.tags.set(tagName, {
            ownerCode,
            tagName,
            attributes: this.normalizeAttributes(registration.attributes),
            render: registration.render,
        });
        this.emitChange();
    }

    unregisterTag(ownerCode: string, tagName: string): void {
        const normalizedTagName = this.normalizeTagName(tagName);
        const existing = this.tags.get(normalizedTagName);
        if (!existing || existing.ownerCode !== ownerCode) {
            return;
        }
        this.tags.delete(normalizedTagName);
        this.emitChange();
    }

    clearTagsForPlugin(ownerCode: string): void {
        let changed = false;
        this.tags.forEach((tag, tagName) => {
            if (tag.ownerCode !== ownerCode) {
                return;
            }
            this.tags.delete(tagName);
            changed = true;
        });
        if (changed) {
            this.emitChange();
        }
    }

    clearStaleTags(activePluginCodes: Set<string>): void {
        let changed = false;
        this.tags.forEach((tag, tagName) => {
            if (activePluginCodes.has(tag.ownerCode)) {
                return;
            }
            this.tags.delete(tagName);
            changed = true;
        });
        if (changed) {
            this.emitChange();
        }
    }

    private emitChange(): void {
        this.snapshot = [...this.tags.values()].sort((a, b) => a.tagName.localeCompare(b.tagName));
        this.listeners.forEach((listener) => listener());
    }

    private normalizeTagName(tagName: string): string {
        return String(tagName || "")
            .trim()
            .toLowerCase();
    }

    private normalizeAttributes(attributes?: string[]): string[] {
        if (!Array.isArray(attributes)) {
            return [];
        }
        const unique = new Set<string>();
        attributes.forEach((attribute) => {
            const normalized = String(attribute || "").trim().toLowerCase();
            if (!normalized) {
                return;
            }
            unique.add(normalized);
        });
        return [...unique];
    }
}

export const markdownRegistry = new MarkdownRegistry();

export function useMarkdownRegistrySnapshot(): RegisteredMarkdownTag[] {
    return useSyncExternalStore(
        (listener) => markdownRegistry.subscribe(listener),
        () => markdownRegistry.getSnapshot(),
        () => markdownRegistry.getSnapshot(),
    );
}
