import { useEffect, useRef, useState } from "react";
import mermaid from "mermaid";
import { Button } from "@/components/ui/button";

let mermaidInitializedFor: boolean | null = null;

function ensureMermaid() {
    const isDark = document.documentElement.classList.contains("dark");
    if (mermaidInitializedFor === isDark) return;
    mermaid.initialize({
        startOnLoad: false,
        theme: isDark ? "dark" : "default",
        securityLevel: "loose",
        fontFamily: "monospace",
        themeVariables: { darkMode: isDark },
        flowchart: { useMaxWidth: false },
    });
    mermaidInitializedFor = isDark;
}

interface MermaidPreviewProps {
    code: string;
    /** 渲染成功返回 SVG（供导出），失败返回错误信息（进控制台）。 */
    onRendered?: (result: { svg: string } | { error: string }) => void;
}

/** Mermaid 图表：滚轮缩放，按住空格拖动。 */
export default function MermaidPreview({ code, onRendered }: MermaidPreviewProps) {
    const containerRef = useRef<HTMLDivElement | null>(null);
    const canvasRef = useRef<HTMLDivElement | null>(null);
    const [svg, setSvg] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [scale, setScale] = useState(1);
    const [position, setPosition] = useState({ x: 0, y: 0 });
    const [isSpacePressed, setIsSpacePressed] = useState(false);
    const [dragStart, setDragStart] = useState<{ x: number; y: number } | null>(null);
    const onRenderedRef = useRef(onRendered);
    onRenderedRef.current = onRendered;

    useEffect(() => {
        let cancelled = false;
        const source = code.trim();
        if (!source) {
            setSvg("");
            setError("Mermaid 内容为空");
            return;
        }
        ensureMermaid();
        mermaid
            .render(`mermaid-${Date.now()}-${Math.random().toString(36).slice(2)}`, source)
            .then(({ svg: rendered }) => {
                if (cancelled) return;
                setSvg(rendered);
                setError(null);
                onRenderedRef.current?.({ svg: rendered });
            })
            .catch((renderError: unknown) => {
                if (cancelled) return;
                const message = renderError instanceof Error ? renderError.message : String(renderError);
                setSvg("");
                setError(message);
                onRenderedRef.current?.({ error: `Mermaid 渲染失败：${message}` });
            });
        return () => {
            cancelled = true;
        };
    }, [code]);

    // 渲染结果做成响应式：去掉固定宽高，保留 viewBox
    useEffect(() => {
        const svgElement = canvasRef.current?.querySelector("svg");
        if (!svgElement) return;
        const width = svgElement.getAttribute("width");
        const height = svgElement.getAttribute("height");
        if (!svgElement.getAttribute("viewBox") && width && height) {
            svgElement.setAttribute("viewBox", `0 0 ${width} ${height}`);
        }
        svgElement.removeAttribute("width");
        svgElement.removeAttribute("height");
        Object.assign(svgElement.style, { width: "100%", height: "auto", maxWidth: "100%", display: "block", margin: "0 auto" });
    }, [svg]);

    useEffect(() => {
        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.code === "Space" && containerRef.current?.matches(":hover")) {
                event.preventDefault();
                setIsSpacePressed(true);
            }
        };
        const handleKeyUp = (event: KeyboardEvent) => {
            if (event.code === "Space") {
                setIsSpacePressed(false);
                setDragStart(null);
            }
        };
        const container = containerRef.current;
        const handleWheel = (event: WheelEvent) => {
            event.preventDefault();
            const delta = event.deltaY > 0 ? -0.1 : 0.1;
            setScale((prev) => Math.max(0.1, Math.min(3, prev + delta)));
        };
        document.addEventListener("keydown", handleKeyDown);
        document.addEventListener("keyup", handleKeyUp);
        container?.addEventListener("wheel", handleWheel, { passive: false });
        return () => {
            document.removeEventListener("keydown", handleKeyDown);
            document.removeEventListener("keyup", handleKeyUp);
            container?.removeEventListener("wheel", handleWheel);
        };
    }, []);

    const resetView = () => {
        setScale(1);
        setPosition({ x: 0, y: 0 });
    };

    return (
        <div className="flex-1 flex flex-col min-h-0 p-3 gap-2">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>缩放 {Math.round(scale * 100)}% · 滚轮缩放，按住空格拖动</span>
                <Button variant="ghost" size="sm" onClick={resetView}>
                    重置视图
                </Button>
            </div>
            <div
                ref={containerRef}
                className={`flex-1 min-h-0 overflow-hidden rounded-md border border-border bg-background ${
                    isSpacePressed ? (dragStart ? "cursor-grabbing" : "cursor-grab") : "cursor-default"
                }`}
                onMouseDown={(event) => {
                    if (isSpacePressed) setDragStart({ x: event.clientX - position.x, y: event.clientY - position.y });
                }}
                onMouseMove={(event) => {
                    if (dragStart && isSpacePressed) setPosition({ x: event.clientX - dragStart.x, y: event.clientY - dragStart.y });
                }}
                onMouseUp={() => setDragStart(null)}
                onMouseLeave={() => setDragStart(null)}
            >
                {error ? (
                    <div className="p-4 text-sm text-destructive whitespace-pre-wrap">渲染失败：{error}</div>
                ) : (
                    <div
                        ref={canvasRef}
                        className="flex h-full w-full items-center justify-center p-5"
                        style={{
                            transform: `scale(${scale}) translate(${position.x}px, ${position.y}px)`,
                            transformOrigin: "center center",
                            transition: dragStart ? "none" : "transform 0.1s ease-out",
                        }}
                        dangerouslySetInnerHTML={{ __html: svg }}
                    />
                )}
            </div>
        </div>
    );
}
