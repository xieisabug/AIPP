import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { useIsMobile } from "@/hooks/use-mobile";
import { toast } from "sonner";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";
import {
    diffChoices,
    partialChoices,
    type ChoiceCard,
    type ChoiceDecision,
    type Candidate,
} from "@/utils/presentChoices";
import { ChoicePreview, previewReference } from "./ChoicePreview";
import SaveArtifactDialog from "@/components/SaveArtifactDialog";

export default function InlineChoicesCard(props: McpToolComponentProps & { requestId?: string }) {
    const cid = props.conversationId ?? props.currentToolCall?.conversation_id;
    const callId = props.callId ?? props.currentToolCall?.call_id;
    const [record, setRecord] = useState<ChoiceCard | null>(null);
    const [error, setError] = useState("");
    const [busy, setBusy] = useState(false);
    const [selected, setSelected] = useState<string[]>([]);
    const [feedback, setFeedback] = useState("");
    const [expanded, setExpanded] = useState(false);
    const [active, setActive] = useState(0);
    const [other, setOther] = useState(1);
    const [layout, setLayout] = useState<string>();
    const [diff, setDiff] = useState(false);
    const [large, setLarge] = useState(false);
    const [phone, setPhone] = useState(false);
    const [save, setSave] = useState<Candidate>();
    const mobile = useIsMobilePlatform(),
        narrow = useIsMobile();
    const raw = props.parameters ?? props.currentToolCall?.parameters ?? "";
    const partial = useMemo(() => partialChoices(raw), [raw]);
    useEffect(() => {
        let live = true,
            revision = 0;
        setRecord(null);
        setError("");
        setSelected([]);
        setFeedback("");
        setExpanded(false);
        setActive(0);
        setOther(1);
        if (!cid || (!callId && !props.requestId)) return;
        const refresh = async () => {
            const ticket = ++revision;
            try {
                const value = await invoke<ChoiceCard | null>("get_present_choices", {
                    conversationId: cid,
                    requestId: props.requestId,
                    callId,
                });
                if (live && ticket === revision) setRecord(value);
            } catch (e) {
                if (live) setError(String(e));
            }
        };
        const off = listen<number>("present-choices-changed", (e) => {
            if (e.payload === cid) void refresh();
        });
        void off.then(refresh).catch((e) => {
            if (live) setError(String(e));
        });
        return () => {
            live = false;
            void off.then((fn) => fn()).catch(console.error);
        };
    }, [cid, callId, props.requestId]);
    const req = record?.request ?? partial;
    const candidates = Array.isArray(req.candidates) ? req.candidates : [];
    const chosen = record?.decision?.selected_ids ?? selected;
    const done = record?.decision;
    const planned = req.expected_count || req.candidate_titles?.length;
    const count = record
        ? candidates.length
        : Math.min(
              6,
              Math.max(
                  candidates.length,
                  planned ?? 0,
                  !planned && props.isStreaming && partial.completedCount === candidates.length
                      ? candidates.length + 1
                      : 0
              )
          );
    const title = (i: number) => candidates[i]?.title || req.candidate_titles?.[i] || "候选 " + (i + 1);
    const callState = callId ? props.mcpToolCallStates?.get(callId) : undefined;
    const terminal = record
        ? record.error
        : props.error ||
          callState?.error ||
          props.currentToolCall?.error ||
          ((callState?.status ?? props.status) === "failed"
              ? "工具调用在候选快照登记前结束，执行记录未提供错误原因。"
              : undefined);
    const mode = narrow ? "tabs" : (layout ?? req.layout ?? "tabs");
    const current = candidates[active];
    const second = candidates[other];
    const actions = Boolean(record && !done && !terminal);
    const differs =
        current &&
        second &&
        active !== other &&
        [current, second].every((c) => ["text", "markdown"].includes(c.type));
    const diffRows = useMemo(() => diff && differs ? diffChoices(current.content ?? "", second.content ?? "") : [],
        [diff, differs, current?.content, second?.content]);
    async function submit(status: ChoiceDecision["status"]) {
        if (!record || busy) return;
        setBusy(true);
        setError("");
        try {
            await invoke("submit_present_choices_response", {
                conversationId: cid,
                requestId: record.request_id,
                decision: {
                    status,
                    selected_ids: status === "selected" ? selected : [],
                    feedback: req.allow_feedback && status !== "cancelled" ? feedback : undefined,
                },
            });
            const r = await invoke<ChoiceCard>("get_present_choices", {
                conversationId: cid,
                requestId: record.request_id,
            });
            setRecord(r);
            setLarge(false);
            setExpanded(false);
            setDiff(false);
        } catch (e) {
            setError(String(e));
        } finally {
            setBusy(false);
        }
    }
    function select(id: string) {
        if (req.selection_mode === "multiple")
            setSelected((prev) => (prev.includes(id) ? prev.filter((v) => v !== id) : [...prev, id]));
        else setSelected([id]);
    }
    const run = async (fn: () => Promise<unknown>, message?: string) => {
        try {
            await fn();
            if (message) toast.success(message);
        } catch (e) {
            setError(String(e));
        }
    };
    function candidateAt(i: number) {
        const c = candidates[i];
        if (!c)
            return (
                <p key={i} className="animate-pulse text-muted-foreground">
                    {title(i)} · 等待生成…
                </p>
            );
        const ref = { conversationId: cid!, requestId: record?.request_id ?? "", candidateId: c.id };
        return (
            <article key={i} className="min-w-0 space-y-2 rounded-md border p-3">
                <div className="flex items-center gap-2">
                    {actions && (
                        <input
                            type={req.selection_mode === "multiple" ? "checkbox" : "radio"}
                            aria-label={"选择 " + title(i)}
                            checked={chosen.includes(c.id)}
                            onChange={() => select(c.id)}
                        />
                    )}
                    <strong>{title(i)}</strong>
                    {chosen.includes(c.id) && <small>已选</small>}
                </div>
                {c.summary && <p className="text-sm text-muted-foreground">{c.summary}</p>}
                <div className={phone ? "mx-auto max-w-[390px]" : "w-full"}>
                    {record ||
                    (i < (partial.completedCount ?? 0) && c.content != null && c.type !== "artifact") ? (
                        <ChoicePreview key={ref.requestId + c.id} candidate={c} reference={ref} />
                    ) : c.type === "markdown" ? (
                        <ReactMarkdown
                            skipHtml
                            components={{
                                img: ({ alt }) => <span>{alt}</span>,
                                a: ({ children }) => <span>{children}</span>,
                            }}
                        >
                            {c.content ?? ""}
                        </ReactMarkdown>
                    ) : (
                        <pre className="max-h-72 overflow-auto whitespace-pre-wrap">
                            {c.type === "image" || c.type === "artifact"
                                ? "正在准备候选…"
                                : c.content || "生成中…"}
                        </pre>
                    )}
                </div>
                {record && c.content != null && (
                    <div className="flex flex-wrap gap-1">
                        <Button
                            size="sm"
                            variant="ghost"
                            onClick={() => void run(() => writeText(c.content!), "已复制")}
                        >
                            {c.type === "artifact" ? "复制入口源码" : "复制"}
                        </Button>
                        {["html", "markdown"].includes(c.type) && (
                            <Button size="sm" variant="ghost" onClick={() => setSave(c)}>
                                收藏
                            </Button>
                        )}
                        {c.type === "artifact" && (
                            <Button
                                size="sm"
                                variant="ghost"
                                onClick={() =>
                                    void run(() => invoke("publish_choice_project", ref), "已发布到侧边栏")
                                }
                            >
                                发布到侧边栏
                            </Button>
                        )}
                        {!mobile && (
                            <Button
                                size="sm"
                                variant="ghost"
                                onClick={() =>
                                    void run(() =>
                                        invoke("run_artifacts", {
                                            lang: "html",
                                            inputStr: previewReference(ref),
                                            conversationId: cid,
                                        })
                                    )
                                }
                            >
                                独立窗口
                            </Button>
                        )}
                    </div>
                )}
            </article>
        );
    }
    function picker(label: string, value: number, change: (n: number) => void) {
        return (
            <label className="text-sm">
                {label}{" "}
                <select className="max-w-48" value={value} onChange={(e) => change(Number(e.target.value))}>
                    {Array.from({ length: count }, (_, i) => (
                        <option key={i} value={i}>
                            {title(i)}
                        </option>
                    ))}
                </select>
            </label>
        );
    }
    function comparison() {
        return (
            <div className="space-y-3">
                {req.prompt && <p>{req.prompt}</p>}
                {record?.recovered && actions && (
                    <p className="text-sm text-muted-foreground">
                        这是重启前的请求。提交后，管家将按本次回答继续。
                    </p>
                )}
                <div className="flex flex-wrap items-center gap-2">
                    {!narrow &&
                        (["tabs", "split", "grid"] as const).map((v, i) => (
                            <Button
                                key={v}
                                size="sm"
                                variant={mode === v ? "secondary" : "ghost"}
                                onClick={() => setLayout(v)}
                            >
                                {["标签", "并排", "网格"][i]}
                            </Button>
                        ))}
                    {record && (
                        <>
                            <Button size="sm" variant="ghost" onClick={() => setPhone(!phone)}>
                                {phone ? "桌面宽度" : "手机宽度"}
                            </Button>
                            <Button size="sm" variant="ghost" onClick={() => setLarge(!large)}>
                                {large ? "收起大窗" : "全屏对比"}
                            </Button>
                            {candidates.filter((c) => ["markdown", "text"].includes(c.type)).length >= 2 && (
                                <Button
                                    size="sm"
                                    variant={diff ? "secondary" : "ghost"}
                                    onClick={() => setDiff(!diff)}
                                >
                                    对比差异
                                </Button>
                            )}
                        </>
                    )}
                </div>
                {(diff || mode === "split") && (
                    <div className="flex flex-wrap gap-3">
                        {picker("版本", active, setActive)}
                        {picker("对照版本", other, setOther)}
                    </div>
                )}
                {mode === "tabs" && !diff && (
                    <div className="flex flex-wrap gap-1">
                        {Array.from({ length: count }, (_, i) => (
                            <Button
                                size="sm"
                                variant={i === active ? "secondary" : "ghost"}
                                key={i}
                                onClick={() => setActive(i)}
                            >
                                {title(i)}
                            </Button>
                        ))}
                    </div>
                )}
                {diff ? (
                    differs ? (
                        <pre className="overflow-auto whitespace-pre-wrap">
                            {diffRows.map((r, i) => (
                                <div
                                    key={i}
                                    className={
                                        r.kind === "add"
                                            ? "bg-green-500/15"
                                            : r.kind === "remove"
                                              ? "bg-red-500/15"
                                              : ""
                                    }
                                >
                                    {r.kind === "add" ? "+ " : r.kind === "remove" ? "− " : "  "}
                                    {r.parts ? r.parts.map((part, j) => part.kind === "remove"
                                        ? <del key={j} className="bg-red-500/35">{part.text}</del>
                                        : part.kind === "add"
                                          ? <ins key={j} className="bg-green-500/35">{part.text}</ins>
                                          : part.text) : r.text}
                                </div>
                            ))}
                        </pre>
                    ) : (
                        <p>请选择两个不同的文本版本。</p>
                    )
                ) : (
                    <div className={mode === "tabs" ? "" : "grid grid-cols-2 gap-2"}>
                        {(mode === "grid"
                            ? Array.from({ length: count }, (_, i) => i)
                            : mode === "split"
                              ? [...new Set([active, other])]
                              : [active]
                        ).map(candidateAt)}
                    </div>
                )}
                {!record && !terminal && (
                    <small className="text-muted-foreground">
                        {!props.isStreaming
                            ? "正在准备候选…"
                            : req.expected_count || req.candidate_titles?.length
                              ? `已生成 ${partial.completedCount ?? 0} / ${count}`
                              : "正在生成下一个方案…"}
                    </small>
                )}
                {actions && (
                    <>
                        {req.allow_feedback && (
                            <input
                                className="w-full rounded-md border bg-transparent p-2 text-sm"
                                aria-label="补充意见"
                                placeholder="补充意见（选填）"
                                value={feedback}
                                onChange={(e) => setFeedback(e.target.value)}
                            />
                        )}
                        <div className="flex flex-wrap gap-2">
                            <Button
                                disabled={busy || !selected.length || candidates.some((c) => c.content == null)}
                                onClick={() => void submit("selected")}
                            >
                                确认选择{selected.length > 1 ? `（${selected.length}）` : ""}
                            </Button>
                            <Button
                                variant="outline"
                                disabled={busy || candidates.some((c) => c.content == null)}
                                onClick={() => void submit("rejected")}
                            >
                                都不满意，重新生成
                            </Button>
                            <Button variant="ghost" disabled={busy} onClick={() => void submit("cancelled")}>
                                取消
                            </Button>
                        </div>
                    </>
                )}
            </div>
        );
    }
    return (
        <section className="my-2 min-w-0 space-y-2 rounded-lg border bg-card p-3" aria-label="候选方案对比">
            <strong>{req.title || "选择方案"}</strong>
            {done && (
                <>
                    <p>
                        {done.status === "selected"
                            ? "已选择：" +
                              candidates
                                  .filter((c) => chosen.includes(c.id))
                                  .map((c) => c.title)
                                  .join("、")
                            : done.status === "rejected"
                              ? "已要求重新生成"
                              : "已取消"}
                    </p>
                    {done.feedback && <p>{done.feedback}</p>}
                    <Button size="sm" variant="ghost" onClick={() => setExpanded(!expanded)}>
                        {expanded ? "收起候选" : `展开全部候选（${count}）`}
                    </Button>
                </>
            )}
            {(error || terminal) && (
                <p role="alert" className="text-destructive">
                    {error || terminal}
                </p>
            )}
            {done?.status === "selected" &&
                !expanded &&
                candidates
                    .filter((c) => chosen.includes(c.id))
                    .map((c) => (
                        <div key={c.id} className="flex flex-wrap gap-1">
                            <Button
                                size="sm"
                                variant="ghost"
                                onClick={() => void run(() => writeText(c.content ?? ""), "已复制")}
                            >
                                复制{c.title}
                            </Button>
                            {["html", "markdown"].includes(c.type) && (
                                <Button size="sm" variant="ghost" onClick={() => setSave(c)}>
                                    收藏{c.title}
                                </Button>
                            )}
                            {c.type === "artifact" && (
                                <Button
                                    size="sm"
                                    variant="ghost"
                                    onClick={() =>
                                        void run(
                                            () =>
                                                invoke("publish_choice_project", {
                                                    conversationId: cid,
                                                    requestId: record!.request_id,
                                                    candidateId: c.id,
                                                }),
                                            "已发布到侧边栏"
                                        )
                                    }
                                >
                                    发布{c.title}
                                </Button>
                            )}
                        </div>
                    ))}
            {(!done || expanded) && !large && comparison()}
            {large && (
                <Dialog open onOpenChange={setLarge}>
                    <DialogContent className="max-h-[96dvh] overflow-auto sm:max-w-[95vw]">
                        <DialogTitle>{req.title}</DialogTitle>
                        {comparison()}
                    </DialogContent>
                </Dialog>
            )}
            {save && (
                <SaveArtifactDialog
                    isOpen
                    onClose={() => setSave(undefined)}
                    artifactType={save.type}
                    code={save.content ?? ""}
                />
            )}
        </section>
    );
}
