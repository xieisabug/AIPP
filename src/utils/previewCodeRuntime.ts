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
    }
}

const PREVIEW_CODE_FRAME_MS = 16;
const ALLOWED_EXTERNAL_SCRIPT_ORIGINS = new Set([
    "https://cdn.jsdelivr.net",
    "https://unpkg.com",
    "https://cdnjs.cloudflare.com",
    "https://esm.sh",
]);

type ScheduledFrameHandle =
    | { kind: "raf"; id: number }
    | { kind: "timeout"; id: number };

function scheduleNextFrame(callback: FrameRequestCallback): ScheduledFrameHandle {
    if (typeof window.requestAnimationFrame === "function") {
        return {
            kind: "raf",
            id: window.requestAnimationFrame(callback),
        };
    }
    return {
        kind: "timeout",
        id: window.setTimeout(() => callback(performance.now()), PREVIEW_CODE_FRAME_MS),
    };
}

function cancelScheduledFrame(handle: ScheduledFrameHandle | null) {
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
    return Array.from(root.querySelectorAll("script")).map((script) => ({
        src: script.getAttribute("src"),
        type: script.getAttribute("type"),
        content: script.textContent ?? "",
    }));
}

function normalizeExternalScriptUrl(src: string): string {
    const url = new URL(src, window.location.href);
    if (!ALLOWED_EXTERNAL_SCRIPT_ORIGINS.has(url.origin)) {
        throw new Error(`preview_code 不允许加载外部脚本: ${url.origin}`);
    }
    return url.toString();
}

async function ensureExternalScriptLoaded(src: string, type: string | null) {
    const normalizedSrc = normalizeExternalScriptUrl(src);
    const registry = getScriptLoadRegistry();
    if (registry[normalizedSrc]) {
        await registry[normalizedSrc];
        return;
    }

    registry[normalizedSrc] = new Promise<void>((resolve, reject) => {
        const scriptElement = document.createElement("script");
        if (type) {
            scriptElement.type = type;
        }
        scriptElement.src = normalizedSrc;
        scriptElement.async = false;
        scriptElement.onload = () => resolve();
        scriptElement.onerror = () => {
            delete registry[normalizedSrc];
            reject(new Error(`preview_code 加载外部脚本失败: ${normalizedSrc}`));
        };
        document.head.appendChild(scriptElement);
    });

    await registry[normalizedSrc];
}

function getElementByIdFromShadowRoot(shadowRoot: ShadowRoot, id: string): HTMLElement | null {
    if (typeof shadowRoot.getElementById === "function") {
        return shadowRoot.getElementById(id);
    }
    return shadowRoot.querySelector<HTMLElement>(`[id="${CSS.escape(id)}"]`);
}

function createPreviewDocumentFacade(shadowRoot: ShadowRoot, host: HTMLElement) {
    const ownerDocument = host.ownerDocument;
    return {
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
}

function createPreviewScriptEnvironment(
    shadowRoot: ShadowRoot,
    host: HTMLElement,
    bridge: PreviewCodeBridge
): PreviewScriptEnvironment {
    const previewDocument = createPreviewDocumentFacade(shadowRoot, host);
    const bridgeRegistry = getBridgeRegistry();
    const previewWindow = Object.create(window) as PreviewWindow;

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

    return {
        previewWindow,
        previewDocument,
    };
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
        for (const attribute of Array.from(element.attributes)) {
            if (!attribute.name.startsWith("on")) {
                continue;
            }

            const eventName = attribute.name.slice(2);
            const handlerRunner = new Function(
                "event",
                "window",
                "document",
                "host",
                "shadowRoot",
                "aippPreviewCode",
                `with(window){ ${attribute.value} }`
            );

            element.removeAttribute(attribute.name);
            element.addEventListener(eventName, (event) => {
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
            });
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
            await ensureExternalScriptLoaded(script.src, script.type);
            continue;
        }
        if (!script.content.trim()) {
            continue;
        }

        const scriptRunner = new Function(
            "window",
            "document",
            "globalThis",
            "host",
            "shadowRoot",
            "aippPreviewCode",
            appendWindowExports(script.content)
        );
        scriptRunner.call(
            environment.previewWindow,
            environment.previewWindow,
            environment.previewDocument,
            environment.previewWindow,
            host,
            shadowRoot,
            environment.previewWindow.aippPreviewCode
        );
    }
}

export function createPreviewCodeRuntime(host: HTMLElement): PreviewCodeRuntimeController {
    let disposed = false;
    let scheduledFrame: ScheduledFrameHandle | null = null;
    let latestUpdate: PreviewCodeRuntimeUpdate | null = null;
    let lastFinalSignature: string | null = null;
    let lastBridgeId: string | null = null;
    let lastRenderedCode: string | null = null;
    let lastScripts: Array<{ src: string | null; type: string | null; content: string }> = [];
    let lastEnvironment: PreviewScriptEnvironment | null = null;
    const shadowRoot = host.shadowRoot ?? host.attachShadow({ mode: "open" });
    const mountPoint = document.createElement("div");
    mountPoint.className = "aipp-preview-code-root";
    shadowRoot.replaceChildren(mountPoint);

    const applyLatest = () => {
        if (disposed || !latestUpdate) {
            return;
        }

        const { code, isFinal, bridgeId, bridge, onError } = latestUpdate;
        const registry = getBridgeRegistry();
        registry[bridgeId] = bridge;
        const liveBridge: PreviewCodeBridge = {
            submit: (payload?: unknown) => getLiveBridgeOrThrow(bridgeId).submit(payload),
            close: () => getLiveBridgeOrThrow(bridgeId).close(),
            emitEvent: (name: string, payload?: unknown) =>
                getLiveBridgeOrThrow(bridgeId).emitEvent(name, payload),
        };

        try {
            if (!lastEnvironment || lastRenderedCode !== code || lastBridgeId !== bridgeId) {
                const template = document.createElement("template");
                template.innerHTML = code;
                normalizeStyleNodes(template.content);
                lastScripts = collectScriptNodes(template.content);
                mountPoint.replaceChildren(template.content.cloneNode(true));
                lastEnvironment = createPreviewScriptEnvironment(shadowRoot, host, liveBridge);
                bindInlineEventHandlers(mountPoint, lastEnvironment, host, shadowRoot);
                if (lastBridgeId !== null && lastBridgeId !== bridgeId) {
                    lastFinalSignature = null;
                }
                lastBridgeId = bridgeId;
                lastRenderedCode = code;
            } else {
                lastEnvironment.previewWindow.aippPreviewCode = liveBridge;
            }

            if (isFinal && lastFinalSignature !== code) {
                lastFinalSignature = code;
                void executeScripts(host, shadowRoot, lastEnvironment, lastScripts).catch((error) => {
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

    return {
        update(next) {
            latestUpdate = next;
            const registry = getBridgeRegistry();
            registry[next.bridgeId] = next.bridge;
            if (scheduledFrame !== null) {
                return;
            }
            scheduledFrame = scheduleNextFrame(() => {
                scheduledFrame = null;
                applyLatest();
            });
        },
        destroy() {
            disposed = true;
            cancelScheduledFrame(scheduledFrame);
            scheduledFrame = null;
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

