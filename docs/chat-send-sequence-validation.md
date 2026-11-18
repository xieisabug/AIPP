# Send entry / scroll sequence validation

Base: main 7b5b74e (merged #94). This follow-up changes the shared message list:
140ms bubble entry, then a 260ms cubic native-scroll motion. The list's reserved
height grows with the scroll; the native scroll position is not teleported to the
destination. A React pre-mutation snapshot preserves the old geometry. Late
virtual measurements are corrected on outer message rows during entry, without
transforming Virtuoso's root. Wheel, touch contact, pointer and scroll keys release
ownership. Reduced motion skips timed animation.

## Reproduction

Build the same harness against main and this branch:

    node_modules/.bin/vite build --config scripts/chat-perf/vite.config.ts
    node scripts/chat-perf/serve.mjs tmp/chat-perf/site 4180

Open /send-motion.html?scenario=short&repeat=3, or omit scenario for the matrix.
Press “运行覆盖矩阵”; “下载结果” exports all samples. The fault=jump and fault=reverse
parameters inject an 80px visible content displacement at different phases.

Copy scripts/chat-perf/send-motion.tsx and send-trajectory.ts to an isolated main
checkout before its build. Use the same browser and query parameters. In this cloud
environment the static servers were launched in the desktop terminal because its
network namespace differs from command execution.

## Evidence and interpretation

[Recorded summaries](./send-sequence-evidence.json) contain production Chromium A/B
and negative controls. Each case records at least five pre-send frames, the old
visible outer message, the new row and actual bubble rect/opacity, native scrollTop,
scrollHeight/clientHeight, and the animation phase. Sampling is scheduled after
rAF callbacks using a timer, to avoid measuring between two callbacks in one frame.
This is geometry instrumentation, not a compositor trace or guaranteed paint capture.

The three short-history main runs jumped native scrollTop by 280px during entry;
the scrollbar's derived thumb top/length changed by 31–42px. The three candidate
runs had zero entry scroll, old-anchor and thumb drift, and finished at 8px.
The injected jump and reverse cases failed even though final alignment was 8px.
The scrollbar metrics derive track geometry from the native scroll values; actual
platform thumb minimum sizes and overlays can differ.

The matrix includes empty/short/120-message histories, rapid sends before and during
scrolling, rapid long history, immediate/delayed replies, incremental reply content,
synthetic wheel/touch interruption, viewport contraction/expansion, delayed data-URL
image insertion, switching, async history, deletion and emulated reduced motion.
Explicit interruption/history/reduced-motion cases use their own functional
expectations, not the unmodified-send trajectory assertion. Viewport expansion
checks content continuity; a thumb-size change is expected when the track changes.
Rapid-during-scroll currently has final-position/cancellation coverage, but does
not assert a separate pre-send baseline for its second send.

A later strict full-entry check found a one-frame 8px virtual-layout correction on
rapid long history. The final controller observes entry-phase spacer updates before
paint and preserves the original history anchor across rapid entry sends. Nested
merged messages share IDs, so both snapshot and harness select the outer message
row; comparing a nested body with its outer bubble gives misleading measurements.

Do not interpret frame-time differences as a causal performance benchmark. These
runs share cloud resources and the harness renders diagnostics. One rapid-long run
concurrent with repository tests recorded a 3234ms maximum sample interval and
failed ordering; the two following warmed runs had no long tasks and passed.
That outlier is retained in the local worklog artifacts; its cause is not established.

## Limits

This is the production-built shared list with synthetic content and Tauri API stubs,
not the complete native application. Windows WebView2, real touch hardware, system
reduced-motion settings, the actual composer/IME resize path, real network image
loading, and native scrollbar drag timing have not been accepted here. Input
collapse is simulated by viewport growth, and streaming by incremental content
updates. No normal-speed/slow-motion video or full compositor/CPU trace was captured.
The checks establish measured geometry and behavior, not subjective native-app
smoothness or a universal frame-time guarantee.

## Final validation run

The final production matrix passed 38/38 scenarios across two sequential runs.
Short/long/rapid-long/streaming case p95 sampling intervals were 18.5–22.4ms;
the largest interval across the matrix was 221.7ms in delayed-image insertion.
These are sampling intervals, not GPU frame presentation times.
The final fixture caches serialized diagnostics rather than repeating serialization
on every send, and its SHA-256 matches the baseline fixture.

Repository validation: 37 test files / 359 tests passed with one worker; the full
TypeScript + production build passed. One unchanged ConversationList test had
previously timed out under parallel load and passed both in isolation and in the
serial suite. Cargo is not installed in this environment; no Rust changes were made.

A final Chromium UI check issued a trusted browser wheel action while the observed
phase was `scroll`; the animation released ownership, and a subsequent reply kept
the view in the earlier history. This is additional desktop-browser interaction
evidence, not real touch or Windows WebView2 coverage.
