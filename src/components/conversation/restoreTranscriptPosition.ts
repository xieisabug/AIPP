/** Keep the same fraction as the virtual list measures its replacement rows. */
export function restoreTranscriptPosition(container: HTMLElement, fraction: number, sync: () => void) {
    let frame = 0;
    const deadline = performance.now() + 500;
    const intentEvents = ["wheel", "touchstart", "pointerdown", "keydown"];
    const stop = () => {
        cancelAnimationFrame(frame);
        for (const event of intentEvents) container.removeEventListener(event, stop, true);
    };
    const restore = () => {
        const target = Math.max(0, container.scrollHeight - container.clientHeight) * fraction;
        if (Math.abs(container.scrollTop - target) > 0.5) container.scrollTop = target;
        sync();
        if (performance.now() < deadline) frame = requestAnimationFrame(restore);
        else stop();
    };
    for (const event of intentEvents) container.addEventListener(event, stop, { capture: true, passive: true });
    restore();
    return stop;
}
