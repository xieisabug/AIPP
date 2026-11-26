import { useEffect, useRef } from "react";

const DRAWIO_EMBED_URL = "https://embed.diagrams.net/?embed=1&ui=min&spin=1&proto=json&noSaveBtn=1&noExitBtn=1";

interface DrawioPreviewProps {
    xml: string;
    /** 图表在编辑器里被改动（autosave）时回传最新 XML。 */
    onChange?: (xml: string) => void;
    /** 用户在编辑器里保存时回传 XML，由宿主决定写到哪里。 */
    onSave?: (xml: string) => void;
    onLog?: (message: string) => void;
}

/** Draw.io 嵌入编辑器：按 JSON 协议加载 XML，并把改动回传。 */
export default function DrawioPreview({ xml, onChange, onSave, onLog }: DrawioPreviewProps) {
    const iframeRef = useRef<HTMLIFrameElement>(null);
    // 只在首次 init 时加载传入的 XML；之后的改动来自编辑器本身，不能再回灌。
    const initialXmlRef = useRef(xml);
    const callbacksRef = useRef({ onChange, onSave, onLog });
    callbacksRef.current = { onChange, onSave, onLog };

    useEffect(() => {
        let loaded = false;
        const post = (message: unknown) => iframeRef.current?.contentWindow?.postMessage(JSON.stringify(message), "*");
        const handleMessage = (event: MessageEvent) => {
            if (event.source !== iframeRef.current?.contentWindow) return;
            if (typeof event.data !== "string" || event.data.length === 0) return;
            let message: { event?: string; xml?: unknown };
            try {
                message = JSON.parse(event.data);
            } catch {
                return;
            }
            if (message.event === "configure") {
                post({ action: "configure", config: { defaultFonts: ["Humor Sans", "Microsoft YaHei", "SimHei"] } });
            } else if (message.event === "init" && !loaded) {
                loaded = true;
                post({ action: "load", xml: initialXmlRef.current, autosave: 1 });
                callbacksRef.current.onLog?.("Draw.io 图表已加载");
            } else if (message.event === "autosave" && typeof message.xml === "string") {
                callbacksRef.current.onChange?.(message.xml);
            } else if (message.event === "save" && typeof message.xml === "string") {
                callbacksRef.current.onChange?.(message.xml);
                callbacksRef.current.onSave?.(message.xml);
            }
        };
        window.addEventListener("message", handleMessage);
        return () => window.removeEventListener("message", handleMessage);
    }, []);

    return (
        <iframe
            ref={iframeRef}
            src={DRAWIO_EMBED_URL}
            className="flex-1 w-full min-h-0 border-0 bg-background"
            sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox allow-downloads"
            title="Draw.io"
        />
    );
}
