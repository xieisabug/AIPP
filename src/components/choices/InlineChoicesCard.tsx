import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import {
    ArrowLeftRight,
    BookmarkPlus,
    Check,
    ChevronDown,
    ChevronUp,
    Columns2,
    Copy,
    Diff,
    ExternalLink,
    LayoutGrid,
    Loader2,
    Maximize2,
    Monitor,
    PanelRightOpen,
    PanelTop,
    RefreshCw,
    Smartphone,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { useIsMobile } from "@/hooks/use-mobile";
import { cn } from "@/utils/utils";
import { toast } from "sonner";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";
import {
    diffChoices,
    partialChoices,
    type ChoiceCard,
    type ChoiceDecision,
    type ChoiceRequest,
    type Candidate,
} from "@/utils/presentChoices";
import { ChoicePreview, previewReference } from "./ChoicePreview";
import { ChoiceIconButton, ChoiceSegmented, ChoiceToggle, type SegmentOption } from "./ChoiceControls";
import SaveArtifactDialog from "@/components/SaveArtifactDialog";

type Layout = ChoiceRequest["layout"];
const LAYOUTS: SegmentOption<Layout>[] = [
    { value: "tabs", label: "标签页", icon: <PanelTop /> },
    { value: "split", label: "并排", icon: <Columns2 /> },
    { value: "grid", label: "网格", icon: <LayoutGrid /> },
];
const VIEWPORTS: SegmentOption<"desktop" | "phone">[] = [
    { value: "desktop", label: "桌面宽度", icon: <Monitor /> },
    { value: "phone", label: "手机宽度", icon: <Smartphone /> },
];

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
    const [layout, setLayout] = useState<Layout>();
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
    const unready = candidates.some((c) => c.content == null);
    const touch = mobile ? "min-h-11" : undefined;
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
    /** 单个候选的连带动作，候选卡片头部与已选摘要共用。 */
    function candidateActions(c: Candidate) {
        if (!record || c.content == null) return null;
        const ref = { conversationId: cid!, requestId: record.request_id, candidateId: c.id };
        return (
            <div className="flex shrink-0 items-center">
                <ChoiceIconButton
                    label={c.type === "artifact" ? "复制入口源码" : "复制"}
                    onClick={() => void run(() => writeText(c.content!), "已复制")}
                >
                    <Copy />
                </ChoiceIconButton>
                {["html", "markdown"].includes(c.type) && (
                    <ChoiceIconButton label="收藏" onClick={() => setSave(c)}>
                        <BookmarkPlus />
                    </ChoiceIconButton>
                )}
                {c.type === "artifact" && (
                    <ChoiceIconButton
                        label="发布到侧边栏"
                        onClick={() => void run(() => invoke("publish_choice_project", ref), "已发布到侧边栏")}
                    >
                        <PanelRightOpen />
                    </ChoiceIconButton>
                )}
                {!mobile && (
                    <ChoiceIconButton
                        label="独立窗口打开"
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
                        <ExternalLink />
                    </ChoiceIconButton>
                )}
            </div>
        );
    }
    function candidateAt(i: number) {
        const c = candidates[i];
        if (!c)
            return (
                <p key={i} className="flex items-center gap-1.5 text-sm text-muted-foreground">
                    <Loader2 className="size-3 animate-spin" />
                    {title(i)} · 等待生成…
                </p>
            );
        const ref = { conversationId: cid!, requestId: record?.request_id ?? "", candidateId: c.id };
        const picked = chosen.includes(c.id);
        return (
            <article
                key={i}
                className={cn("min-w-0 space-y-2 rounded-md border p-3", picked && "border-foreground/40")}
            >
                <div className="flex min-w-0 items-center gap-2">
                    <label className={cn("flex min-w-0 flex-1 items-center gap-2", actions && "cursor-pointer")}>
                        {actions && (
                            <input
                                type={req.selection_mode === "multiple" ? "checkbox" : "radio"}
                                className="size-4 shrink-0 accent-primary"
                                aria-label={"选择 " + title(i)}
                                checked={picked}
                                onChange={() => select(c.id)}
                            />
                        )}
                        <strong className="min-w-0 break-words">{title(i)}</strong>
                        {picked && !actions && (
                            <span className="inline-flex shrink-0 items-center gap-0.5 text-xs text-muted-foreground">
                                <Check className="size-3" />
                                已选
                            </span>
                        )}
                    </label>
                    {candidateActions(c)}
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
            </article>
        );
    }
    function picker(label: string, value: number, change: (n: number) => void) {
        return (
            <select
                aria-label={label}
                className={cn("max-w-48 rounded-md border bg-background px-2 text-sm", mobile ? "h-11" : "h-8")}
                value={value}
                onChange={(e) => change(Number(e.target.value))}
            >
                {Array.from({ length: count }, (_, i) => (
                    <option key={i} value={i}>
                        {title(i)}
                    </option>
                ))}
            </select>
        );
    }
    function candidateTabs() {
        const pending = (i: number) => !record && !terminal && i >= (partial.completedCount ?? 0);
        return (
            <div
                role="tablist"
                aria-label="候选版本"
                className="flex max-w-full gap-0.5 overflow-x-auto rounded-md bg-muted p-0.5"
            >
                {Array.from({ length: count }, (_, i) => {
                    const id = candidates[i]?.id;
                    return (
                        <button
                            type="button"
                            role="tab"
                            key={i}
                            aria-selected={i === active}
                            onClick={() => setActive(i)}
                            className={cn(
                                "inline-flex shrink-0 items-center gap-1 rounded px-3 text-sm text-muted-foreground transition-colors hover:text-foreground",
                                mobile ? "h-10" : "h-7",
                                i === active && "bg-background text-foreground shadow-sm"
                            )}
                        >
                            {pending(i) ? (
                                <Loader2 className="size-3 animate-spin" />
                            ) : id && chosen.includes(id) ? (
                                <Check className="size-3" />
                            ) : null}
                            {title(i)}
                        </button>
                    );
                })}
            </div>
        );
    }
    function comparison(inDialog: boolean) {
        const canDiff = candidates.filter((c) => ["markdown", "text"].includes(c.type)).length >= 2;
        const visual = candidates.some((c) => c.type !== "text");
        return (
            <div className="space-y-3">
                {req.prompt && <p>{req.prompt}</p>}
                {record?.recovered && actions && (
                    <p className="text-sm text-muted-foreground">
                        这是重启前的请求。提交后，管家将按本次回答继续。
                    </p>
                )}
                {(!narrow || record) && (
                    <div className="flex flex-wrap items-center gap-2">
                        {!narrow && (
                            <ChoiceSegmented label="视图布局" value={mode} options={LAYOUTS} onChange={setLayout} />
                        )}
                        {record && visual && (
                            <ChoiceSegmented
                                label="预览宽度"
                                value={phone ? "phone" : "desktop"}
                                options={VIEWPORTS}
                                onChange={(v) => setPhone(v === "phone")}
                            />
                        )}
                        {record && canDiff && (
                            <ChoiceToggle label="对比差异" pressed={diff} onPressedChange={setDiff}>
                                <Diff />
                            </ChoiceToggle>
                        )}
                        {record && !inDialog && (
                            <div className="ml-auto">
                                <ChoiceIconButton label="全屏对比" onClick={() => setLarge(true)}>
                                    <Maximize2 />
                                </ChoiceIconButton>
                            </div>
                        )}
                    </div>
                )}
                {(diff || mode === "split") && (
                    <div className="flex flex-wrap items-center gap-1">
                        {picker("版本", active, setActive)}
                        <ChoiceIconButton
                            label="交换两侧版本"
                            onClick={() => {
                                setActive(other);
                                setOther(active);
                            }}
                        >
                            <ArrowLeftRight />
                        </ChoiceIconButton>
                        {picker("对照版本", other, setOther)}
                    </div>
                )}
                {mode === "tabs" && !diff && candidateTabs()}
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
                    <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
                        <Loader2 className="size-3 animate-spin" />
                        {!props.isStreaming
                            ? "正在准备候选…"
                            : req.expected_count || req.candidate_titles?.length
                              ? `已生成 ${partial.completedCount ?? 0} / ${count}`
                              : "正在生成下一个方案…"}
                    </p>
                )}
                {actions && (
                    <div className="flex flex-wrap items-center gap-2">
                        {req.allow_feedback && (
                            <Input
                                className={cn("min-w-48 flex-1", mobile && "h-11")}
                                aria-label="补充意见"
                                placeholder="补充意见（选填）"
                                value={feedback}
                                onChange={(e) => setFeedback(e.target.value)}
                            />
                        )}
                        <div className="ml-auto flex flex-wrap gap-2">
                            <Button
                                className={touch}
                                variant="ghost"
                                disabled={busy}
                                onClick={() => void submit("cancelled")}
                            >
                                取消
                            </Button>
                            <Button
                                className={touch}
                                variant="outline"
                                disabled={busy || unready}
                                onClick={() => void submit("rejected")}
                            >
                                <RefreshCw />
                                重新生成
                            </Button>
                            <Button
                                className={touch}
                                disabled={busy || !selected.length || unready}
                                onClick={() => void submit("selected")}
                            >
                                <Check />
                                确认选择{selected.length > 1 ? `（${selected.length}）` : ""}
                            </Button>
                        </div>
                    </div>
                )}
            </div>
        );
    }
    return (
        <section className="my-2 min-w-0 space-y-2 rounded-lg border bg-card p-3" aria-label="候选方案对比">
            <div className="flex items-center gap-2">
                <strong className="min-w-0 flex-1 break-words">{req.title || "选择方案"}</strong>
                {done && (
                    <ChoiceIconButton
                        label={expanded ? "收起候选" : `展开全部候选（${count}）`}
                        onClick={() => setExpanded(!expanded)}
                    >
                        {expanded ? <ChevronUp /> : <ChevronDown />}
                    </ChoiceIconButton>
                )}
            </div>
            {done && (
                <div className="space-y-1 text-sm">
                    {done.status === "selected" ? (
                        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                            <span className="text-muted-foreground">已选择</span>
                            {candidates
                                .filter((c) => chosen.includes(c.id))
                                .map((c) => (
                                    <span key={c.id} className="inline-flex min-w-0 items-center gap-1">
                                        <span className="font-medium">{c.title}</span>
                                        {!expanded && candidateActions(c)}
                                    </span>
                                ))}
                        </div>
                    ) : (
                        <p className="text-muted-foreground">
                            {done.status === "rejected" ? "已要求重新生成" : "已取消"}
                        </p>
                    )}
                    {done.feedback && <p className="text-muted-foreground">补充意见：{done.feedback}</p>}
                </div>
            )}
            {(error || terminal) && (
                <p role="alert" className="text-destructive">
                    {error || terminal}
                </p>
            )}
            {(!done || expanded) && !large && comparison(false)}
            {large && (
                <Dialog open onOpenChange={setLarge}>
                    <DialogContent className="max-h-[96dvh] overflow-auto sm:max-w-[95vw]">
                        <DialogTitle>{req.title}</DialogTitle>
                        {comparison(true)}
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
