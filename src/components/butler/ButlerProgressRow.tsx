import { useEffect, useState } from "react";
import { Check, ChevronRight, Loader2, X } from "lucide-react";
import { useButlerProgress, type ButlerProgressSummary } from "./butlerProgress";
import { useButlerLiveStatus } from "./useButlerLiveStatus";

function useElapsedSeconds() {
    const [start] = useState(() => Date.now());
    const [now, setNow] = useState(start);
    useEffect(() => {
        const timer = setInterval(() => setNow(Date.now()), 1000);
        return () => clearInterval(timer);
    }, []);
    return Math.floor((now - start) / 1000);
}

const formatElapsed = (seconds: number) => seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`;

/** 简洁聊天中的状态行。有辅助模型摘要时显示一句进展，可展开查看步骤；否则与联系人列表用同一句短状态。 */
export default function ButlerProgressRow({ conversationId }: { conversationId: number }) {
    const progress = useButlerProgress(conversationId);
    const live = useButlerLiveStatus(conversationId);
    return <>
        {progress?.reports.map(text => <div key={text} className="butler-peer-bubble w-fit max-w-full break-words">{text}</div>)}
        {progress?.helper ? <HelperSummaryRow summary={progress.helper} />
            : <div role="status" data-aipp-slot="butler-processing"
                className="butler-exchange flex justify-center text-xs text-muted-foreground">{live}</div>}
    </>;
}

export function HelperSummaryRow({ summary }: { summary: ButlerProgressSummary }) {
    const elapsed = useElapsedSeconds();
    const [expanded, setExpanded] = useState(false);
    const status = <>
            <Loader2 aria-hidden="true" className="size-3 shrink-0 animate-spin" />
            <span className="truncate">{summary.text}</span>
            <span className="shrink-0 tabular-nums opacity-70">· {formatElapsed(elapsed)}</span>
    </>;
    const rowClass = "flex max-w-full items-center gap-1.5 px-2 py-1";
    return <div role="status" data-aipp-slot="butler-processing" className="butler-exchange flex flex-col items-center text-xs text-muted-foreground">
        {summary.steps.length ? <button type="button" onClick={() => setExpanded(!expanded)} aria-expanded={expanded}
            className={`${rowClass} rounded-md hover:bg-muted hover:text-foreground`}>
            {status}
            <ChevronRight aria-hidden="true" className={`size-3 shrink-0 transition-transform ${expanded ? "rotate-90" : ""}`} />
        </button> : <div className={rowClass}>{status}</div>}
        {expanded && summary.steps.length > 0 && <ol className="mt-1 space-y-0.5" data-aipp-slot="butler-progress-steps">
            {summary.steps.map((step, index) => <li key={index} className="flex max-w-md items-center gap-1.5">
                {step.status === "success" ? <Check aria-label="已完成" className="size-3 shrink-0" />
                    : step.status === "failed" ? <X aria-label="失败" className="size-3 shrink-0" />
                    : <Loader2 aria-label="进行中" className="size-3 shrink-0 animate-spin" />}
                <span className="truncate font-mono text-[11px]">{step.label}</span>
            </li>)}
        </ol>}
    </div>;
}
