# PR 95 supplemental acceptance — 2026-11-18

No product animation code changed in this follow-up. Added optional `composer=1`
to the production harness using the real InputArea and a 650ms delayed HTTP image
endpoint in the static server. Start serve.mjs on port 4182 and open
`/send-motion.html?scenario=late-image&repeat=6&networkImage=1`.

## Completed

- Captured the Linux desktop Chromium production preview through FFmpeg X11 at a
  requested 60fps. Before: main 7b5b74e with the same fixture; after: 8776f65.
  Three short-history sends per side. A 6.3s side-by-side 1x video and 25.18s 0.25x
  video are included in the downloadable evidence package. Slowdown repeats frames;
  no generated/interpolated animation. Cropped page area contains synthetic data.
  Clips are approximately aligned by the first send, not frame-perfect synchronized.
  Later repetitions include history reset/diagnostic rendering, not continuous sends.
  The full 90s recording is retained locally; its capture dropped 22 frames, so this
  is a visual review artifact, not a compositor or exact frame-time benchmark.
- Original delayed data-URL image scenario repeated six times: 6/6 passed;
  max sampling intervals 43.3, 90.6, 113.1, 37.2, 66.2, 30.5ms. The fifth run had
  a 58ms long task near send; the others reported none. Screen capture was active.
- Real loopback HTTP image responses delayed 650ms: 6/6 passed, actual image
  `complete` and `naturalWidth` checked; max intervals 33.8, 38.6, 23.3, 26.9,
  22.3, 28.2ms; no reported long tasks. This exercises HTTP fetch/decode/layout,
  not mobile radio/WAN latency or a provider's image format mix.
- Real InputArea in production preview: Chinese five-line text entered through
  browser fill, Enter sent once and cleared the field, final user top 8px. With
  120 history messages, two successive Enter sends also ended at 8px. This covers
  the actual component with stubbed backend, not the full ConversationUI shell.
- InputArea and trajectory tests: 27/27 passed, including existing synthetic IME
  composition/Enter debounce regression. Synthetic composition is not real IME.

## 221.7ms interval analysis

The old result has a 204ms long task at sample-relative 386.5ms. Send action was
82ms; the long task began 304.5ms after send, before the fixture's image insertion
scheduled 650ms after send. It is therefore not evidence of image decode cost.
The interval spans samples at 371.5 and 593.2ms during `scroll`; bubble top went
252 -> 225px, then to 8px at 614ms. A main-thread pause followed by catching up
can be visibly abrupt even though the final geometry and monotonicity pass.
The six repeat runs did not reproduce that 204ms task. No CPU stack attribution
was captured, so its root cause remains unresolved. Do not claim it was fixed,
caused by cloud noise, or caused by product code based on these measurements.

## Capability checks and remaining gaps

- Available environment is Linux/Xfce with Chromium; no Windows environment or
  WebView2 host exposed. Cargo and Wine are absent. Native Windows acceptance
  cannot be obtained by relabeling Chromium or installing a Linux dependency.
- ibus/fcitx5 executables absent; no configured IME shown in available apps.
  Real candidate selection was not performed; text fill does not exercise IME.
  Installing/configuring an IME and restarting a desktop session is additional
  environment setup, not an already available device test.
- Native automation exposes mouse/keyboard but no physical touch device or touch
  injection API. Existing synthetic touch release tests remain the coverage.
- Opened Xfce Accessibility and Settings Editor -> xsettings. Accessibility lists
  assistive technologies/keyboard/mouse settings; xsettings contains no animation
  toggle. No system preference was changed. Existing matchMedia emulation remains
  distinct from a system-preference test.
- An Alt+F10 desktop resize attempt retained final 8px positioning, but no distinct
  measured viewport dimensions were captured, so no new window-size coverage is
  claimed beyond the earlier geometry simulations.

No changes to user data, no merge. Videos show the shared-list preview, not Windows
WebView2 subjective acceptance. Timing results do not guarantee all frames fit 16.7ms.

Full TypeScript + production build and the expanded production harness build passed.

[Download video and raw evidence package](https://chatgpt.com/api/library/files/libfile_fde3_f-d6829f21b71881918b9d77dfbe2a34c2_17ceaecb2adc81918e57e382aad5b3e4/download) (owner Library access required).
