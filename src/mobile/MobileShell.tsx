import { lazy, Suspense, useEffect, useLayoutEffect, useRef, useState } from "react";
import LoadingScreen from "../components/LoadingScreen";
import { isMobileRoute, MobileRoute, MobilePreview } from "./navigation";

const Chat = lazy(() => import("../windows/ChatUIWindow"));
const Config = lazy(() => import("../windows/ConfigWindow"));
const Collections = lazy(() => import("./MobileCollections"));
const Preview = lazy(() => import("./MobileArtifactPreview"));

declare global {
    interface Window { __aippMobileBack?: () => boolean }
}

/** Keep visited pages mounted: streams, drafts and unsaved forms belong to the session. */
export default function MobileShell() {
    const [stack, setStack] = useState<MobileRoute[]>(["chat_ui"]);
    const [visited, setVisited] = useState<MobileRoute[]>(["chat_ui"]);
    const [preview, setPreview] = useState<MobilePreview | null>(null);
    const shellRef = useRef<HTMLDivElement>(null);
    const stackRef = useRef(stack);
    const pageRefs = useRef(new Map<MobileRoute, HTMLDivElement>());
    const positions = useRef(new Map<MobileRoute, { element: HTMLElement; top: number; left: number }[]>());
    const capturePosition = () => {
        const route = stackRef.current.at(-1)!;
        const page = pageRefs.current.get(route);
        if (page) positions.current.set(route, Array.from(page.querySelectorAll<HTMLElement>("*")).filter(element => element.scrollHeight > element.clientHeight || element.scrollWidth > element.clientWidth).map(element => ({ element, top: element.scrollTop, left: element.scrollLeft })));
    };
    stackRef.current = stack;
    useEffect(() => {
        // Add a stricter mobile frame policy without relaxing the app's CSP.
        // In particular, component frames must not navigate to local services.
        const framePolicy = document.createElement("meta");
        framePolicy.httpEquiv = "Content-Security-Policy";
        framePolicy.content = "frame-src 'self'";
        document.head.append(framePolicy);
        const navigate = (event: Event) => {
            const route = (event as CustomEvent).detail;
            if (!isMobileRoute(route)) return;
            capturePosition();
            setVisited(previous => previous.includes(route) ? previous : [...previous, route]);
            const previous = stackRef.current;
            stackRef.current = route === "chat_ui" ? ["chat_ui"] : previous.at(-1) === route ? previous : [...previous, route];
            setStack(stackRef.current);
        };
        window.addEventListener("aipp:mobile-navigate", navigate);
        const showPreview = (event: Event) => {
            const detail = (event as CustomEvent<MobilePreview>).detail;
            if (!detail || typeof detail.language !== "string" || typeof detail.code !== "string") return;
            setPreview(previous => previous?.language === detail.language && previous.code === detail.code && previous.title === detail.title ? previous : detail);
            navigate(new CustomEvent("aipp:mobile-navigate", { detail: "preview" }));
        };
        window.addEventListener("aipp:mobile-preview", showPreview);
        window.__aippMobileBack = () => {
            const panels = document.querySelectorAll<HTMLElement>('[role="dialog"][data-state="open"], [role="alertdialog"][data-state="open"], [role="menu"][data-state="open"], [data-mobile-panel]');
            const panel = Array.from(panels).filter(node => node.getClientRects().length > 0).at(-1);
            if (panel) {
                panel.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true }));
                return true;
            }
            if (stackRef.current.length <= 1) return false;
            capturePosition();
            stackRef.current = stackRef.current.slice(0, -1);
            setStack(stackRef.current);
            return true;
        };
        return () => {
            framePolicy.remove();
            window.removeEventListener("aipp:mobile-navigate", navigate);
            window.removeEventListener("aipp:mobile-preview", showPreview);
            delete window.__aippMobileBack;
        };
    }, []);
    // Android WebView can pan the visual viewport instead of resizing the layout viewport
    // when the IME opens. Keep the mobile shell inside the visible viewport in that case.
    // Owner decision: MobileShell owns viewport geometry (height + translateY). While the
    // shell transform is active, force --aipp-keyboard-inset-bottom to 0 on the shell so
    // chat composer / sticky footers do not double-lift. Document-level CSS var remains
    // for pages that are not under an active shell keyboard offset.
    useEffect(() => {
        const viewport = window.visualViewport;
        const shell = shellRef.current;
        if (!viewport || !shell) return;
        const update = () => {
            const keyboardOpen = window.innerHeight - viewport.height > 150;
            shell.style.height = keyboardOpen ? `${viewport.height}px` : "";
            shell.style.transform = keyboardOpen ? `translateY(${viewport.offsetTop}px)` : "";
            if (keyboardOpen) {
                shell.style.setProperty("--aipp-keyboard-inset-bottom", "0px");
                shell.dataset.aippShellKeyboardOffset = "true";
            } else {
                shell.style.removeProperty("--aipp-keyboard-inset-bottom");
                delete shell.dataset.aippShellKeyboardOffset;
            }
        };
        viewport.addEventListener("resize", update);
        viewport.addEventListener("scroll", update);
        update();
        return () => {
            viewport.removeEventListener("resize", update);
            viewport.removeEventListener("scroll", update);
            shell.style.removeProperty("--aipp-keyboard-inset-bottom");
            delete shell.dataset.aippShellKeyboardOffset;
        };
    }, []);
    const current = stack.at(-1);
    useLayoutEffect(() => {
        const restore = () => positions.current.get(current!)?.forEach(({ element, top, left }) => {
            element.scrollTop = top; element.scrollLeft = left;
        });
        restore();
        const frame = requestAnimationFrame(restore);
        return () => cancelAnimationFrame(frame);
    }, [current]);
    return <div ref={shellRef} className="h-screen overflow-hidden" data-aipp-mobile-shell>
        {visited.map(route => <div key={route} ref={element => { if (element) pageRefs.current.set(route, element); else pageRefs.current.delete(route); }} hidden={current !== route} className="h-full">
            <Suspense fallback={<LoadingScreen title={route === "chat_ui" ? "正在准备你的 AI 助手" : "正在打开页面"} />}>
                {route === "chat_ui" ? <Chat /> : route === "config" ? <Config /> : route === "collections" ? <Collections /> : preview && <Preview preview={preview} />}
            </Suspense>
        </div>)}
    </div>;
}
