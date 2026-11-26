import { sendRowSelector } from "@/utils/composerMessage";
import { BUTLER_BUBBLE_ENTRY_MS, playButlerSentBubble } from './butlerBubbleAnimation';
import type { SendLayout } from './SendLayoutSnapshot';
import { holdForSendMotion } from './sendMotion';
import { playSentBubbleAnimation, SENT_BUBBLE_ANIMATION_MS } from './sentBubbleAnimation';

export const SEND_SCROLL_DURATION_MS = 260;
const SETTLE_MS = 120;
const ease = (t: number) => t < .5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;

/** Keep native scroll position and content in the same coordinate system. */
export function startSendScrollSequence({ container, content, previousLayout, messageId, butlerBubbleMotion = false, onComplete, onUserRelease, onScrollStateChange }: {
    container: HTMLDivElement;
    content?: HTMLDivElement | null;
    previousLayout?: SendLayout | null;
    messageId: number | string;
    butlerBubbleMotion?: boolean;
    onComplete: () => void;
    onUserRelease: () => void;
    onScrollStateChange?: (container: HTMLDivElement) => void;
}): () => void {
    const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
    // Defer conversation re-renders (e.g. confirming the optimistic bubble) until the motion ends.
    const releaseMotion = holdForSendMotion();
    const previousBehavior = container.style.scrollBehavior;
    container.style.scrollBehavior = 'auto';
    // Keep the pre-send scrollbar geometry during entry, then grow the reserved
    // space with real scrolling. No root transform and no end-position teleport.
    const savedHeight = content?.style.height ?? '';
    const savedOverflow = content?.style.overflow ?? '';
    const savedFlexShrink = content?.style.flexShrink ?? '';
    let finalHeight = content?.getBoundingClientRect().height ?? 0;
    const initialHeight = previousLayout ? Math.max(previousLayout.height, finalHeight - Math.max(0, container.scrollHeight - previousLayout.scrollHeight)) : finalHeight;
    let heightFrom = initialHeight;
    let currentBaseHeight = initialHeight;
    let viewportExpandProgress = 0;
    const animateHeight = !!content && !!previousLayout && !reduced;
    const heightStyle = (height: number, progress: number) => previousLayout
        ? `calc(${height}px + max(0px, 100% - ${previousLayout.viewportHeight}px) * ${1-progress})`
        : height + 'px';
    if (animateHeight) {
        content.style.height = heightStyle(initialHeight, 0);
        content.style.overflow = 'clip';
        content.style.flexShrink = '0';
        container.scrollTop = previousLayout!.scrollTop;
    }
    // Compensate late virtual-row measurement on the message rows themselves.
    // Moving the Virtuoso root would change its viewport coordinates and trigger
    // another native scroll correction.
    const translatedRows = new Map<HTMLElement, string>();
    let entryOffset = 0;
    let displayedOffset = 0;
    const restoreRows = () => {
        translatedRows.forEach((translate, row) => { row.style.translate = translate; });
        translatedRows.clear();
    };
    const applyOffset = (offset: number) => {
        displayedOffset = offset;
        if (!offset) { restoreRows(); return; }
        container.querySelectorAll<HTMLElement>('[data-message-item]').forEach(row => {
            if (row.parentElement?.closest('[data-message-item]')) return;
            if (!translatedRows.has(row)) translatedRows.set(row, row.style.translate ?? '');
            row.style.translate = `0 ${offset}px`;
        });
    };
    const preserveAnchor = () => {
        if (previousLayout?.anchorTop == null || !previousLayout.anchorId) return;
        const anchor = container.querySelector<HTMLElement>(`[data-message-item][data-message-id='${previousLayout.anchorId}']`);
        if (!anchor) return;
        const delta = previousLayout.anchorTop - (anchor.getBoundingClientRect().top - container.getBoundingClientRect().top);
        if (Math.abs(delta) > .5) { entryOffset += delta; applyOffset(entryOffset); }
    };
    if (!reduced) preserveAnchor();
    let stopped = false;
    let frame = 0;
    const started = performance.now();
    let scrollStarted: number | null = null;
    let scrollPhaseStarted: number | null = null;
    let from = container.scrollTop;
    let destination = from;
    let duration = SEND_SCROLL_DURATION_MS;
    let stableSince: number | null = null;
    container.dataset.sendPhase = reduced ? 'scroll' : 'entry';
    const entryDuration = butlerBubbleMotion ? BUTLER_BUBBLE_ENTRY_MS : SENT_BUBBLE_ANIMATION_MS;
    const stopBubble = butlerBubbleMotion
        ? playButlerSentBubble(container, messageId)
        : playSentBubbleAnimation(container, messageId);
    const synchronizeEntry = () => {
        if (!stopped && scrollStarted === null && !reduced) {
            container.scrollTop = from;
            preserveAnchor();
        }
    };
    // Virtuoso can publish a spacer measurement after our rAF callback.
    // Repair that entry-phase change before paint, not one frame later.
    const entryMutation = new MutationObserver(synchronizeEntry);
    if (content) entryMutation.observe(content, {subtree:true,childList:true,attributes:true,attributeFilter:['style']});
    const entryResize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(synchronizeEntry);
    if (content) entryResize?.observe(content);
    entryResize?.observe(container);
    const bubbleReady = () => {
        const bubble = container.querySelector<HTMLElement>(sendRowSelector(messageId))?.lastElementChild;
        if (!bubble) return true;
        if (butlerBubbleMotion && Array.from(bubble.children).some(child => Number.parseFloat(getComputedStyle(child).opacity) < .99)) return false;
        const opacity = Number.parseFloat(getComputedStyle(bubble).opacity);
        return !Number.isFinite(opacity) || opacity >= .99;
    };
    const target = () => {
        const row = container.querySelector<HTMLElement>(sendRowSelector(messageId));
        if (!row) return null;
        return Math.max(0, Math.min(container.scrollHeight - container.clientHeight + (animateHeight ? finalHeight - content!.getBoundingClientRect().height : 0),
            container.scrollTop + row.getBoundingClientRect().top - container.getBoundingClientRect().top - 8 - displayedOffset));
    };
    const cleanup = () => {
        if (stopped) return;
        stopped = true;
        cancelAnimationFrame(frame);
        entryMutation.disconnect();
        entryResize?.disconnect();
        stopBubble();
        if (animateHeight) {
            content!.style.height = savedHeight;
            content!.style.overflow = savedOverflow;
            content!.style.flexShrink = savedFlexShrink;
        }
        restoreRows();
        if (displayedOffset) container.scrollTop -= displayedOffset;
        container.style.scrollBehavior = previousBehavior;
        delete container.dataset.sendPhase;
        container.removeEventListener('wheel', release);
        container.removeEventListener('touchmove', release);
        container.removeEventListener('touchstart', release);
        container.removeEventListener('pointerdown', release);
        container.removeEventListener('keydown', key);
        releaseMotion();
    };
    const release = () => { if (!stopped) { cleanup(); onUserRelease(); } };
    const key = (event: KeyboardEvent) => {
        if (['ArrowUp','ArrowDown','PageUp','PageDown','Home','End',' '].includes(event.key)) release();
    };
    container.addEventListener('wheel', release, {passive:true});
    container.addEventListener('touchmove', release, {passive:true});
    container.addEventListener('touchstart', release, {passive:true});
    container.addEventListener('pointerdown', release, {passive:true});
    container.addEventListener('keydown', key);
    const tick = () => {
        if (stopped) return;
        const now = performance.now();
        if (animateHeight && content!.firstElementChild) {
            finalHeight = content!.firstElementChild.getBoundingClientRect().height;
        }
        const next = target();
        if (next === null) {
            if (now - started > 1500) { cleanup(); onComplete(); return; }
        } else if (reduced) {
            container.scrollTop = next;
            onScrollStateChange?.(container);
            cleanup(); onComplete(); return;
        } else if (scrollStarted === null && (now - started < entryDuration || !bubbleReady())) {
            // Virtual measurements may adjust scrollTop after the send commit.
            // Entry owns this position until the actual scrolling phase starts.
            container.scrollTop = from;
            preserveAnchor();

        } else {
            container.dataset.sendPhase = 'scroll';
            if (scrollStarted === null) {
                scrollStarted = now; scrollPhaseStarted = now; from = container.scrollTop; destination = next;
            } else if (Math.abs(next - destination) > 1) {
                // Rebase late virtual measurements from the currently displayed position.
                from = container.scrollTop; destination = next; scrollStarted = now;
                heightFrom = currentBaseHeight;
                entryOffset = displayedOffset;
                duration = Math.max(120, SEND_SCROLL_DURATION_MS - (now - started - entryDuration));
                stableSince = null;
            }
            const progress = Math.min(1, (now - scrollStarted) / duration);
            const eased = ease(progress);
            applyOffset(entryOffset * (1 - eased));
            if (animateHeight) {
                currentBaseHeight = heightFrom + (finalHeight - heightFrom) * eased;
                viewportExpandProgress = Math.max(viewportExpandProgress, eased);
                content!.style.height = heightStyle(currentBaseHeight, viewportExpandProgress);
            }
            container.scrollTop = from + (destination - from) * eased;
            onScrollStateChange?.(container);
            if (progress === 1 && Math.abs(container.scrollTop - next) <= 1) {
                stableSince ??= now;
                if (now - stableSince >= SETTLE_MS) { cleanup(); onComplete(); return; }
            } else stableSince = null;
        }
        // Bound a perpetually changing response; hand back to the normal layout follower.
        if (scrollPhaseStarted !== null && now - scrollPhaseStarted > 1500) { cleanup(); onComplete(); return; }
        frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return cleanup;
}
