import { createContext, useContext, useSyncExternalStore } from "react";

/**
 * Mobile settings: a page (via ConfigPageLayout) can open a second level (list → detail).
 * The settings header and system back must step back to the list before leaving the
 * settings page, so the open detail is published here, keyed by settings menu id.
 */
export interface ConfigSectionDetail {
    /** Title of the open detail item; null when the page shows its list. */
    title: string | null;
    /** Close the detail and return to the list. */
    back: () => void;
}

/** Menu id of the settings section rendering the current subtree (set by ConfigWindow). */
export const ConfigSectionContext = createContext<string | null>(null);

export function useConfigSectionId(): string | null {
    return useContext(ConfigSectionContext);
}

const details = new Map<string, ConfigSectionDetail>();
const listeners = new Set<() => void>();
const emit = () => listeners.forEach(listener => listener());

/** Publish (or clear with null) the open detail for a section. */
export function setConfigSectionDetail(sectionId: string, detail: ConfigSectionDetail | null): void {
    if (detail && detail.title !== null) details.set(sectionId, detail);
    else if (!details.delete(sectionId)) return;
    emit();
}

/** Returns true when the section had an open detail and it was closed. */
export function backConfigSection(sectionId: string): boolean {
    const detail = details.get(sectionId);
    if (!detail) return false;
    detail.back();
    return true;
}

const subscribe = (listener: () => void) => {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
};

export function useConfigSectionDetailTitle(sectionId: string): string | null {
    return useSyncExternalStore(subscribe, () => details.get(sectionId)?.title ?? null);
}
