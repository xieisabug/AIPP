# Chat browser performance benchmark

中文操作步骤见 [浏览器对照测试指南](../../docs/chat-browser-perf.md)。

This isolated entry reuses `VirtuosoMessageList`, `MessageItem`, Markdown, tool cards,
and the actual Preview Code runtime. It does not change the application entry or
enable browser mocks in production. All Tauri imports are replaced only in this
benchmark's Vite configuration. Node 22.16+ (including `node:sqlite`) is required.

## Export real conversations

Pause active generation before exporting. From the repository root:

```sh
node scripts/chat-perf/export.mjs
```

An optional first argument selects the database directory; an optional second
argument selects the output directory. The default is the current platform's AIPP
database directory, exported to `tmp/chat-perf/fixtures` (gitignored).
Both SQLite connections use read-only mode and explicit read transactions. No
provider settings, API keys, assistant configuration or session credentials are queried.
Chat text itself may contain sensitive material; these files are private, not sanitized.

The exporter selects the top two conversations for independent metrics including
Preview Code calls, scripted previews, tool count, visible message count and length,
raw length, largest message, code fences, tool-result volume, reasoning length,
image/table/math markers and failed tools. It also includes long chats (at least
20 visible messages). Marker counts are heuristics, not proof of rendered content.
Duplicates are combined, and a short baseline is added
when available. `ranking.json` includes all conversation statistics; `manifest.json`
includes selection reasons and SHA-256 hashes. Each fixture retains complete raw
messages, tool parameters/results, and attachment metadata without truncation.
The two databases have separate snapshot timestamps; do not export during writes.

## Build and open on a Mac

For the already-built portable archive `tmp/chat-perf/aipp-chat-perf.zip`, extract
it on the Mac, enter the extracted directory containing `index.html`, and run:

```sh
node serve.mjs .
```

Then open `http://127.0.0.1:4179` in Safari and Chrome. No database, Rust or npm
installation is needed for this portable route (Node itself must be installed).
Change the port with `node serve.mjs . 4180`. The archive contains private chats.

Copy the same exported `tmp/chat-perf/fixtures` directory to the Mac checkout.
Keep both checkouts at the same source revision and dependency lockfile.

```sh
npx tsc -p scripts/chat-perf/tsconfig.json --noEmit
npx vite build --config scripts/chat-perf/vite.config.ts
npx vite preview --config scripts/chat-perf/vite.config.ts
```

Open `http://127.0.0.1:4179` in Safari and Chrome. Use `--port 4180` if the port
is occupied. The preview server binds only to loopback and serves a production
build, not Vite development/HMR. Its output, including private fixture data, is
under `tmp/chat-perf/site`; never publish that directory.

Select a case and run the scroll probe. The seconds field is the duration of each
leg (down and up), so the total scroll duration is twice that value. Keep window size, zoom, display refresh
rate, power mode, reasoning expansion and duration equal. Keep the tab visible
and DevTools closed while recording. First run after page reload is a first-scroll
sample (not a fully cold browser/OS cache); record it separately. For warmed comparison, reload both browsers, run
once as warm-up, then record at least three runs with the same settings. Download
the JSON results using the download button. Remount does not clear module caches;
reload the page for a fresh cache. Do not interact with cards during timed runs.

```sh
node scripts/chat-perf/compare.mjs safari.json chrome.json
```

The comparison groups by content hash, settings and browser, excludes incomplete
runs, and reports medians. Match groups manually on the same machine. `p95FrameMs`
and dropped-frame counts derive from requestAnimationFrame at a 60 Hz reference
budget, not GPU/compositor traces; compare durations and blankness as well.

## Fidelity and limitations

- This is an offline historical-message rendering benchmark, not a complete
  application or end-to-end backend benchmark. No AI calls or tools are executed.
- Rust syntect highlighting is replaced with Highlight.js for every browser.
  Thus CPU/highlighting timing is not directly comparable with the installed app.
- Default display configuration is used. Plugins, native agent activity
  reconstruction, older `parent_id` regeneration reconstruction, app sidebars,
  input processing and live streaming are not replayed. Generation-group version
  filtering and built-in assistant-message merging are retained.
- Preview Code uses the original runtime and original collapse/activation rules.
  A stored preview count does not guarantee every preview is simultaneously mounted
  or every historical script executes. Expand cards manually to inspect them before
  running a separately documented expanded scenario.
  Results also record observed mounted Preview Code host instances; remounts can
  count twice, so this is coverage evidence rather than a unique preview count.
- External preview-resource preparation requires Rust and is deliberately rejected,
  not replaced with empty successful responses. Such calls and preview errors mark
  results incomplete. External attachment files are not copied; remote images can
  still use their original URLs, so cache/network effects require separate control.
- Unsupported backend commands are rejected and included in the result. Mutating
  tool actions have no real backend. Never use these results to infer live tool or
  IPC throughput.
- Safari is a useful WebKit comparison but is not WKWebView. A Chrome/Safari gap
  warrants an Electron/WKWebView comparison; it does not prove Electron fixes AIPP.

## Automation

Install the pinned automation library if it is not already available:

```sh
npm install --prefix scripts/chat-perf --ignore-scripts --package-lock=false
```

This installs the JavaScript library only. Do not run `playwright install`: the
runner exclusively uses existing Chrome/Edge installations. Historical fixed-width
preview content can remain clipped on narrow viewports, as in the original components.
Run `node scripts/chat-perf/verify.mjs` against the running site to collect smoke
results and screenshots in `tmp/chat-perf/results`. It requires Playwright and an
installed Edge browser; `CHAT_PERF_CHANNEL=chrome` can select Chrome instead.
`CHAT_PERF_URL` overrides the server URL.

Use installed browser channels in visible windows without any browser downloads:

```sh
node scripts/chat-perf/verify.mjs --headed --channel=chrome
node scripts/chat-perf/verify.mjs --headed --channel=msedge
```

Run these serially. Keep the tested window visible and avoid interacting while it
scrolls. `--repeats=3` records first-scroll and subsequent warm samples separately;
`--cases=123,456` limits cases to IDs in your local manifest, and `--seconds=15` sets each scroll leg's duration.
Each execution creates a timestamped result directory including the actual
installed browser version. An absent browser produces an error, never a download.
Browser profile data is temporary; personal browser tabs and profiles are not used.

## Send-motion regression entry

The same production build also emits `send-motion.html`. It uses synthetic messages
with the real `VirtuosoMessageList`, `MessageItem`, and `useScrollManagement` viewport
sizing. It does not call a model. Open:

- `http://127.0.0.1:4179/send-motion.html?history=0` (first send)
- `http://127.0.0.1:4179/send-motion.html?history=8` (retained history)
- `http://127.0.0.1:4179/send-motion.html?history=120` (virtualized history)

“发送” appends a user message and Butler processing row in one commit. “回复” replaces
the processing row; click immediately or after the slide. Send twice rapidly, scroll
up or touch-drag during motion, switch conversations, resize the window, and repeat
with the OS reduced-motion preference. The newest short turn should settle about
8px from the scroll viewport top, with blank room below. Record frames in browser
performance tools, checking bubble opacity/transform and row bounds, not just
`scrollTop` (the slide intentionally uses a compensating transform).

This entry is a frontend reproduction, not an end-to-end Tauri validation. The
automated matrix below supplies controlled layout/timing changes. Real network
images, streaming responses, native WebView behavior and compositor smoothness
still need application validation on the target platform.

### Automated browser matrix

“运行覆盖矩阵” runs 14 scenarios with real DOM layout and Web Animations:
empty/short/120-row history, rapid sends, immediate/delayed replies, synthetic wheel
and touch interruption, delayed image insertion/decoding, container resize, switching,
async history, history deletion, and emulated reduced-motion. “下载结果” exports every
rAF sample (time, row top, scroll position, height, opacity), animation call count,
functional assertions, p95/max frame intervals, and browser metadata. Keep the tab
visible and avoid input during the run. The scroller uses production overflow-anchor:none.

Pass/fail asserts position and animation ownership, **not** a frame-time budget.
The wheel/touch events, history/model timing and media-query override are simulated;
image loading uses a local data URL after a delay, not a remote network transfer.
Resize changes the actual container's height, not the native window manager. These
checks supplement manual device interaction and do not replace native WebView,
physical touch, OS reduced-motion, network-delayed images or compositor profiling.


### Long-frame and delayed HTTP image diagnostics

With the Vite **development server**, append `&networkImage=1` to the URL above.
The late-image scenario then requests a no-store SVG over HTTP; the server delays
its response by 650ms. The assertion requires a successfully decoded image and
checks the final message position. This endpoint is not included in static builds.
Without that query parameter, the existing delayed data-URL case remains available.

Downloads also include `historyCommitMs` (the synchronous async-history update),
Long Tasks when supported, and `imageLoaded`. Long Task times are relative to the
scenario start; a slightly negative start can describe the enclosing timer task.
rAF gaps exclude work before the first sample, so inspect first-sample latency and
Long Tasks as well. These development-mode measurements include instrumentation
and React/layout work and cannot identify a responsible function by themselves.

On 2026-11-18, the prior 171.6ms async-history gap repeated at 130.2ms and 128.4ms
after warmup, with the same height correction (14794 to 15178) and no send animation.
Two instrumented runs measured 173.7ms / 123.7ms gaps, history commits of 143.7ms /
36.1ms, and subsequent long tasks of 148ms / 98ms. All four runs passed 14/14
functional assertions; both HTTP image runs decoded successfully and settled at
8px. The history-layout stall is reproducible here, but its exact attribution and
production/WebView impact remain unresolved. Do not infer that warmup fixed it or
that the send animation caused it. Use a production CPU/compositor trace before
choosing an optimization.

F11, maximize and boundary-drag attempts did not yield a confirmed native viewport
size change in this desktop; Xfce Accessibility exposed no reduced-motion setting.
Those platform checks remain unverified; container resizing and matchMedia override
coverage must not be reported as native OS validation.
