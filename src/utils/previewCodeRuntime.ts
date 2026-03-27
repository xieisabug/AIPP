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

function buildScopedBridgeExpression(bridgeId: string): string {
    return `window.__AIPP_PREVIEW_CODE_BRIDGES__[${JSON.stringify(bridgeId)}]`;
}

function rewriteBridgeReferences(scriptContent: string, bridgeId: string): string {
    const scopedBridge = buildScopedBridgeExpression(bridgeId);
    return scriptContent
        .replaceAll("window.aippPreviewCode", scopedBridge)
        .replaceAll("globalThis.aippPreviewCode", scopedBridge);
}

function rewriteInlineBridgeReferences(root: ParentNode, bridgeId: string) {
    const elements = "querySelectorAll" in root ? root.querySelectorAll("*") : [];
    elements.forEach((element) => {
        for (const attribute of Array.from(element.attributes)) {
            if (!attribute.name.startsWith("on")) {
                continue;
            }
            element.setAttribute(attribute.name, rewriteBridgeReferences(attribute.value, bridgeId));
        }
    });
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

async function executeScripts(
    shadowRoot: ShadowRoot,
    host: HTMLElement,
    bridgeId: string,
    bridge: PreviewCodeBridge,
    scripts: Array<{ src: string | null; type: string | null; content: string }>
) {
    const previewDocument = createPreviewDocumentFacade(shadowRoot, host);
    const bridgeRegistry = getBridgeRegistry();

    for (const script of scripts) {
        if (script.src) {
            await ensureExternalScriptLoaded(script.src, script.type);
            continue;
        }
        if (!script.content.trim()) {
            continue;
        }
        const previewWindow = Object.create(window) as Window &
            Record<string, unknown>;
        Object.defineProperty(previewWindow, "document", {
            configurable: true,
            value: previewDocument,
        });
        Object.defineProperty(previewWindow, "aippPreviewCode", {
            configurable: true,
            value: bridge,
        });
        Object.defineProperty(previewWindow, "__AIPP_PREVIEW_CODE_BRIDGES__", {
            configurable: true,
            value: bridgeRegistry,
        });

        const scriptRunner = new Function(
            "window",
            "document",
            "globalThis",
            "host",
            "shadowRoot",
            "aippPreviewCode",
            rewriteBridgeReferences(script.content, bridgeId)
        );
        scriptRunner(previewWindow, previewDocument, previewWindow, host, shadowRoot, bridge);
    }
}

export function createPreviewCodeRuntime(host: HTMLElement): PreviewCodeRuntimeController {
    let disposed = false;
    let scheduledFrame: ScheduledFrameHandle | null = null;
    let latestUpdate: PreviewCodeRuntimeUpdate | null = null;
    let lastFinalSignature: string | null = null;
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

        try {
            const template = document.createElement("template");
            template.innerHTML = code;
            rewriteInlineBridgeReferences(template.content, bridgeId);
            normalizeStyleNodes(template.content);
            const scripts = collectScriptNodes(template.content);
            mountPoint.replaceChildren(template.content.cloneNode(true));

            if (isFinal && lastFinalSignature !== code) {
                lastFinalSignature = code;
                void executeScripts(shadowRoot, host, bridgeId, bridge, scripts).catch((error) => {
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
            shadowRoot.replaceChildren();
        },
    };
}

