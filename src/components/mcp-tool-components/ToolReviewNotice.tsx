import React, { useEffect } from "react";
import { Loader2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { noteToolReview, useToolReview } from "@/hooks/toolReviewStore";

interface ToolReviewLog {
    verdict?: string;
    reason?: string;
}

const verdictLabel = (verdict: string): string => {
    switch (verdict) {
        case "safe":
            return "审核通过";
        case "risky":
            return "审核认为有风险";
        case "error":
            return "自动审核失败";
        default:
            return "自动审核";
    }
};

export const ToolReviewNotice: React.FC<{ callId?: number | null }> = ({ callId }) => {
    const review = useToolReview(callId);

    useEffect(() => {
        if (!callId) {
            return;
        }
        let cancelled = false;
        invoke<ToolReviewLog | null>("get_tool_review", { callId })
            .then((loaded) => {
                if (cancelled || !loaded?.verdict) {
                    return;
                }
                noteToolReview(callId, {
                    phase: "done",
                    callId,
                    verdict: loaded.verdict,
                    reason: loaded.reason?.trim() || "",
                });
            })
            .catch(() => undefined);
        return () => {
            cancelled = true;
        };
    }, [callId]);

    if (!review) {
        return null;
    }

    if (review.phase === "reviewing") {
        return (
            <div className="mt-2 flex items-center gap-1.5 rounded-md border border-border bg-muted px-2 py-1.5 text-xs text-foreground">
                <Loader2 className="h-3 w-3 animate-spin" />
                <span className="font-medium">自动审核中</span>
            </div>
        );
    }

    return (
        <div className="mt-2 rounded-md border border-border bg-muted px-2 py-1.5 text-xs text-foreground">
            <span className="font-medium">{verdictLabel(review.verdict)}</span>
            {review.reason ? <span className="text-muted-foreground">：{review.reason}</span> : null}
        </div>
    );
};
