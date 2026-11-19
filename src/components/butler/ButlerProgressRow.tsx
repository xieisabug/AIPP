import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
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

export function ReportBubble({ text }: { text: string }) {
    return <div data-aipp-slot="butler-report" className="butler-peer-bubble w-fit max-w-full whitespace-pre-line break-words">{text}</div>;
}

/** 已经进入时间线的每次成功上报，只抵消实时快照中的一次。 */
export function LiveReportBubbles({ reports = [], published = [] }: { reports?: string[]; published?: string[] }) {
    const savedCounts = new Map<string, number>();
    for (const text of published) savedCounts.set(text, (savedCounts.get(text) ?? 0) + 1);
    return reports.map((text, index) => {
        const saved = savedCounts.get(text) ?? 0;
        if (!saved) return <ReportBubble key={index} text={text} />;
        savedCounts.set(text, saved - 1);
        return null;
    });
}

export default function ButlerProgressRow({ conversationId, publishedReports }: { conversationId: number; publishedReports?: string[] }) {
    const progress = useButlerProgress(conversationId);
    const live = useButlerLiveStatus(conversationId);
    return <>
        <LiveReportBubbles reports={progress?.reports} published={publishedReports} />
        {progress?.helper ? <HelperSummaryRow summary={progress.helper} />
            : <div role="status" data-aipp-slot="butler-processing"
                className="butler-exchange flex justify-center text-xs text-muted-foreground">{live}</div>}
    </>;
}

export function HelperSummaryRow({ summary }: { summary: ButlerProgressSummary }) {
    const elapsed = useElapsedSeconds();
    return <div role="status" data-aipp-slot="butler-processing" className="butler-exchange flex items-center justify-center text-xs text-muted-foreground">
        <div className="flex max-w-full items-center gap-1.5 px-2 py-1">
            <Loader2 aria-hidden="true" className="size-3 shrink-0 animate-spin" />
            <span className="truncate">{summary.text}</span>
            <span className="shrink-0 tabular-nums opacity-70">· {formatElapsed(elapsed)}</span>
        </div>
    </div>;
}
