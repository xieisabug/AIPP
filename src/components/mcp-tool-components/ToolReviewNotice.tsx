import React from "react";
import { Loader2 } from "lucide-react";
import { useToolReview } from "@/hooks/toolReviewStore";

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
