/** Butler-only entry. Geometry stays unchanged so virtual rows and scroll anchors do not jump. */
export const BUTLER_BUBBLE_ENTRY_MS = 400;
export const BUTLER_BUBBLE_SHRINK_MS = 300;
export function playButlerBubbleAnimation(bubble: HTMLElement): () => void {
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches || !bubble.animate) return () => {};
    const row = bubble.closest<HTMLElement>('[data-message-item]');
    const { width, height } = bubble.getBoundingClientRect();
    if (!row || width <= 0 || height <= 0) return () => {};
    const sx = Math.max(1, row.getBoundingClientRect().width / width);
    const sy = 1 + Math.min(height * .5, 100) / height;
    const origin = bubble.style.transformOrigin;
    // Transforms can enlarge scrollable overflow even though layout is unchanged.
    // Clip overflow propagation, but allow the requested extra 100px to be painted.
    const overflow = row.style.overflow;
    const clipMargin = row.style.overflowClipMargin;
    row.style.overflow = 'clip';
    row.style.overflowClipMargin = '100px';
    const restore = () => {
        bubble.style.transformOrigin = origin;
        row.style.overflow = overflow;
        row.style.overflowClipMargin = clipMargin;
    };
    bubble.style.transformOrigin = row.dataset.messageType === 'user' ? '100% 0%' : '0% 0%';
    const shape = bubble.animate([
        { transform: `scale(${sx}, ${sy})`, offset: 0, easing: 'cubic-bezier(.16,1,.3,1)' },
        { transform: 'scale(.975,.975)', offset: .72, easing: 'ease-out' },
        { transform: 'scale(1.006,1.006)', offset: .9, easing: 'ease-out' },
        { transform: 'scale(1,1)', offset: 1 },
    ], { duration: BUTLER_BUBBLE_SHRINK_MS, fill: 'backwards' });
    // Hide all visible contents while the shell contracts; streaming updates stay in these nodes.
    const text = Array.from(bubble.children).map(child => child.animate([
        { opacity: 0 }, { opacity: 1 },
    ], { duration: 100, delay: BUTLER_BUBBLE_SHRINK_MS, fill: 'backwards', easing: 'ease-out' }));
    shape.onfinish = restore;
    return () => { shape.cancel(); text.forEach(animation => animation.cancel()); restore(); };
}

/** Virtualized rows can mount after the send commit. Never fall back to the Chat animation. */
export function playButlerSentBubble(container: HTMLElement, messageId: number): () => void {
    let frame = 0;
    let attempts = 0;
    let stop = () => {};
    const run = () => {
        const bubble = container.querySelector<HTMLElement>(`[data-message-item][data-message-id='${messageId}'] [data-bubble-entry-id]`);
        if (bubble) { stop = playButlerBubbleAnimation(bubble); return; }
        if (++attempts < 10) frame = requestAnimationFrame(run);
    };
    run();
    return () => { cancelAnimationFrame(frame); stop(); };
}
