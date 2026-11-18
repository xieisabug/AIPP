export interface MotionSample {
    ms: number; scroll: number; anchorTop: number | null; bubbleTop: number | null;
    phase: string; clientHeight?: number; opacity?: number | null; thumbPosition: number; thumbSize: number;
}
export function analyzeTrajectory(samples: MotionSample[], actionMs: number) {
    const baseline = samples.filter(frame => frame.ms <= actionMs).at(-1);
    const after = samples.filter(frame => frame.ms > actionMs);
    const scrollPhaseIndex = after.findIndex(frame => frame.phase === 'scroll');
    const entry = after.filter((frame, index) => frame.ms - actionMs < 125 || (frame.phase === 'entry' && (scrollPhaseIndex < 0 || index < scrollPhaseIndex)));
    const withBaseline = baseline ? [baseline, ...after] : after;
    const moving = after.filter(frame => frame.phase === 'scroll');
    const max = (values: number[]) => Math.max(0, ...values);
    const entryScrollDrift = baseline ? max(entry.map(frame => Math.abs(frame.scroll - baseline.scroll))) : null;
    const entryAnchorDrift = baseline?.anchorTop != null
        ? max(entry.filter(frame => frame.anchorTop != null).map(frame => Math.abs(frame.anchorTop! - baseline.anchorTop!))) : null;
    const thumbTop = (frame: MotionSample) => frame.thumbPosition * (1 - frame.thumbSize) * (frame.clientHeight ?? 0);
    const thumbSize = (frame: MotionSample) => frame.thumbSize * (frame.clientHeight ?? 0);
    const entryThumbDrift = baseline ? max(entry.map(frame => Math.abs(thumbTop(frame) - thumbTop(baseline)))) : null;
    const entryThumbSizeDrift = baseline ? max(entry.map(frame => Math.abs(thumbSize(frame) - thumbSize(baseline)))) : null;
    const firstMovingFrame = after.find(frame => baseline && Math.abs(frame.scroll - baseline.scroll) > 1);
    // A delayed rapid send can replace the bubble after the previous send already
    // moved. Evaluate fade order only while the current send owns the scroll phase.
    const bubbleBeforeScroll = moving.length
        ? !after.some((frame, index) => frame.phase === 'scroll' && frame.opacity != null && frame.opacity < .99
            && Math.abs(frame.scroll - (index ? after[index - 1].scroll : baseline?.scroll ?? frame.scroll)) > 1)
        : firstMovingFrame?.opacity == null || firstMovingFrame.opacity >= .99;
    const reverseAnchorPixels = max(moving.slice(1).map((frame, i) => frame.anchorTop == null || moving[i].anchorTop == null ? 0 : frame.anchorTop - moving[i].anchorTop!));
    const reversePixels = max(moving.slice(1).map((frame, i) => moving[i].scroll - frame.scroll));
    const firstScrollMs = after.find(frame => baseline && Math.abs(frame.scroll - baseline.scroll) > 1)?.ms;
    return { entryThumbDrift, entryThumbSizeDrift, bubbleBeforeScroll, baselineFrames: samples.length - after.length, entryScrollDrift, entryAnchorDrift,
        reversePixels, reverseAnchorPixels, firstScrollMs: firstScrollMs == null ? null : firstScrollMs - actionMs,
        maxThumbStep: max(withBaseline.slice(1).map((frame, i) => Math.abs(frame.thumbPosition - withBaseline[i].thumbPosition))),
        maxThumbSizeStep: max(withBaseline.slice(1).map((frame, i) => Math.abs(frame.thumbSize - withBaseline[i].thumbSize))),
        // Geometry checks, not a claim about subjective smoothness or frame scheduling.
        passed: !!baseline && entry.length > 0 && entryScrollDrift! <= 1
            && (entryAnchorDrift == null || entryAnchorDrift <= 1) && reversePixels <= 1 && reverseAnchorPixels <= 1
            && entryThumbDrift! <= 1 && entryThumbSizeDrift! <= 1 && bubbleBeforeScroll };
}
