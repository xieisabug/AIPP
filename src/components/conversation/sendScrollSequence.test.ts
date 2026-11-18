import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { startSendScrollSequence } from './sendScrollSequence';

function setup(butlerBubbleMotion = false) {
    const container = document.createElement('div');
    const row = document.createElement('div');
    row.dataset.messageItem = ''; row.dataset.messageId = '3';
    row.appendChild(document.createElement('div')); container.appendChild(row);
    document.body.appendChild(container);
    let rowTop = 600;
    Object.defineProperties(container, {scrollHeight: {get:()=>1200}, clientHeight: {get:()=>400}});
    container.getBoundingClientRect = () => ({top:0} as DOMRect);
    row.getBoundingClientRect = () => ({top:rowTop-container.scrollTop} as DOMRect);
    const complete = vi.fn(); const release = vi.fn();
    const stop = startSendScrollSequence({container,messageId:3,butlerBubbleMotion,onComplete:complete,onUserRelease:release});
    return {container, complete, release, stop, moveRow:(top:number)=>{rowTop=top;}};
}
beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal('requestAnimationFrame', (cb:FrameRequestCallback)=>setTimeout(()=>cb(performance.now()),16));
    vi.stubGlobal('cancelAnimationFrame', clearTimeout);
    vi.stubGlobal('matchMedia', ()=>({matches:false}));
});
afterEach(()=>{vi.useRealTimers();vi.unstubAllGlobals();document.body.innerHTML='';});
describe('send entry then native scroll',()=>{
    it('keeps the scrollbar still during entry then traverses intermediate positions',()=>{
        const {container,complete}=setup();
        vi.advanceTimersByTime(140); expect(container.scrollTop).toBe(0);
        expect(container.dataset.sendPhase).toBe('entry');
        vi.advanceTimersByTime(100); expect(container.scrollTop).toBeGreaterThan(0);expect(container.scrollTop).toBeLessThan(592);
        vi.advanceTimersByTime(500);expect(container.scrollTop).toBe(592);expect(complete).toHaveBeenCalledOnce();
        expect(container.style.scrollBehavior).toBe('');
    });
    it.each(['wheel','touchmove','touchstart','pointerdown'])('does not resume after user %s interruption',event=>{
        const {container,release,complete}=setup();vi.advanceTimersByTime(240);
        container.dispatchEvent(new Event(event));const top=container.scrollTop;
        vi.advanceTimersByTime(2000);expect(container.scrollTop).toBe(top);expect(release).toHaveBeenCalledOnce();expect(complete).not.toHaveBeenCalled();
    });
    it('retargets a late height change without snapping in that frame',()=>{
        const {container,moveRow}=setup();vi.advanceTimersByTime(240);const top=container.scrollTop;
        moveRow(750);vi.advanceTimersByTime(16);expect(container.scrollTop).toBe(top);
        vi.advanceTimersByTime(600);expect(container.scrollTop).toBe(742);
    });
    it('cancels without rewriting the current scroll position',()=>{
        const {container,stop,complete}=setup();vi.advanceTimersByTime(240);const top=container.scrollTop;
        stop();vi.advanceTimersByTime(2000);expect(container.scrollTop).toBe(top);expect(complete).not.toHaveBeenCalled();
    });
    it('skips both timed phases for reduced motion',()=>{
        vi.stubGlobal('matchMedia',()=>({matches:true}));const {container,complete}=setup();
        vi.advanceTimersByTime(16);expect(container.scrollTop).toBe(592);expect(complete).toHaveBeenCalledOnce();
    });
});

describe('reserved space', () => {
    it('holds pre-send height, grows with scrolling, and restores styles on cancellation', () => {
        const container = document.createElement('div');
        const content = document.createElement('div');
        const child = document.createElement('div');
        const row = document.createElement('div');
        row.dataset.messageItem = ''; row.dataset.messageId = '3';
        row.appendChild(document.createElement('div'));
        child.appendChild(row); content.appendChild(child); container.appendChild(content);
        document.body.appendChild(container);
        // happy-dom does not parse nested CSS max() in calc(); browser acceptance
        // covers computed geometry. This test observes writes and cleanup.
        let writtenHeight = '';
        Object.defineProperty(content.style, 'height', {get:()=>writtenHeight,set:(value:string)=>{writtenHeight=value;}});
        const renderedHeight = () => Number.parseFloat(content.style.height.replace('calc(', '')) || 1000;
        content.getBoundingClientRect = () => ({height:renderedHeight()} as DOMRect);
        child.getBoundingClientRect = () => ({height:1000} as DOMRect);
        container.getBoundingClientRect = () => ({top:0} as DOMRect);
        row.getBoundingClientRect = () => ({top:608-container.scrollTop} as DOMRect);
        Object.defineProperties(container,{scrollHeight:{get:renderedHeight},clientHeight:{value:400}});
        container.scrollTop = 300;
        const cancel = startSendScrollSequence({container,content,previousLayout:{height:700,scrollHeight:700,scrollTop:300,viewportHeight:400},
            messageId:3,onComplete:vi.fn(),onUserRelease:vi.fn()});
        vi.advanceTimersByTime(140);
        expect(renderedHeight()).toBe(700); expect(container.scrollTop).toBe(300);
        vi.advanceTimersByTime(100);
        expect(renderedHeight()).toBeGreaterThan(700); expect(renderedHeight()).toBeLessThan(1000);
        const top = container.scrollTop; cancel();
        expect(container.scrollTop).toBe(top);
        expect(content.style.height).toBe(''); expect(content.style.overflow).toBe(''); expect(content.style.flexShrink).toBe('');
    });
});

it('keeps an old visual anchor steady through a virtual measurement correction', () => {
    const container = document.createElement('div');
    const content = document.createElement('div');
    const inner = document.createElement('div');
    const old = document.createElement('div');
    const row = document.createElement('div');
    old.dataset.messageItem = ''; old.dataset.messageId = '1';
    row.dataset.messageItem = ''; row.dataset.messageId = '3'; row.appendChild(document.createElement('div'));
    inner.append(old, row); content.append(inner); container.append(content); document.body.append(container);
    Object.defineProperties(container,{scrollHeight:{value:1000},clientHeight:{value:400}});
    container.scrollTop = 300;
    const offset = () => Number.parseFloat((old.style.translate ?? '').split(' ')[1]) || 0;
    content.getBoundingClientRect = inner.getBoundingClientRect = () => ({height:1000} as DOMRect);
    container.getBoundingClientRect = () => ({top:0} as DOMRect);
    old.getBoundingClientRect = () => ({top:408-container.scrollTop+offset()} as DOMRect);
    row.getBoundingClientRect = () => ({top:608-container.scrollTop+offset()} as DOMRect);
    const cancel = startSendScrollSequence({container,content,previousLayout:{
        height:1000,scrollTop:300,scrollHeight:1000,viewportHeight:400,anchorId:'1',anchorTop:100,
    },messageId:3,onComplete:vi.fn(),onUserRelease:vi.fn()});
    vi.advanceTimersByTime(140);
    expect(container.scrollTop).toBe(300); expect(old.getBoundingClientRect().top).toBe(100);
    vi.advanceTimersByTime(100);
    const visual = old.getBoundingClientRect().top;
    cancel();
    expect(old.getBoundingClientRect().top).toBeCloseTo(visual);
    expect(old.style.translate).toBe('');
});

it('should wait for the actual bubble fade when its animation starts late', () => {
    const {container,complete} = setup();
    const bubble = container.querySelector('[data-message-item]')!.lastElementChild as HTMLElement;
    bubble.style.opacity = '0.5';
    vi.advanceTimersByTime(300);
    expect(container.scrollTop).toBe(0);
    expect(container.dataset.sendPhase).toBe('entry');
    bubble.style.opacity = '1';
    vi.advanceTimersByTime(600);
    expect(container.scrollTop).toBe(592);
    expect(complete).toHaveBeenCalledOnce();
});

it('should wait for the Butler shell and text before scrolling',()=>{
    const {container,complete}=setup(true);
    vi.advanceTimersByTime(399); expect(container.scrollTop).toBe(0);
    vi.advanceTimersByTime(120); expect(container.scrollTop).toBeGreaterThan(0);
    vi.advanceTimersByTime(600); expect(container.scrollTop).toBe(592); expect(complete).toHaveBeenCalledOnce();
});
