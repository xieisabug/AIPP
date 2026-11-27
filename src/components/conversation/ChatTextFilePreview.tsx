import { lazy, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PreviewFileContent, PreviewFileResources, type PreviewFileRequest } from "@/components/InlineInteractionCards";
import { injectConsoleScript } from "@/components/artifact/artifactRuntime";
import mermaidUrl from "mermaid/dist/mermaid.min.js?url";
const ArtifactViewer = lazy(() => import("@/components/artifact/ArtifactViewer"));
const PreparedFrame = lazy(() =>
    import("@/components/choices/ChoicePreview").then((m) => ({ default: m.PreparedFrame }))
);
const ReadOnlyDrawioPreview = lazy(() =>
    import("@/components/choices/ChoicePreview").then((m) => ({ default: m.ReadOnlyDrawioPreview }))
);

function LinkedMermaidFrame({ content, title }: { content: string; title: string }) {
    const srcDoc = useMemo(() => {
        const script = new URL(mermaidUrl, window.location.href).href;
        const diagram = content.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
        return `<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src ${script}; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'"><style>body{margin:0;min-height:100vh;display:grid;place-items:center}pre{max-width:100%}</style><pre class="mermaid">${diagram}</pre><script src="${script}"></script>`;
    }, [content]);
    return (
        <iframe
            title={title}
            sandbox="allow-scripts"
            referrerPolicy="no-referrer"
            srcDoc={srcDoc}
            className="min-h-0 w-full flex-1 border-0 bg-white"
        />
    );
}

export default function ChatTextFilePreview(props: { path: string; language: string; conversationId?: number }) {
    const { path, language, conversationId } = props;
    const [request, setRequest] = useState<PreviewFileRequest>();
    const [error, setError] = useState<string>();
    const [source, setSource] = useState("");
    useEffect(() => {
        let active = true;
        setRequest(undefined);
        setError(undefined);
        invoke<PreviewFileRequest & { source?: string }>("prepare_chat_text_file_preview", {
            path,
            language,
            conversationId,
        }).then(
            (result) => {
                if (active) {
                    setSource(result.source ?? "");
                    setRequest(result);
                }
            },
            (reason) => {
                if (active) setError(String(reason));
            }
        );
        return () => {
            active = false;
        };
    }, [path, language, conversationId]);

    const file = request?.files[0];
    const prepared = useMemo(
        () => ({
            code:
                file?.language === "html"
                    ? injectConsoleScript(file.content ?? "")
                    : file?.language === "svg"
                    ? `<!doctype html><meta charset="utf-8"><style>body{margin:0;display:grid;place-items:center;min-height:100vh}svg{max-width:100%;max-height:100vh}</style>${
                          file.content ?? ""
                      }`
                    : "",
            externalResources: request?.externalResources,
        }),
        [file, request?.externalResources]
    );
    if (error)
        return (
            <p role="alert" className="p-4 text-destructive">
                {error}
            </p>
        );
    if (!request) return <p>正在读取文件…</p>;
    if (!file) return <p role="alert">预览请求中缺少文件。</p>;
    const type = file.language;
    if (type === "html" || type === "svg" || type === "mermaid" || type === "drawio") {
        const override =
            type === "drawio" ? (
                <ReadOnlyDrawioPreview key={request.request_id} title={file.title} content={file.content} fill />
            ) : type === "html" || type === "svg" ? (
                <PreparedFrame ready={prepared} scripts={type === "html"} fill title={file.title} />
            ) : type === "mermaid" ? (
                <LinkedMermaidFrame content={file.content ?? ""} title={file.title} />
            ) : undefined;
        return (
            <ArtifactViewer
                type={type}
                title={file.title}
                code={type === "html" || type === "svg" ? source : file.content ?? ""}
                contentKey={request.request_id}
                buildLogs={[]}
                previewOverride={override}
                actions={<PreviewFileResources request={request} onChange={setRequest} />}
            />
        );
    }
    return (
        <>
            <PreviewFileResources request={request} onChange={setRequest} />
            <PreviewFileContent file={file} expandedCode />
        </>
    );
}
