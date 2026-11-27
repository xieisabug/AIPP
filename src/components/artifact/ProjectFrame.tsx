import { useEffect, useMemo, useRef, useState } from "react";
import mermaidUrl from "mermaid/dist/mermaid.min.js?url";
import runtimeUrl from "@/components/choices/projectRuntime.ts?worker&url";
import { projectHtml } from "@/components/choices/projectHtml";
import { randomNonce } from "@/mobile/previewDocument";
import { inlineConsoleScript, injectConsoleScript } from "./artifactRuntime";

/** 文件名 → base64 原始字节（与比选工程快照同格式）。 */
export type ProjectFiles = Record<string, string>;

interface ProjectFrameProps {
    title: string;
    files: ProjectFiles;
    entry: string;
}

/**
 * 多文件 Artifact 版本的预览：
 * - HTML 入口：把同工程的样式、脚本、图片内联后按普通 HTML 加载；
 * - 组件/文档入口：交给比选工程同款的浏览器内运行时（Babel + Vue 编译器，不经 Bun），
 *   运行时只提供 react、react-dom/client、vue 与工程内文件。
 * 两种方式都先注入诊断脚本，报错进查看器的控制台。
 */
export default function ProjectFrame({ title, files, entry }: ProjectFrameProps) {
    const html = useMemo(() => {
        if (!/\.html?$/i.test(entry)) return undefined;
        try {
            return projectHtml(files, entry);
        } catch {
            return undefined;
        }
    }, [files, entry]);

    if (html !== undefined) {
        return (
            <iframe
                srcDoc={injectConsoleScript(html)}
                className="flex-1 w-full min-h-0 border-0 bg-white"
                sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                title={title}
            />
        );
    }
    return <RuntimeFrame title={title} files={files} entry={entry} />;
}

function RuntimeFrame({ title, files, entry }: ProjectFrameProps) {
    const frameRef = useRef<HTMLIFrameElement>(null);
    const [nonce] = useState(randomNonce);
    const [error, setError] = useState("");

    const doc = useMemo(() => {
        const runtime = new URL(runtimeUrl, window.location.href).href;
        const diagram = /\.(?:mmd|mermaid)$/i.test(entry) ? new URL(mermaidUrl, window.location.href).href : "";
        const csp = `default-src 'none'; script-src ${runtime} ${diagram} 'nonce-${nonce}' 'unsafe-eval'; img-src data: blob:; style-src 'unsafe-inline'; font-src data:; form-action 'none'; base-uri 'none'`;
        return (
            `<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${csp}">` +
            `<script nonce="${nonce}">${inlineConsoleScript()}</script>` +
            `<style>html,body{margin:0}#error:empty{display:none}</style><div id="root"></div><pre id="error" role="alert"></pre>` +
            (diagram ? `<script src="${diagram}"></script>` : "") +
            `<script src="${runtime}"></script>`
        );
    }, [entry, nonce]);

    // 运行时就绪后把工程交给它；每次文件或入口变化都会换一个新 iframe（key）
    useEffect(() => {
        let delivered = false;
        const timer = setTimeout(() => setError("工程预览运行时未响应，请重新加载。"), 15000);
        const handleReady = (event: MessageEvent) => {
            const target = frameRef.current?.contentWindow;
            if (!target || event.source !== target || event.data?.type !== "aipp-choice-project-ready" || delivered) {
                return;
            }
            delivered = true;
            clearTimeout(timer);
            setError("");
            target.postMessage({ type: "aipp-choice-project", files, entry }, "*");
        };
        window.addEventListener("message", handleReady);
        return () => {
            clearTimeout(timer);
            window.removeEventListener("message", handleReady);
        };
    }, [files, entry]);

    return (
        <>
            {error && (
                <p role="alert" className="px-3 py-2 text-sm text-destructive">
                    {error}
                </p>
            )}
            <iframe
                ref={frameRef}
                srcDoc={doc}
                className="flex-1 w-full min-h-0 border-0 bg-white"
                sandbox="allow-scripts"
                referrerPolicy="no-referrer"
                title={title}
            />
        </>
    );
}

/** 解码工程里的文本文件。 */
export function readProjectFile(files: ProjectFiles, name: string): string {
    const encoded = files[name];
    if (encoded === undefined) return "";
    return new TextDecoder().decode(Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0)));
}
