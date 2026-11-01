import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import { renderToStaticMarkup } from "react-dom/server";
import { Button } from "@/components/ui/button";
import { MobilePreview } from "./navigation";
import { staticPreviewDocument } from "./previewDocument";
import runtimeUrl from "./previewRuntime.ts?worker&url";
import { usePlatform } from "@/hooks/use-platform";
import SaveArtifactDialog from "@/components/SaveArtifactDialog";

export default function MobileArtifactPreview({ preview }: { preview: MobilePreview }) {
    const platform = usePlatform();
    const [document, setDocument] = useState("");
    const [error, setError] = useState("");
    const [zoom, setZoom] = useState(1);
    const [saveOpen, setSaveOpen] = useState(false);
    const frame = useRef<HTMLIFrameElement>(null);
    const initializedFrames = useRef(new WeakSet<HTMLIFrameElement>());
    const loadTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
    const component = ["react", "jsx", "vue"].includes(preview.language.toLowerCase());
    useEffect(() => {
        const ready = (event: MessageEvent) => {
            if (event.source === frame.current?.contentWindow && event.data?.type === "aipp-preview-runtime-ready") clearTimeout(loadTimer.current);
        };
        window.addEventListener("message", ready);
        return () => { window.removeEventListener("message", ready); clearTimeout(loadTimer.current); };
    }, []);
    useEffect(() => {
        let active = true;
        clearTimeout(loadTimer.current);
        setDocument(""); setError(""); setZoom(1);
        const build = async () => {
            const language = preview.language.toLowerCase();
            if (["react", "jsx", "vue"].includes(language)) {
                const url = new URL(runtimeUrl, window.location.href).href.replace(/&/g, "&amp;").replace(/"/g, "&quot;");
                if (active) setDocument(`<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src ${url} 'unsafe-eval'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; form-action 'none'; base-uri 'none'"></head><body><pre id="error" role="alert" hidden></pre><div id="root"></div><script src="${url}"></script></body></html>`);
                return;
            }
            let markup: string;
            if (["markdown", "md"].includes(language)) markup = renderToStaticMarkup(<ReactMarkdown skipHtml>{preview.code}</ReactMarkdown>);
            else if (["text", "txt", "xml"].includes(language)) markup = renderToStaticMarkup(<pre style={{ whiteSpace: "pre-wrap" }}>{preview.code}</pre>);
            else if (language === "image") {
                if (!/^data:image\/(png|jpeg|gif|webp);base64,/.test(preview.code)) throw new Error("图片预览仅支持已导入的 PNG/JPEG/GIF/WebP 数据；远端地址不可直接加载");
                markup = renderToStaticMarkup(<img src={preview.code} alt="制品图片" style={{ maxWidth: "100%" }} />);
            } else if (language === "mermaid") {
                const { default: mermaid } = await import("mermaid");
                mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
                markup = (await mermaid.render(`mobile-mermaid-${crypto.randomUUID()}`, preview.code)).svg;
            } else if (["html", "svg"].includes(language)) markup = preview.code;
            else throw new Error(`移动预览不支持 ${language} 格式`);
            if (active) setDocument(staticPreviewDocument(markup));
        };
        void build().catch(e => { if (active) setError(`制品渲染失败（${preview.language}）：${String(e)}`); });
        return () => { active = false; };
    }, [preview.language, preview.code, preview.title]);
    return <section className={`h-full flex flex-col safe-top safe-bottom bg-background ${platform === "android" ? "safe-top-android" : ""}`}>
        <header className="flex flex-wrap gap-2 items-center p-2">
            <Button className="min-h-11" variant="ghost" onClick={() => window.__aippMobileBack?.()}>返回</Button>
            <h1 className="flex-1 truncate">{preview.title || "制品预览"}</h1>
            <Button className="min-h-11" variant="outline" onClick={() => setSaveOpen(true)}>保存到收藏</Button>
            <Button className="min-h-11 min-w-11" variant="outline" aria-label="缩小" onClick={() => setZoom(value => Math.max(.5, value - .25))}>−</Button>
            <span>{Math.round(zoom * 100)}%</span>
            <Button className="min-h-11 min-w-11" variant="outline" aria-label="放大" onClick={() => setZoom(value => Math.min(3, value + .25))}>+</Button>
        </header>
        {error && <p role="alert" className="p-4">{error}</p>}
        <div className="flex-1 overflow-auto min-h-0">
            {document && <iframe key={JSON.stringify([preview.language, preview.code, preview.title])} ref={frame} title="隔离制品预览" sandbox={component ? "allow-scripts" : ""} referrerPolicy="no-referrer" srcDoc={document}
                onLoad={event => { if (component) {
                    clearTimeout(loadTimer.current);
                    const loadedFrame = event.currentTarget;
                    if (initializedFrames.current.has(loadedFrame)) {
                        setError("组件预览已停止：预览框发生了页面导航，请重新打开制品。");
                        return;
                    }
                    initializedFrames.current.add(loadedFrame);
                    loadTimer.current = setTimeout(() => setError("组件运行时未响应：本地预览脚本加载或初始化失败，请重新打开制品。"), 10000);
                    loadedFrame.contentWindow?.postMessage({ type: "aipp-preview-component", language: preview.language.toLowerCase(), code: preview.code }, "*");
                } }}
                className="border-0 bg-white w-full h-full" style={{ zoom }} />}
        </div>
        <SaveArtifactDialog
            isOpen={saveOpen}
            onClose={() => setSaveOpen(false)}
            artifactType={preview.language || "text"}
            code={preview.code}
        />
    </section>;
}
