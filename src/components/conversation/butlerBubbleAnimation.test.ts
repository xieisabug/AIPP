import { afterEach, expect, it, vi } from 'vitest';
import { playButlerBubbleAnimation, playButlerSentBubble } from './butlerBubbleAnimation';
afterEach(() => { document.body.innerHTML = ''; vi.unstubAllGlobals(); });
it.each([40, 400])('should cap expanded height and reveal text after shrinking (%s px)', height => {
    document.body.innerHTML = '<div data-message-item data-message-type="user"><div><p>Text</p></div></div>';
    const row = document.body.firstElementChild as HTMLElement;
    const bubble = row.firstElementChild as HTMLElement;
    row.getBoundingClientRect = () => ({ width: 600 } as DOMRect);
    bubble.getBoundingClientRect = () => ({ width: 200, height } as DOMRect);
    const shapeCancel = vi.fn(); const textCancel = vi.fn();
    const shape = vi.fn(() => ({cancel: shapeCancel}) as unknown as Animation);
    const text = vi.fn(() => ({cancel: textCancel}) as unknown as Animation);
    bubble.animate = shape; bubble.firstElementChild!.animate = text;
    const stop = playButlerBubbleAnimation(bubble);
    expect(shape.mock.calls[0]).toEqual([expect.arrayContaining([expect.objectContaining({transform:`scale(3, ${1 + Math.min(height * .5, 100) / height})`})]), expect.objectContaining({duration:300})]);
    expect(text.mock.calls[0]).toEqual([[{opacity:0},{opacity:1}],expect.objectContaining({delay:300,duration:100,fill:'backwards'})]);
    expect(bubble.style.transformOrigin).toBe('100% 0%');
    stop(); expect(shapeCancel).toHaveBeenCalledOnce(); expect(textCancel).toHaveBeenCalledOnce();
    expect(bubble.style.transformOrigin).toBe('');
    expect(row.style.overflow).toBe('');
    expect(row.style.overflowClipMargin).toBe('');
});
it('should skip animation for reduced motion', () => {
    vi.stubGlobal('matchMedia', () => ({matches:true}));
    const bubble = document.createElement('div'); bubble.animate = vi.fn();
    playButlerBubbleAnimation(bubble); expect(bubble.animate).not.toHaveBeenCalled();
});

it('should wait for a virtual user row before playing the Butler animation', () => {
    let next: FrameRequestCallback | undefined;
    vi.stubGlobal('requestAnimationFrame', (cb:FrameRequestCallback) => { next = cb; return 1; });
    vi.stubGlobal('cancelAnimationFrame', vi.fn());
    const container = document.createElement('div'); document.body.appendChild(container);
    const stop = playButlerSentBubble(container, 3);
    container.innerHTML = '<div data-message-item data-message-id="3"><div data-bubble-entry-id="3"><p>hello</p></div></div>';
    const row = container.firstElementChild as HTMLElement; const bubble = row.firstElementChild as HTMLElement;
    row.getBoundingClientRect = () => ({width:600} as DOMRect);
    bubble.getBoundingClientRect = () => ({width:200,height:40} as DOMRect);
    const animate = vi.fn(() => ({cancel:vi.fn()}) as unknown as Animation);
    bubble.animate = animate; bubble.firstElementChild!.animate = animate;
    next!(16);
    expect(animate).toHaveBeenCalledTimes(2);
    stop();
});
