# Butler bubble entry validation

Base: main `d7c9a59`. Butler-only opt-in passed from ConversationUI; ordinary Chat retains 140 ms entry and its existing animation. Butler uses a 300 ms shell contraction with a small damped overshoot, then 100 ms content reveal, followed by the existing native-scroll sequence. The shell starts at row width and H + min(0.5 H, 100px); actual layout geometry is unchanged. Row overflow is isolated during expansion to avoid adding transform overflow to the scroll range.

## Automated checks

- 96 tests passed across bubble geometry/cancellation/reduced motion, async history and conversation changes, streaming reply identity, merged-segment identity, virtual row late mount, existing MessageItem/useMessageListElements and send/list regression tests.
- `npm run build` passed (TypeScript and production build); existing large chunk warnings remain.
- No Rust code changed; `cargo` is unavailable in this cloud environment.

## Real browser, synthetic conversations

Used the cloud Chromium with a Vite **production build** of the repository's real list, MessageItem and MergedAssistantGroup components. Backend/model events are synthetic. Server is started in the desktop terminal because the command runner and desktop have separate networking.

Reproduce: build `node_modules/.bin/vite build --config scripts/chat-perf/vite.config.ts`, serve with `vite preview` using that config and port 4179, open `/send-motion.html?history=8&butlerMotion&merged`, then run the coverage matrix. Omit `merged` for individual reply bubbles; omit `butlerMotion` for the ordinary Chat path.

- Ordinary Chat component path: 19/19 scenarios passed. Short send used one animation call, first measured scroll at 195.5ms and final top 8px; entry anchor/scroll/thumb drift 0px. This checks the shared component path, not a live ordinary Chat backend session.
- Final merged mode: 19/19 scenarios passed, including empty/short/120-message histories, rapid entry and scroll-phase sends, immediate/delayed/streaming replies, input collapse, late image, resize, session switch, async history, history deletion, emulated reduced motion and synthetic wheel/touch interruption.
- Short/long/streaming-reply entries: old-anchor, scrollTop and native thumb entry drift all 0px; final user top 8px. Sampled shell width spanned approximately 198–1117px (normal width about 203px; slight undershoot is intentional).
- Short: content fully opaque at 411.8ms, first measured scroll at 445.1ms. Long: 403.1ms then 454.6ms. Streaming reply case: 409.8ms then 460.7ms. These are browser samples, not fixed timing guarantees.
- No PerformanceObserver long tasks in this final merged run. Largest sampling gap 64.4ms occurred in session switch; this is not evidence of perfectly paced frames. Other gaps include input collapse 63.2ms and delayed reply 51.4ms.
- Browser testing caught a real late-mount issue in long history: the initial implementation fell back to the old animation. The fix waits for the virtual user bubble and for visible content before scrolling; final long-history animation calls match short history.
- A manual browser send and reply showed the empty AI shell before its text and final user placement at the viewport top.

The accompanying JSON contains per-scenario summaries. Full frame data remains under `tmp/chat-perf/butler-entry/merged-browser.json` in the implementation worktree.

## Limits

This is real Chromium rendering of product components, not the packaged Tauri application or Windows WebView2. No live model/network backend conversation was run. Touch and reduced-motion cases are explicitly emulated; native touch hardware, OS setting changes, IME composition, and exhaustive rich-content/late-image decoding combinations are not covered by this run. Existing first-new-conversation backend loading semantics are unchanged.
