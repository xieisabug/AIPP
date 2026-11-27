import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import type { ButlerReport, PublishedButlerReport } from "@/data/Conversation";
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
export function LiveReportBubbles({ reports = [], published = [] }: { reports?: ButlerReport[]; published?: PublishedButlerReport[] }) {
    const savedIds = new Set(published.map(report => report.request_id));
    // 原生工具活动和 MCP 回执可能比交互记录先到，此时按所属消息和原文逐次匹配。
    const provisional = published.filter(report => !report.request_id);
    return reports.map((report, index) => {
        if (report.request_id && savedIds.has(report.request_id)) return null;
        const match = provisional.findIndex(row => row.text === report.text
            && (report.anchor == null ? row.currentTurn : row.anchor === report.anchor));
        if (match < 0) return <ReportBubble key={report.request_id ?? index} text={report.text} />;
        provisional.splice(match, 1);
        return null;
    });
}

export default function ButlerProgressRow({ conversationId, publishedReports }: { conversationId: number; publishedReports?: PublishedButlerReport[] }) {
    const progress = useButlerProgress(conversationId);
    const live = useButlerLiveStatus(conversationId);
    return <>
        <LiveReportBubbles reports={progress?.reports} published={publishedReports} />
        {progress?.helper ? <HelperSummaryRow summary={progress.helper} />
            : <div role="status" data-aipp-slot="butler-processing"
                className="butler-exchange flex justify-center text-xs text-muted-foreground">{live}</div>}
    </>;
}

export function HelperSummaryRow({ summary, memberName }: { summary: ButlerProgressSummary; memberName?: string }) {
    const elapsed = useElapsedSeconds();
    return <div role="status" data-aipp-slot="butler-processing"
        className={`butler-exchange flex items-center text-xs text-muted-foreground ${memberName ? "justify-start" : "justify-center"}`}>
        <div className={`flex max-w-full items-center gap-1.5 py-1 ${memberName ? "" : "px-2"}`}>
            <Loader2 aria-hidden="true" className="size-3 shrink-0 animate-spin" />
            {memberName && <>
                <span className="max-w-[40%] shrink-0 truncate font-medium" title={memberName}>{memberName}</span>
                <span aria-hidden="true">·</span>
            </>}
            <span className="truncate">{summary.text}</span>
            <span className="shrink-0 tabular-nums opacity-70">· {formatElapsed(elapsed)}</span>
        </div>
    </div>;
}
