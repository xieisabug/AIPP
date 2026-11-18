# Send performance follow-up (2026-11-18)

The Chromium DevTools keyboard shortcut was tried on the local preview. The browser
showed an organization-policy dialog prohibiting developer tools on this page.
No policy was changed and no alternate debugging connection was used. Instead the
fixture now records the standard PerformanceObserver long-animation-frame entries,
including scripts and forced style/layout durations, alongside longtask entries.
These public page metrics are not a CPU sampling stack or DevTools trace.

Ten delayed-image repeats passed with no longtask entries. Maximum sample gaps were
42.2, 36.1, 63.4, 65.2, 25.0, 36.8, 22.6, 21.5, 27.1, 41.7ms. Two long animation
frames (63.0/64.7ms) had zero blocking duration and empty script attribution.

Ten rapid-long repeats passed. One 58ms longtask began at send time; the enclosing
81.1ms animation frame attributed 58.5ms to TimerHandler:setTimeout with 10.2ms
forced style/layout. Function name/URL/position were unavailable. Another 106.3ms
frame attributed only 11.5ms to a timer, with 1.1ms forced layout and zero blocking.
This does not establish a reproducible 204ms product-code defect. The fixture adds
explicit synchronous send-flushSync and geometry-sampling timing to distinguish
its own work from browser scheduling and the real application path.

## Environment verification and attempted alternatives

The selected environment and desktop inventory expose Linux/Xfce and Chromium only.
Command checks found no Wine, QEMU, libvirt, VirtualBox, adb, Cargo, ibus, fcitx or
fcitx5. There is no /dev/input in the execution environment. The native Mouse and
Touchpad dialog had no selected device, with device/touchpad/tablet controls disabled;
the browser reports maxTouchPoints=0. Physical touch and a Windows WebView2 host
are not available through these surfaces. Existing synthetic composition/touch
regressions remain explicitly synthetic; Chinese text fill is not IME composition.

The earlier lack of a visible reduced-motion switch was investigated further.
GTK's toolkit animation setting is documented by GTK, and Chromium documents its
Linux reduced-motion propagation from that setting:
- https://docs.gtk.org/gtk3/property.Settings.gtk-enable-animations.html
- https://chromium.googlesource.com/chromium/src/+/fd66f1594ac32e3b8b7c514bcd7c3e6916564591

Using the desktop terminal, /Gtk/EnableAnimations in Xfce xsettings was first checked
(absent), temporarily created as false, and read back as false. The production
fixture's new reduced-motion-native scenario does not override matchMedia and
requires the real preference to be true. All three attempts correctly failed this
requirement: nativeReducedMotion stayed false, one animation ran, final top stayed
8px. This is evidence the setting did not propagate in this desktop, not a passing
system-preference test and not evidence that the app ignored a true preference.
The temporary property was removed and its absence was verified afterward.

No product animation change is justified solely by these non-reproducing samples.
The original 204ms task remains unattributed; the previously observed catch-up can
be visible, so final-position success is not treated as proof of perfect smoothness.

## Final instrumented repeat

The final ten rapid-long cases passed with no longtask or long-animation-frame
entries. Maximum sample gaps ranged 23.4–37.0ms. Synchronous send flushes ranged
10.4–17.1ms; the maximum geometry sampler cost was 2.4ms. These measurements do
not reproduce a 204ms task or establish a code fix. Across this follow-up, 30
normal-motion scenarios passed; the three native-reduced capability checks failed
as explicitly described above. Summaries are in send-sequence-profiling-evidence.json.

Validation: 38 focused tests passed (InputArea, sequence and trajectory), including
synthetic IME composition, touch interruption and reduced-motion regressions.
Final production harness build and full TypeScript/production build passed.
