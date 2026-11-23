import { invoke } from "@tauri-apps/api/core";

export interface ProactiveConfig {
    enabled: boolean; private_chats: boolean; groups: boolean; normal_chats: boolean;
    allow_notifications: boolean; custom_instructions: string; pulse_hours: number; daily_limit: number;
    quiet_start: string; quiet_end: string; release: string; digest: string; small_model: string; deep_model: string;
    adaptive_window: boolean;
}
export interface ProactiveItem {
    id: number; kind: string; title: string; detail: string; tracking?: string; status: string; date?: string;
    next?: string; revision: number; owner?: number; instruction: string; source: number; position: number;
    yearly: boolean; calendar: string; lead: number;
}
export interface Center {
    deliveries: {title:string;status:string;date:string;stage:string;fallback?:string;external?:string;error?:string}[];
    window?: { release: string; digest: string; days: number };
    items: ProactiveItem[];
    runs: { id: string; trigger: string; status: string; summary: string; scanned: number; time: number; conversation?: number }[];
}
export interface Delivery {
    origin: "butler_proactive"; role: "result"; uuid: string; item: number; delivery: number; source: number;
    position: number; revision: number; kind: string; brief?: number; stage: string; cards?: Delivery[];
}
export function deliveryMetadata(raw?: string | null): Delivery | null {
    try {
        const data = JSON.parse(raw || "null");
        const valid = (card: Partial<Delivery> | null): boolean =>
            card?.origin === "butler_proactive" && card.role === "result" && typeof card.uuid === "string"
            && [card.item,card.delivery,card.revision,card.source,card.position].every(Number.isSafeInteger);
        if (!valid(data)) return null;
        if (data.cards !== undefined && (!Array.isArray(data.cards) || !data.cards.every(valid))) return null;
        return data;
    } catch { return null; }
}
export const center = () => invoke<Center>("get_butler_proactive_center");
export const changeItem = (id: number, revision: number, action: string, deliveryId?: number, title?: string,
    timing?: {date:string;yearly:boolean;calendar:string;lead:number}) =>
    invoke<void>("update_butler_proactive_item", { id, revision, action, deliveryId, title, timing });
export const undoProactiveMemory = (id: number, revision: number) => invoke<void>("undo_butler_proactive_memory", { id, revision });

export interface Confirmation {
    status: "confirm" | "sent" | "pending"; assistant?: number; name?: string; instruction?: string;
    revision?: number; redirected?: boolean; conversation?: number; message?: number;
}
export const accept = (id: number, revision: number, confirmedAssistant?: number, confirmedInstruction?: string) =>
    invoke<Confirmation>("accept_butler_proactive_proposal", { id, revision, confirmedAssistant, confirmedInstruction });
export const showConversation = (conversation: number, position?: number) =>
    window.dispatchEvent(new CustomEvent("butler-proactive-open", { detail: { conversation, position } }));
