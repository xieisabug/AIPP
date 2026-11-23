import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { staticPreviewDocument } from "@/mobile/previewDocument";
import { acquireChoiceBuild, type BuildView } from "./choiceBuildViews";
import { projectHtml, projectImage } from "./projectHtml";
import mermaidUrl from "mermaid/dist/mermaid.min.js?url";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import { renderToStaticMarkup } from "react-dom/server";
import remarkGfm from "remark-gfm";
import { Button } from "@/components/ui/button";
import PreviewExternalResourcesDialog from "@/components/PreviewExternalResourcesDialog";
import type { PreviewCodeRequestEvent } from "@/utils/previewCode";
import { hasActionablePreviewResources } from "@/utils/previewExternalResources";
import type { Candidate, ChoiceCard } from "@/utils/presentChoices";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import runtimeUrl from "./projectRuntime.ts?worker&url";
import { Code, Eye } from "lucide-react";
import { cn } from "@/utils/utils";
import { ChoiceSegmented, type SegmentOption } from "./ChoiceControls";

const PROJECT_VIEWS: SegmentOption<"preview" | "source">[] = [
    { value: "preview", label: "预览", icon: <Eye /> },
    { value: "source", label: "工程源码", icon: <Code /> },
];

export type ChoiceRef = { conversationId: number; requestId: string; candidateId: string };
type CollectionRef = { collectionId: number; conversationId?: never; requestId?: never; candidateId?: never };
export type PreviewRef = (ChoiceRef & { collectionId?: never }) | CollectionRef;
export const archivePrefix = "AIPP_CHOICE_ARCHIVE_V1\n";
type Files = Record<string, string>;
const marker = "AIPP_CHOICE_PREVIEW_V1\n";
export function previewReference(ref: ChoiceRef) {
    return marker + JSON.stringify(ref);
}
export function readPreviewReference(code: string): ChoiceRef | null {
    if (!code.startsWith(marker)) return null;
    try {
        const ref = JSON.parse(code.slice(marker.length));
        return Number.isSafeInteger(ref.conversationId) &&
            typeof ref.requestId === "string" &&
            typeof ref.candidateId === "string"
            ? ref
            : null;
    } catch {
        return null;
    }
}
export function SavedChoicePreview({ reference }: { reference: ChoiceRef }) {
    const [candidate, setCandidate] = useState<Candidate>();
    const [error, setError] = useState("");
    useEffect(() => {
        let active = true;
        setCandidate(undefined);
        setError("");
        invoke<ChoiceCard>("get_present_choices", {
            conversationId: reference.conversationId,
            requestId: reference.requestId,
        })
            .then((r) => {
                const c = r?.request.candidates.find((item) => item.id === reference.candidateId);
                if (!c) throw Error("候选快照不存在");
                if (active) setCandidate(c);
            })
            .catch((e) => {
                if (active) setError(String(e));
            });
        return () => {
            active = false;
        };
    }, [reference.conversationId, reference.requestId, reference.candidateId]);
    return error ? (
        <p role="alert">{error}</p>
    ) : candidate ? (
        <ChoicePreview candidate={candidate} reference={reference} />
    ) : (
        <p>正在读取快照…</p>
    );
}
export function CollectedChoicePreview({ collectionId }: { collectionId: number }) {
    const [snapshot, setSnapshot] = useState<{ candidate: Candidate; files: Files | null }>();
    const [error, setError] = useState("");
    useEffect(() => {
        let active = true;
        setSnapshot(undefined);
        setError("");
        void invoke<{ candidate: Candidate; files: Files | null }>("get_collected_choice", { collectionId })
            .then(value => { if (active) setSnapshot(value); })
            .catch(reason => { if (active) setError(String(reason)); });
        return () => { active = false; };
    }, [collectionId]);
    if (error) return <p role="alert">{error}</p>;
    if (!snapshot) return <p>正在读取收藏快照…</p>;
    return <ChoicePreview candidate={snapshot.candidate} reference={{ collectionId }} projectFiles={snapshot.files ?? undefined} />;
}
function PreparedFrame({ ready, scripts }: { ready: PreviewCodeRequestEvent; scripts: boolean }) {
    const [src, setSrc] = useState(""),
        [error, setError] = useState("");
    useEffect(() => {
        let active = true,
            token: string | undefined;
        const close = (id: string) => void invoke("close_choice_document", { token: id }).catch(console.error);
        const allowed = ready.externalResources?.resources.filter((r) => r.status === "allowed") ?? [];
        const code = ready.code.replace(/aipp-preview:\/\/localhost\/([0-9a-f]{64})/g, (_, id: string) =>
            convertFileSrc(id, "aipp-preview")
        );
        setSrc("");
        setError("");
        void invoke<string>("create_choice_document", { html: code, allowed, scripts })
            .then((id) => {
                if (active) {
                    token = id;
                    setSrc(convertFileSrc(id, "aipp-choice"));
                } else close(id);
            })
            .catch((e) => {
                if (active) setError(String(e));
            });
        return () => {
            active = false;
            if (token) close(token);
        };
    }, [ready, scripts]);
    return error ? (
        <p role="alert">{error}</p>
    ) : src ? (
        <iframe
            title={ready.title}
            className="h-80 w-full border-0 bg-white"
            sandbox={scripts ? "allow-scripts" : ""}
            referrerPolicy="no-referrer"
            src={src}
        />
    ) : null;
}
function MarkupPreview({ candidate, reference, files }: { candidate: Candidate; reference: PreviewRef; files?: Files }) {
    const mobile = useIsMobilePlatform();
    const code = useMemo(() => {
        if (candidate.type !== "markdown") return candidate.content || " ";
        const entry = candidate.entry_file;
        return renderToStaticMarkup(
            <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                skipHtml
                components={files && entry ? {
                    img: ({ src, alt, title }) => <img loading="lazy" alt={alt} title={title}
                        src={typeof src === "string" ? projectImage(files, entry, src) : undefined} />,
                } : undefined}
            >
                {candidate.content}
            </ReactMarkdown>
        );
    }, [candidate.content, candidate.type, candidate.entry_file, files]);
    const mobileDocument = useMemo(() => mobile ? staticPreviewDocument(code) : "", [mobile, code]);
    const [ready, setReady] = useState<PreviewCodeRequestEvent>();
    const [error, setError] = useState("");
    const [auth, setAuth] = useState(false);
    useEffect(() => {
        let active = true;
        setReady(undefined);
        setError("");
        if (mobile) return;
        invoke<PreviewCodeRequestEvent>("prepare_preview_code_request_for_ui", {
            conversationId: reference.conversationId,
            request: { title: candidate.title, renderer: "html", code: code || " " },
        })
            .then((r) => {
                if (active) setReady(r);
            })
            .catch((e) => {
                if (active) setError(String(e));
            });
        return () => {
            active = false;
        };
    }, [code, candidate.title, reference.conversationId, mobile]);
    if (mobile) return <iframe title={candidate.title} sandbox="" referrerPolicy="no-referrer"
        className="h-80 w-full border-0 bg-white" srcDoc={mobileDocument} />;
    if (error) return <p role="alert">{error}</p>;
    if (!ready) return <p>正在准备预览…</p>;
    return (
        <>
            {hasActionablePreviewResources(ready.externalResources) && (
                <Button variant="outline" size="sm" onClick={() => setAuth(true)}>
                    加载外部资源
                </Button>
            )}
            <PreviewExternalResourcesDialog<PreviewCodeRequestEvent, unknown>
                open={auth}
                onOpenChange={setAuth}
                externalResources={ready.externalResources}
                onAuthorized={(r) => {
                    if (r.previewCode) setReady(r.previewCode);
                }}
            />
            <PreparedFrame ready={ready} scripts={candidate.type !== "markdown"} />
        </>
    );
}
function source(files: Files, name: string) {
    return new TextDecoder().decode(Uint8Array.from(atob(files[name]), (c) => c.charCodeAt(0)));
}
function DrawioPreview({ candidate }: { candidate: Candidate }) {
    const [allowed, setAllowed] = useState(false);
    const frame = useRef<HTMLIFrameElement>(null);
    useEffect(() => {
        if (!allowed) return;
        function message(event: MessageEvent) {
            if (event.source !== frame.current?.contentWindow || event.origin !== "https://embed.diagrams.net")
                return;
            try {
                if (JSON.parse(event.data).event === "init") {
                    frame.current.contentWindow?.postMessage(
                        JSON.stringify({ action: "load", xml: candidate.content }),
                        event.origin
                    );
                }
            } catch {
                /* Ignore unrelated viewer messages. */
            }
        }
        window.addEventListener("message", message);
        return () => window.removeEventListener("message", message);
    }, [allowed, candidate.content]);
    return allowed ? (
        <iframe
            ref={frame}
            title={candidate.title}
            sandbox="allow-scripts allow-same-origin"
            referrerPolicy="no-referrer"
            className="h-96 w-full border-0"
            src="https://embed.diagrams.net/?embed=1&ui=min&proto=json&noSaveBtn=1&noExitBtn=1"
        />
    ) : (
        <Button size="sm" variant="outline" onClick={() => setAllowed(true)}>
            允许将此图交给 diagrams.net 预览
        </Button>
    );
}
function ProjectRuntimeFrame({ title, doc, files, entry }: { title: string; doc: string; files: Files; entry: string }) {
    const frame = useRef<HTMLIFrameElement>(null);
    const delivered = useRef(false);
    const [error, setError] = useState("");
    useLayoutEffect(() => {
        if (delivered.current) return;
        const timer = setTimeout(() => setError("工程预览运行时未响应，请重新打开预览。"), 15000);
        function ready(event: MessageEvent) {
            const target = frame.current?.contentWindow;
            if (!target || event.source !== target || event.data?.type !== "aipp-choice-project-ready" || delivered.current) return;
            delivered.current = true;
            clearTimeout(timer);
            setError("");
            target.postMessage({ type: "aipp-choice-project", files, entry }, "*");
        }
        window.addEventListener("message", ready);
        return () => {
            clearTimeout(timer);
            window.removeEventListener("message", ready);
        };
    }, [entry, files]);
    return <>
        {error && <p role="alert">{error}</p>}
        <iframe ref={frame} title={title} className="h-96 w-full border-0 bg-white"
            sandbox="allow-scripts" referrerPolicy="no-referrer" srcDoc={doc} />
    </>;
}
function ProjectPreview({ candidate, reference, projectFiles }: { candidate: Candidate; reference: PreviewRef; projectFiles?: Files }) {
    const [loadedFiles, setFiles] = useState<Files>();
    const files = projectFiles ?? loadedFiles;
    const [error, setError] = useState("");
    const [showSource, setShowSource] = useState(false);
    const [file, setFile] = useState(candidate.entry_file || "");
    const mobile = useIsMobilePlatform();
    const [buildView, setBuildView] = useState<BuildView | null>(null);
    const [building, setBuilding] = useState(false);
    const [buildError, setBuildError] = useState("");
    const [output, setOutput] = useState("dist");
    const [generation, setGeneration] = useState(0);
    useEffect(() => {
        if (mobile) return;
        let active = true;
        setBuildView(null);
        setBuildError("");
        const view = acquireChoiceBuild(reference, generation > 0);
        void view.ready.then((result) => {
            if (active) setBuildView(result);
        }).catch((e) => {
            if (active) setBuildError(String(e));
        });
        return () => {
            active = false;
            view.release();
        };
    }, [reference.conversationId, reference.requestId, reference.candidateId, reference.collectionId, generation, mobile]);
    useEffect(() => {
        let active = true;
        if (reference.collectionId !== undefined) return;
        invoke<Files>("get_choice_project", reference)
            .then((r) => {
                if (active) setFiles(r);
            })
            .catch((e) => {
                if (active) setError(String(e));
            });
        return () => {
            active = false;
        };
    }, [reference.conversationId, reference.requestId, reference.candidateId, reference.collectionId]);
    const html = useMemo(() => {
        if (files && candidate.entry_file && /\.html?$/i.test(candidate.entry_file)) {
            try {
                return projectHtml(files, candidate.entry_file);
            } catch {
                return undefined;
            }
        }
    }, [files, candidate.entry_file]);
    if (error) return <p role="alert">{error}</p>;
    if (!files) return <p>正在读取工程…</p>;
    const url = new URL(runtimeUrl, window.location.href).href;
    const diagram = /\.(?:mmd|mermaid)$/i.test(candidate.entry_file || "")
        ? new URL(mermaidUrl, window.location.href).href
        : "";
    const doc = `<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src ${url} ${diagram} 'unsafe-eval'; img-src data: blob:; style-src 'unsafe-inline'; font-src data:; form-action 'none'; base-uri 'none'"><div id="root"></div><pre id="error" role="alert"></pre>${diagram ? `<script src="${diagram}"></script>` : ""}<script src="${url}"></script>`;
    return (
        <>
            <div className="mb-2 flex flex-wrap items-center gap-2">
                <ChoiceSegmented
                    label="工程视图"
                    value={showSource ? "source" : "preview"}
                    options={PROJECT_VIEWS}
                    onChange={(view) => setShowSource(view === "source")}
                />
                {showSource && (
                    <select
                        aria-label="工程文件"
                        className={cn("max-w-full rounded-md border bg-background px-2 text-sm", mobile ? "h-11" : "h-8")}
                        value={file}
                        onChange={(e) => setFile(e.target.value)}
                    >
                        {Object.keys(files).map((p) => (
                            <option key={p}>{p}</option>
                        ))}
                    </select>
                )}
            </div>
            {!mobile && !buildView && files["package.json"] && (
                <details className="my-2">
                    <summary>工程构建</summary>
                    <label>
                        构建输出目录{" "}
                        <input
                            className="w-32 rounded border px-1"
                            value={output}
                            onChange={(e) => setOutput(e.target.value)}
                        />
                    </label>
                    <Button
                        size="sm"
                        disabled={building || !output.trim()}
                        onClick={async () => {
                            setBuilding(true);
                            setBuildError("");
                            try {
                                await invoke(reference.collectionId !== undefined ? "build_collected_choice" : "build_choice_project", { ...reference, outputDir: output });
                                setGeneration((v) => v + 1);
                                setShowSource(false);
                            } catch (e) {
                                setBuildError(String(e));
                            } finally {
                                setBuilding(false);
                            }
                        }}
                    >
                        {building ? "正在构建…" : "用 Bun 安装依赖并构建"}
                    </Button>
                </details>
            )}
            {buildError && <p role="alert">{buildError}</p>}
            {showSource ? (
                <pre className="max-h-96 overflow-auto whitespace-pre-wrap">
                    {files[file] !== undefined ? source(files, file) : ""}
                </pre>
            ) : buildView ? (
                <iframe
                    title={candidate.title}
                    sandbox="allow-scripts"
                    referrerPolicy="no-referrer"
                    src={buildView.url}
                    className="h-96 w-full border-0 bg-white"
                />
            ) : html !== undefined ? (
                <MarkupPreview candidate={{ ...candidate, type: "html", content: html }} reference={reference} files={files} />
            ) : /\.drawio$/i.test(candidate.entry_file || "") ? (
                mobile ? <p>此 Drawio 工程可在桌面端授权预览，也可切换到源码视图查看文件。</p> : <DrawioPreview candidate={candidate} />
            ) : /\.(md|markdown)$/i.test(candidate.entry_file || "") ? (
                <MarkupPreview candidate={{ ...candidate, type: "markdown" }} reference={reference} files={files} />
            ) : !/\.(?:html?|[cm]?[jt]sx?|vue|svg|mmd|mermaid|txt)$/i.test(candidate.entry_file || "") ? (
                <pre className="max-h-96 overflow-auto whitespace-pre-wrap">{candidate.content}</pre>
            ) : (
                <ProjectRuntimeFrame title={candidate.title} doc={doc} files={files} entry={candidate.entry_file!} />
            )}
        </>
    );
}
export function ChoicePreview({ candidate, reference, projectFiles }: { candidate: Candidate; reference: PreviewRef; projectFiles?: Files }) {
    const [error, setError] = useState("");
    const [busy, setBusy] = useState(false);
    if (reference.collectionId !== undefined && candidate.content == null) return <p role="alert">收藏缺少候选内容。</p>;
    if (candidate.url && candidate.content == null)
        return (
            <div>
                <p className="break-all">{candidate.url}</p>
                <Button
                    disabled={busy}
                    onClick={async () => {
                        setBusy(true);
                        try {
                            await invoke("authorize_choice_source", reference);
                        } catch (e) {
                            setError(String(e));
                        } finally {
                            setBusy(false);
                        }
                    }}
                >
                    允许读取此来源
                </Button>
                {error && <p role="alert">{error}</p>}
            </div>
        );
    if (candidate.type === "artifact") return <ProjectPreview key={JSON.stringify(reference)} candidate={candidate} reference={reference} projectFiles={projectFiles} />;
    if (candidate.type === "image")
        return (
            <img
                className="max-h-96 max-w-full object-contain"
                alt={candidate.title}
                src={candidate.content?.startsWith("data:image/") ? candidate.content : undefined}
            />
        );
    if (candidate.type === "html" || candidate.type === "markdown")
        return <MarkupPreview candidate={candidate} reference={reference} />;
    return <pre className="max-h-96 overflow-auto whitespace-pre-wrap">{candidate.content}</pre>;
}
