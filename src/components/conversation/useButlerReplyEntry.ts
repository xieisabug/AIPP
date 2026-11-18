import { useLayoutEffect, useRef, type RefObject } from 'react';
import type { Message } from '@/data/Conversation';
import { playButlerBubbleAnimation } from './butlerBubbleAnimation';

/** Track message identity across DOM remounts; initial/async history is always a baseline. */
export function useButlerReplyEntry(containerRef: RefObject<HTMLDivElement | null>, conversationId: string,
    loaded: boolean, enabled: boolean, messages: Message[]) {
    const state = useRef({ conversationId: '', ready: false, known: new Set<number>(), pending: new Set<number>() });
    const animations = useRef(new Map<HTMLElement, () => void>());
    useLayoutEffect(() => {
        const container = containerRef.current;
        const previous = state.current;
        if (previous.conversationId !== conversationId || !enabled || !loaded) {
            animations.current.forEach(stop => stop()); animations.current.clear();
            state.current = { conversationId, ready: false, known: new Set(), pending: new Set() };
        }
        if (!enabled) return;
        const current = state.current;
        const present = new Set(messages.map(message => message.id));
        current.pending.forEach(id => { if (!present.has(id)) current.pending.delete(id); });
        for (const message of messages) {
            if (enabled && loaded && current.ready && !current.known.has(message.id) && ['response', 'assistant'].includes(message.message_type)) {
                current.pending.add(message.id);
            }
            current.known.add(message.id);
        }
        current.ready = enabled && loaded;
        if (!current.ready || !container) return;
        const scan = () => {
            animations.current.forEach((stop, element) => {
                if (!element.isConnected) { stop(); animations.current.delete(element); }
            });
            container.querySelectorAll<HTMLElement>('[data-bubble-entry-id]').forEach(bubble => {
                const id = Number(bubble.dataset.bubbleEntryId);
                // A later response segment joins an existing merged shell: consume it,
                // so switching merge settings cannot replay that historical segment.
                const members = bubble.dataset.bubbleEntryMembers?.split(',').map(Number) ?? [id];
                members.filter(member => member !== id).forEach(member => current.pending.delete(member));
                if (!current.pending.has(id) || !bubble.textContent?.trim()) return;
                current.pending.delete(id);
                animations.current.set(bubble, playButlerBubbleAnimation(bubble));
            });
        };
        scan();
        const observer = new MutationObserver(scan);
        observer.observe(container, { childList: true, subtree: true, characterData: true });
        return () => observer.disconnect();
    }, [containerRef, conversationId, loaded, enabled, messages]);
    useLayoutEffect(() => () => {
        animations.current.forEach(stop => stop()); animations.current.clear();
    }, []);
}
