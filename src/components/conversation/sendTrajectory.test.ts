import { describe, it, expect } from 'vitest';
import { analyzeTrajectory, type MotionSample } from '../../../scripts/chat-perf/send-trajectory';
const samples = (): MotionSample[] => Array.from({length: 30}, (_, i) => {
    const scroll = i < 14 ? 100 : 100 + (i - 14) * 8;
    return { ms: i * 16, scroll, anchorTop: 200 - scroll, bubbleTop: 600 - scroll,
        phase: i < 14 ? 'entry' : 'scroll', thumbPosition: scroll / 800, thumbSize: .3 };
});
describe('send trajectory assertions and injected negative controls', () => {
    it('accepts stationary entry followed by forward scrolling', () => {
        expect(analyzeTrajectory(samples(), 64).passed).toBe(true);
    });
    it('detects a pre-scroll native jump even if content compensates', () => {
        const frames = samples(); frames[6].scroll += 400;
        expect(analyzeTrajectory(frames, 64).entryScrollDrift).toBe(400);
        expect(analyzeTrajectory(frames, 64).passed).toBe(false);
    });
    it('detects an old-anchor flash without a scrollTop change', () => {
        const frames = samples(); frames[6].anchorTop! += 30;
        expect(analyzeTrajectory(frames, 64).entryAnchorDrift).toBe(30);
        expect(analyzeTrajectory(frames, 64).passed).toBe(false);
    });
    it('detects a scrollbar-size jump without any content movement', () => {
        const frames = samples().map(frame => ({...frame, clientHeight:500}));
        frames[6].thumbSize = .2;
        expect(analyzeTrajectory(frames, 64).entryThumbSizeDrift).toBe(50);
        expect(analyzeTrajectory(frames, 64).passed).toBe(false);
    });
    it('detects reversal despite correct final position', () => {
        const frames = samples(); frames[20].scroll -= 40;
        expect(analyzeTrajectory(frames, 64).reversePixels).toBe(32);
        expect(analyzeTrajectory(frames, 64).passed).toBe(false);
    });
});

it('rejects actual movement while the current bubble is still fading', () => {
    const frames = samples().map(frame => ({...frame, opacity:1}));
    frames[20].opacity = .5;
    expect(analyzeTrajectory(frames, 64).bubbleBeforeScroll).toBe(false);
});
it('does not attribute prior movement to a replacement bubble in entry', () => {
    const frames = samples().map(frame => ({...frame, opacity:1}));
    frames[20].opacity = .5; frames[20].phase = 'entry';
    expect(analyzeTrajectory(frames, 64).bubbleBeforeScroll).toBe(true);
});
