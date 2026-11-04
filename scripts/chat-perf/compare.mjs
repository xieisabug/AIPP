import { readFile } from 'node:fs/promises';

if (process.argv.length < 4) throw new Error('Usage: node scripts/chat-perf/compare.mjs result-a.json result-b.json [...]');
const groups = new Map();
for (const file of process.argv.slice(2)) {
    const runs = JSON.parse(await readFile(file, 'utf8'));
    if (!Array.isArray(runs)) throw new Error(`Expected an array of results: ${file}`);
    for (const run of runs) {
        if (!run.valid) { console.warn(`Excluded incomplete run: ${file} case ${run.fixtureId}`); continue; }
        const key = JSON.stringify([run.fixtureSha256, run.mode, run.expandedReasoning, run.seconds,
            run.environment.viewport, run.environment.devicePixelRatio, run.environment.reducedMotion, run.environment.userAgent,
            run.automation?.headed ?? 'manual', run.automation?.cache ?? 'unspecified']);
        const group = groups.get(key) || { run, values: [] };
        group.values.push(run.metrics); groups.set(key, group);
    }
}
function median(values) {
    const sorted = [...values].sort((a,b) => a-b);
    const middle = Math.floor(sorted.length/2);
    return sorted.length % 2 ? sorted[middle] : (sorted[middle-1]+sorted[middle])/2;
}
console.table([...groups.values()].map(({run,values}) => ({
    case: run.fixtureId, hash: run.fixtureSha256.slice(0,12), runs: values.length,
    viewport: run.environment.viewport.join('x'), dpr: run.environment.devicePixelRatio,
    reasoning: run.expandedReasoning, seconds: run.seconds,
    headed: run.automation?.headed ?? 'manual', cache: run.automation?.cache ?? 'unspecified',
    p95MedianMs: median(values.map(v => v.p95FrameMs)),
    worstMedianMs: median(values.map(v => v.worstFrameMs)),
    blankRatioMedian: median(values.map(v => v.maxBlankViewportRatio)),
    browser: run.environment.userAgent,
})));
console.log('Compare only identical fixture hashes, mode, viewport, DPR, reasoning and duration. Use the same Mac and display refresh rate. These are rAF estimates, not compositor frame traces.');
