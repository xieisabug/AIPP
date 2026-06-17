import morphdom from "morphdom";
import { PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS } from "@/utils/previewCode";

export interface PreviewCodeBridge {
    submit: (payload?: unknown) => Promise<void>;
    close: () => Promise<void>;
    emitEvent: (name: string, payload?: unknown) => void;
}

interface PreviewCodeRuntimeUpdate {
    code: string;
    isFinal: boolean;
    bridgeId: string;
    bridge: PreviewCodeBridge;
    onError?: (message: string | null) => void;
}

export interface PreviewCodeRuntimeController {
    update: (next: PreviewCodeRuntimeUpdate) => void;
    destroy: () => void;
}

declare global {
    interface Window {
        aippPreviewCode?: PreviewCodeBridge;
        __AIPP_PREVIEW_CODE_BRIDGES__?: Record<string, PreviewCodeBridge>;
        __AIPP_PREVIEW_CODE_SCRIPT_LOADS__?: Record<string, Promise<void> | undefined>;
        __AIPP_PREVIEW_CODE_SCRIPT_TEXTS__?: Record<string, Promise<string> | undefined>;
    }
}

const PREVIEW_CODE_FRAME_FALLBACK_MS = 16;
const PREVIEW_CODE_ENTER_ATTRIBUTE = "data-aipp-preview-enter";
const PREVIEW_CODE_ENTER_DURATION_MS = 280;
const PREVIEW_CODE_MUTATION_OBSERVER_FLUSH_MS = 100;
const PREVIEW_CODE_MUTATION_OBSERVER_BUDGET_WINDOW_MS = 5000;
const PREVIEW_CODE_MUTATION_OBSERVER_MAX_CALLBACKS = 40;
const PREVIEW_CODE_RUNTIME_STYLES = `
:host {
    display: block;
    min-height: inherit;
    color: inherit;
}

.aipp-preview-code-shell {
    position: relative;
    min-height: inherit;
    overflow: clip;
}

.aipp-preview-code-root {
    min-height: inherit;
}

.aipp-preview-code-root :where([data-aipp-preview-enter="true"]) {
    animation: aipp-preview-code-enter 240ms cubic-bezier(0.22, 1, 0.36, 1);
    transform-origin: top center;
    will-change: opacity, transform, filter;
}

@keyframes aipp-preview-code-enter {
    from {
        opacity: 0;
        transform: translateY(10px) scale(0.985);
        filter: blur(8px);
    }
    to {
        opacity: 1;
        transform: translateY(0) scale(1);
        filter: blur(0);
    }
}

@media (prefers-reduced-motion: reduce) {
    .aipp-preview-code-root > :where(:not(style):not(script)) {
        animation: none;
    }
}
`;
type ScheduledApplyHandle =
    | { kind: "raf"; id: number }
    | { kind: "timeout"; id: number };

function scheduleImmediateApply(callback: () => void): ScheduledApplyHandle {
    if (typeof window.requestAnimationFrame === "function") {
        return {
            kind: "raf",
            id: window.requestAnimationFrame(() => callback()),
        };
    }
    return {
        kind: "timeout",
        id: window.setTimeout(callback, PREVIEW_CODE_FRAME_FALLBACK_MS),
    };
}

function scheduleApply(callback: () => void, delayMs: number): ScheduledApplyHandle {
    if (delayMs <= 0) {
        return scheduleImmediateApply(callback);
    }
    return {
        kind: "timeout",
        id: window.setTimeout(callback, delayMs),
    };
}

function cancelScheduledApply(handle: ScheduledApplyHandle | null) {
    if (!handle) {
        return;
    }
    if (handle.kind === "raf" && typeof window.cancelAnimationFrame === "function") {
        window.cancelAnimationFrame(handle.id);
        return;
    }
    window.clearTimeout(handle.id);
}

function getBridgeRegistry(): Record<string, PreviewCodeBridge> {
    if (!window.__AIPP_PREVIEW_CODE_BRIDGES__) {
        window.__AIPP_PREVIEW_CODE_BRIDGES__ = {};
    }
    return window.__AIPP_PREVIEW_CODE_BRIDGES__;
}

function getScriptLoadRegistry(): Record<string, Promise<void> | undefined> {
    if (!window.__AIPP_PREVIEW_CODE_SCRIPT_LOADS__) {
        window.__AIPP_PREVIEW_CODE_SCRIPT_LOADS__ = {};
    }
    return window.__AIPP_PREVIEW_CODE_SCRIPT_LOADS__;
}

function getScriptTextRegistry(): Record<string, Promise<string> | undefined> {
    if (!window.__AIPP_PREVIEW_CODE_SCRIPT_TEXTS__) {
        window.__AIPP_PREVIEW_CODE_SCRIPT_TEXTS__ = {};
    }
    return window.__AIPP_PREVIEW_CODE_SCRIPT_TEXTS__;
}

function getLiveBridgeOrThrow(bridgeId: string): PreviewCodeBridge {
    const bridge = getBridgeRegistry()[bridgeId];
    if (!bridge) {
        throw new Error(`preview_code bridge is unavailable: ${bridgeId}`);
    }
    return bridge;
}

function rewriteStyleSelectors(styleContent: string): string {
    return styleContent
        .replace(/(^|[,{]\s*)(:root|html|body)(?=(\s|,|[{>+~.#:\[]|$))/gm, "$1:host")
        .replace(/(:host\s+)(html|body)(?=(\s|,|[{>+~.#:\[]|$))/gm, "$1:host");
}

function normalizeStyleNodes(root: ParentNode) {
    const styles = "querySelectorAll" in root ? root.querySelectorAll("style") : [];
    styles.forEach((style) => {
        style.textContent = rewriteStyleSelectors(style.textContent ?? "");
    });
}

function collectScriptNodes(root: ParentNode) {
    return Array.from(root.querySelectorAll("script")).map((script) => {
        const src = script.getAttribute("src");
        const type = script.getAttribute("type");
        const content = script.textContent ?? "";
        if (src) {
            script.removeAttribute("src");
            script.setAttribute("data-aipp-preview-script-src", src);
            if (type) {
                script.setAttribute("data-aipp-preview-script-type", type);
            }
        }
        return { src, type, content };
    });
}

function buildTargetMountPoint(code: string): {
    mountPoint: HTMLDivElement;
    scripts: Array<{ src: string | null; type: string | null; content: string }>;
} {
    const template = document.createElement("template");
    template.innerHTML = code;
    normalizeStyleNodes(template.content);
    const scripts = collectScriptNodes(template.content);
    const mountPoint = document.createElement("div");
    mountPoint.className = "aipp-preview-code-root";
    mountPoint.append(template.content.cloneNode(true));
    return { mountPoint, scripts };
}

function getMorphNodeKey(node: Node): string | undefined {
    if (!(node instanceof Element)) {
        return undefined;
    }
    return node.getAttribute("data-aipp-key") ?? node.id ?? undefined;
}

function markNodeForEntranceAnimation(node: Node) {
    if (!(node instanceof Element) || node.tagName === "STYLE" || node.tagName === "SCRIPT") {
        return;
    }
    node.setAttribute(PREVIEW_CODE_ENTER_ATTRIBUTE, "true");
    const clear = () => node.removeAttribute(PREVIEW_CODE_ENTER_ATTRIBUTE);
    node.addEventListener("animationend", clear, { once: true });
    window.setTimeout(clear, PREVIEW_CODE_ENTER_DURATION_MS);
}

function patchMountPoint(mountPoint: HTMLDivElement, nextMountPoint: HTMLDivElement) {
    morphdom(mountPoint, nextMountPoint, {
        childrenOnly: true,
        getNodeKey: getMorphNodeKey,
        onBeforeElUpdated(fromEl, toEl) {
            if (fromEl.isEqualNode(toEl)) {
                return false;
            }
            return true;
        },
        onNodeAdded(node) {
            markNodeForEntranceAnimation(node);
        },
    });
}

function normalizeExternalScriptUrl(src: string): string {
    const url = new URL(src, window.location.href);
    if (url.protocol !== "aipp-preview:") {
        throw new Error(`preview_code 不允许直接加载外部脚本: ${url.origin}`);
    }
    return url.toString();
}

async function fetchExternalScriptText(src: string) {
    const registry = getScriptTextRegistry();
    if (!registry[src]) {
        registry[src] = fetch(src)
            .then((response) => {
                if (!response.ok) {
                    throw new Error(`preview_code 获取外部脚本失败: ${src}`);
                }
                return response.text();
            })
            .catch((error) => {
                delete registry[src];
                throw error;
            });
    }
    return registry[src];
}

const previewScopedExternalScriptRuns = new WeakMap<ShadowRoot, Set<string>>();

function isClassicJavaScriptType(type: string | null) {
    if (!type) {
        return true;
    }
    const normalized = type.trim().toLowerCase();
    return normalized === ""
        || normalized === "text/javascript"
        || normalized === "application/javascript"
        || normalized === "application/ecmascript"
        || normalized === "text/ecmascript";
}

function runScriptContent(
    host: HTMLElement,
    shadowRoot: ShadowRoot,
    environment: PreviewScriptEnvironment,
    scriptContent: string
) {
    const scopedScriptContent = `with(window){
${appendWindowExports(scriptContent)}
}`;
    const scriptRunner = new Function(
        "window",
        "document",
        "globalThis",
        "self",
        "host",
        "shadowRoot",
        "aippPreviewCode",
        "MutationObserver",
        scopedScriptContent
    );
    scriptRunner.call(
        environment.previewWindow,
        environment.previewWindow,
        environment.previewDocument,
        environment.previewWindow,
        environment.previewWindow,
        host,
        shadowRoot,
        environment.previewWindow.aippPreviewCode,
        environment.previewWindow.MutationObserver
    );
}

async function runPreviewScopedExternalScript(
    src: string,
    host: HTMLElement,
    shadowRoot: ShadowRoot,
    environment: PreviewScriptEnvironment
) {
    let runs = previewScopedExternalScriptRuns.get(shadowRoot);
    if (!runs) {
        runs = new Set<string>();
        previewScopedExternalScriptRuns.set(shadowRoot, runs);
    }
    if (runs.has(src)) {
        return;
    }
    const scriptContent = await fetchExternalScriptText(src);
    runScriptContent(host, shadowRoot, environment, scriptContent);
    runs.add(src);
}

async function loadExternalScriptElement(src: string, type: string | null) {
    const registry = getScriptLoadRegistry();
    if (!registry[src]) {
        registry[src] = new Promise<void>((resolve, reject) => {
            const scriptElement = document.createElement("script");
            if (type) {
                scriptElement.type = type;
            }
            scriptElement.src = src;
            scriptElement.async = false;
            scriptElement.onload = () => resolve();
            scriptElement.onerror = () => {
                reject(new Error(`preview_code 加载外部脚本失败: ${src}`));
            };
            document.head.appendChild(scriptElement);
        }).catch((error) => {
            delete registry[src];
            throw error;
        });
    }
    await registry[src];
}

async function ensureExternalScriptLoaded(
    src: string,
    type: string | null,
    host: HTMLElement,
    shadowRoot: ShadowRoot,
    environment: PreviewScriptEnvironment
) {
    const normalizedSrc = normalizeExternalScriptUrl(src);
    if (isClassicJavaScriptType(type)) {
        await runPreviewScopedExternalScript(normalizedSrc, host, shadowRoot, environment);
        return;
    }
    await loadExternalScriptElement(normalizedSrc, type);
}

function getElementByIdFromShadowRoot(shadowRoot: ShadowRoot, id: string): HTMLElement | null {
    if (typeof shadowRoot.getElementById === "function") {
        return shadowRoot.getElementById(id);
    }
    return shadowRoot.querySelector<HTMLElement>(`[id="${CSS.escape(id)}"]`);
}

function createPreviewDocumentFacade(shadowRoot: ShadowRoot, host: HTMLElement) {
    const ownerDocument = host.ownerDocument;
    const previewHead = {
        append: (...nodes: Node[]) => {
            nodes.forEach((node) => shadowRoot.appendChild(node));
        },
        appendChild: (node: Node) => shadowRoot.appendChild(node),
    };
    return {
        head: previewHead,
        body: shadowRoot,
        documentElement: shadowRoot,
        querySelector: shadowRoot.querySelector.bind(shadowRoot),
        querySelectorAll: shadowRoot.querySelectorAll.bind(shadowRoot),
        getElementById: (id: string) => getElementByIdFromShadowRoot(shadowRoot, id),
        createElement: ownerDocument.createElement.bind(ownerDocument),
        createElementNS: ownerDocument.createElementNS.bind(ownerDocument),
        createTextNode: ownerDocument.createTextNode.bind(ownerDocument),
        createDocumentFragment: ownerDocument.createDocumentFragment.bind(ownerDocument),
        addEventListener: shadowRoot.addEventListener.bind(shadowRoot),
        removeEventListener: shadowRoot.removeEventListener.bind(shadowRoot),
        dispatchEvent: shadowRoot.dispatchEvent.bind(shadowRoot),
        defaultView: window,
    };
}

type PreviewWindow = Window & Record<string, unknown>;

interface PreviewScriptEnvironment {
    previewWindow: PreviewWindow;
    previewDocument: ReturnType<typeof createPreviewDocumentFacade>;
    reportRuntimeError?: (message: string) => void;
}

function isPreviewScopedNode(node: Node, shadowRoot: ShadowRoot) {
    return node === shadowRoot || shadowRoot.contains(node);
}

function normalizePreviewMutationTarget(target: unknown, shadowRoot: ShadowRoot): Node {
    if (!(target instanceof Node)) {
        return shadowRoot;
    }
    return isPreviewScopedNode(target, shadowRoot) ? target : shadowRoot;
}

function createScopedMutationObserverConstructor(
    shadowRoot: ShadowRoot,
    reportRuntimeError: (message: string) => void
): typeof MutationObserver {
    const NativeMutationObserver = window.MutationObserver;
    if (typeof NativeMutationObserver !== "function") {
        return NativeMutationObserver;
    }

    return class ScopedPreviewMutationObserver {
        private readonly nativeObserver: MutationObserver;
        private pendingRecords: MutationRecord[] = [];
        private flushTimer: number | null = null;
        private callbackWindowStartedAt = 0;
        private callbackCount = 0;
        private disconnected = false;

        constructor(private readonly callback: MutationCallback) {
            this.nativeObserver = new NativeMutationObserver((records) => {
                if (this.disconnected) {
                    return;
                }
                const scopedRecords = records.filter((record) =>
                    isPreviewScopedNode(record.target, shadowRoot)
                );
                if (scopedRecords.length === 0) {
                    return;
                }
                this.pendingRecords.push(...scopedRecords);
                this.scheduleFlush();
            });
        }

        observe(target: Node, options?: MutationObserverInit) {
            if (this.disconnected) {
                return;
            }
            this.nativeObserver.observe(normalizePreviewMutationTarget(target, shadowRoot), options);
        }

        disconnect() {
            this.disconnected = true;
            this.pendingRecords = [];
            if (this.flushTimer !== null) {
                window.clearTimeout(this.flushTimer);
                this.flushTimer = null;
            }
            this.nativeObserver.disconnect();
        }

        takeRecords(): MutationRecord[] {
            const nativeRecords = this.nativeObserver
                .takeRecords()
                .filter((record) => isPreviewScopedNode(record.target, shadowRoot));
            const records = [...this.pendingRecords, ...nativeRecords];
            this.pendingRecords = [];
            return records;
        }

        private scheduleFlush() {
            if (this.flushTimer !== null) {
                return;
            }
            this.flushTimer = window.setTimeout(
                () => this.flush(),
                PREVIEW_CODE_MUTATION_OBSERVER_FLUSH_MS
            );
        }

        private flush() {
            this.flushTimer = null;
            if (this.disconnected || this.pendingRecords.length === 0) {
                return;
            }

            const now = Date.now();
            if (
                this.callbackWindowStartedAt === 0
                || now - this.callbackWindowStartedAt > PREVIEW_CODE_MUTATION_OBSERVER_BUDGET_WINDOW_MS
            ) {
                this.callbackWindowStartedAt = now;
                this.callbackCount = 0;
            }
            this.callbackCount += 1;

            if (this.callbackCount > PREVIEW_CODE_MUTATION_OBSERVER_MAX_CALLBACKS) {
                this.disconnect();
                reportRuntimeError(
                    "preview_code 已停止一个高频 MutationObserver：预览脚本持续扫描 DOM，可能导致主界面卡死。"
                );
                return;
            }

            const records = this.pendingRecords;
            this.pendingRecords = [];
            this.callback(records, this as unknown as MutationObserver);
        }
    } as typeof MutationObserver;
}

const inlineHandlerRegistry = new WeakMap<Element, Map<string, EventListener>>();

function definePreviewWindowValue(previewWindow: PreviewWindow, name: string, value: unknown) {
    Object.defineProperty(previewWindow, name, {
        configurable: true,
        writable: true,
        value,
    });
}

function definePreviewWindowNativeGlobals(previewWindow: PreviewWindow) {
    const sourceWindow = window as unknown as Record<string, unknown>;
    const nativeValueNames = [
        "performance",
        "navigator",
        "location",
        "history",
        "crypto",
        "screen",
        "localStorage",
        "sessionStorage",
        "console",
        "innerWidth",
        "innerHeight",
        "devicePixelRatio",
    ];
    const nativeMethodNames = [
        "setTimeout",
        "clearTimeout",
        "setInterval",
        "clearInterval",
        "requestAnimationFrame",
        "cancelAnimationFrame",
        "queueMicrotask",
        "fetch",
    ];

    nativeValueNames.forEach((name) => {
        try {
            definePreviewWindowValue(previewWindow, name, sourceWindow[name]);
        } catch {
            // Some browser globals can throw when unavailable or blocked by settings.
        }
    });

    nativeMethodNames.forEach((name) => {
        const value = sourceWindow[name];
        if (typeof value === "function") {
            definePreviewWindowValue(previewWindow, name, value.bind(window));
        }
    });
}

function createPreviewScriptEnvironment(
    shadowRoot: ShadowRoot,
    host: HTMLElement,
    bridge: PreviewCodeBridge
): PreviewScriptEnvironment {
    const previewDocument = createPreviewDocumentFacade(shadowRoot, host);
    const bridgeRegistry = getBridgeRegistry();
    const previewWindow = Object.create(window) as PreviewWindow;
    const environment: PreviewScriptEnvironment = {
        previewWindow,
        previewDocument,
    };
    definePreviewWindowNativeGlobals(previewWindow);
    const ScopedMutationObserver = createScopedMutationObserverConstructor(
        shadowRoot,
        (message) => environment.reportRuntimeError?.(message)
    );

    Object.defineProperty(previewWindow, "document", {
        configurable: true,
        value: previewDocument,
    });
    Object.defineProperty(previewWindow, "window", {
        configurable: true,
        value: previewWindow,
    });
    Object.defineProperty(previewWindow, "self", {
        configurable: true,
        value: previewWindow,
    });
    Object.defineProperty(previewWindow, "globalThis", {
        configurable: true,
        value: previewWindow,
    });
    Object.defineProperty(previewWindow, "aippPreviewCode", {
        configurable: true,
        writable: true,
        value: bridge,
    });
    Object.defineProperty(previewWindow, "__AIPP_PREVIEW_CODE_BRIDGES__", {
        configurable: true,
        writable: true,
        value: bridgeRegistry,
    });
    Object.defineProperty(previewWindow, "MutationObserver", {
        configurable: true,
        value: ScopedMutationObserver,
    });

    return environment;
}

function collectExportedSymbolNames(scriptContent: string): string[] {
    const names = new Set<string>();
    const patterns = [
        /(?:^|[;\n\r]\s*)(?:async\s+)?function\s+([A-Za-z_$][\w$]*)\s*\(/g,
        /(?:^|[;\n\r]\s*)class\s+([A-Za-z_$][\w$]*)\s*/g,
        /(?:^|[;\n\r]\s*)(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=/g,
    ];

    patterns.forEach((pattern) => {
        for (const match of scriptContent.matchAll(pattern)) {
            const name = match[1];
            if (name) {
                names.add(name);
            }
        }
    });

    return Array.from(names);
}

function appendWindowExports(scriptContent: string): string {
    const names = collectExportedSymbolNames(scriptContent);
    if (names.length === 0) {
        return scriptContent;
    }

    const exportStatements = names
        .map((name) => `if (typeof ${name} !== "undefined") { window[${JSON.stringify(name)}] = ${name}; }`)
        .join("\n");
    return `${scriptContent}\n${exportStatements}`;
}

function bindInlineEventHandlers(
    root: ParentNode,
    environment: PreviewScriptEnvironment,
    host: HTMLElement,
    shadowRoot: ShadowRoot
) {
    const elements = "querySelectorAll" in root ? root.querySelectorAll("*") : [];
    elements.forEach((element) => {
        const existingBindings = inlineHandlerRegistry.get(element) ?? new Map<string, EventListener>();
        const nextBindings = new Set<string>();
        for (const attribute of Array.from(element.attributes)) {
            if (!attribute.name.startsWith("on")) {
                continue;
            }

            const eventName = attribute.name.slice(2);
            nextBindings.add(eventName);
            const handlerRunner = new Function(
                "event",
                "window",
                "document",
                "host",
                "shadowRoot",
                "aippPreviewCode",
                `with(window){ ${attribute.value} }`
            );

            const previousListener = existingBindings.get(eventName);
            if (previousListener) {
                element.removeEventListener(eventName, previousListener);
            }

            const listener: EventListener = (event) => {
                const result = handlerRunner.call(
                    environment.previewWindow,
                    event,
                    environment.previewWindow,
                    environment.previewDocument,
                    host,
                    shadowRoot,
                    environment.previewWindow.aippPreviewCode
                );

                if (result === false) {
                    event.preventDefault();
                    event.stopPropagation();
                }
            };

            element.removeAttribute(attribute.name);
            existingBindings.set(eventName, listener);
            element.addEventListener(eventName, listener);
        }

        for (const [eventName, listener] of existingBindings) {
            if (nextBindings.has(eventName)) {
                continue;
            }
            element.removeEventListener(eventName, listener);
            existingBindings.delete(eventName);
        }

        if (existingBindings.size > 0) {
            inlineHandlerRegistry.set(element, existingBindings);
        } else {
            inlineHandlerRegistry.delete(element);
        }
    });
}

async function executeScripts(
    host: HTMLElement,
    shadowRoot: ShadowRoot,
    environment: PreviewScriptEnvironment,
    scripts: Array<{ src: string | null; type: string | null; content: string }>
) {
    for (const script of scripts) {
        if (script.src) {
            await ensureExternalScriptLoaded(script.src, script.type, host, shadowRoot, environment);
            continue;
        }
        if (!script.content.trim()) {
            continue;
        }

        runScriptContent(host, shadowRoot, environment, script.content);
    }
}

export function createPreviewCodeRuntime(host: HTMLElement): PreviewCodeRuntimeController {
    let disposed = false;
    let scheduledApply: ScheduledApplyHandle | null = null;
    let latestUpdate: PreviewCodeRuntimeUpdate | null = null;
    let lastFinalSignature: string | null = null;
    let lastBridgeId: string | null = null;
    let lastRenderedCode: string | null = null;
    let lastScripts: Array<{ src: string | null; type: string | null; content: string }> = [];
    let lastEnvironment: PreviewScriptEnvironment | null = null;
    let lastAppliedAt = 0;
    let lastPatchedAt = 0;
    const shadowRoot = host.shadowRoot ?? host.attachShadow({ mode: "open" });
    const styleElement = document.createElement("style");
    styleElement.textContent = PREVIEW_CODE_RUNTIME_STYLES;
    const shell = document.createElement("div");
    shell.className = "aipp-preview-code-shell";
    shell.dataset.streaming = "false";
    const mountPoint = document.createElement("div");
    mountPoint.className = "aipp-preview-code-root";
    shell.appendChild(mountPoint);
    shadowRoot.replaceChildren(styleElement, shell);

    const applyLatest = () => {
        if (disposed || !latestUpdate) {
            return;
        }

        const { code, isFinal, bridgeId, bridge, onError } = latestUpdate;
        shell.dataset.streaming = (!isFinal).toString();
        const registry = getBridgeRegistry();
        registry[bridgeId] = bridge;
        const liveBridge: PreviewCodeBridge = {
            submit: (payload?: unknown) => getLiveBridgeOrThrow(bridgeId).submit(payload),
            close: () => getLiveBridgeOrThrow(bridgeId).close(),
            emitEvent: (name: string, payload?: unknown) =>
                getLiveBridgeOrThrow(bridgeId).emitEvent(name, payload),
        };

        try {
            if (!lastEnvironment || lastBridgeId !== bridgeId) {
                lastEnvironment = createPreviewScriptEnvironment(shadowRoot, host, liveBridge);
                bindInlineEventHandlers(mountPoint, lastEnvironment, host, shadowRoot);
                if (lastBridgeId !== null && lastBridgeId !== bridgeId) {
                    delete registry[lastBridgeId];
                    lastFinalSignature = null;
                }
                lastBridgeId = bridgeId;
            } else {
                lastEnvironment.previewWindow.aippPreviewCode = liveBridge;
            }
            lastEnvironment.reportRuntimeError = (message: string) => onError?.(message);

            if (lastRenderedCode !== code) {
                const canPatch = isFinal
                    || lastPatchedAt === 0
                    || Date.now() - lastPatchedAt >= PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS;
                if (canPatch) {
                    const parsed = buildTargetMountPoint(code);
                    lastScripts = parsed.scripts;
                    patchMountPoint(mountPoint, parsed.mountPoint);
                    bindInlineEventHandlers(mountPoint, lastEnvironment, host, shadowRoot);
                    lastRenderedCode = code;
                    lastPatchedAt = Date.now();
                } else if (scheduledApply === null) {
                    scheduledApply = scheduleApply(
                        flushLatest,
                        PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS - (Date.now() - lastPatchedAt),
                    );
                }
            } else {
                lastEnvironment.previewWindow.aippPreviewCode = liveBridge;
            }

            if (isFinal && lastFinalSignature !== code) {
                lastFinalSignature = code;
                void executeScripts(host, shadowRoot, lastEnvironment, lastScripts)
                    .then(() => undefined)
                    .catch((error) => {
                        const message =
                            error instanceof Error
                                ? error.message
                                : "Failed to execute preview_code scripts";
                        onError?.(message);
                    });
            } else if (!isFinal) {
                lastFinalSignature = null;
            }
            onError?.(null);
        } catch (error) {
            const message =
                error instanceof Error ? error.message : "Failed to render preview_code";
            onError?.(message);
        }
    };

    const flushLatest = () => {
        scheduledApply = null;
        if (disposed || !latestUpdate) {
            return;
        }
        lastAppliedAt = Date.now();
        applyLatest();
    };

    return {
        update(next) {
            latestUpdate = next;
            const registry = getBridgeRegistry();
            registry[next.bridgeId] = next.bridge;

            if (next.isFinal) {
                cancelScheduledApply(scheduledApply);
                scheduledApply = scheduleApply(flushLatest, 0);
                return;
            }

            if (scheduledApply !== null) {
                return;
            }

            const throttleDelay =
                lastAppliedAt === 0
                    ? 0
                    : Math.max(
                          PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS - (Date.now() - lastAppliedAt),
                          0
                      );
            scheduledApply = scheduleApply(flushLatest, throttleDelay);
        },
        destroy() {
            disposed = true;
            cancelScheduledApply(scheduledApply);
            scheduledApply = null;
            if (latestUpdate) {
                const registry = getBridgeRegistry();
                delete registry[latestUpdate.bridgeId];
            }
            lastBridgeId = null;
            lastRenderedCode = null;
            lastScripts = [];
            lastEnvironment = null;
            shadowRoot.replaceChildren();
        },
    };
}
