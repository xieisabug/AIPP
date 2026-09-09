import { Check, FileText, Loader2, RotateCcw } from "lucide-react";
import UnifiedMarkdown from "@/components/UnifiedMarkdown";
import { Button } from "@/components/ui/button";

interface AgentPlanCardProps {
    content: string;
    isStreaming: boolean;
    canAct: boolean;
    modeSwitching: boolean;
    onContinuePlanning?: () => void;
    onApprove?: () => void;
}

export function AgentPlanCard({
    content,
    isStreaming,
    canAct,
    modeSwitching,
    onContinuePlanning,
    onApprove,
}: AgentPlanCardProps) {
    return (
        <div
            className="w-full max-w-3xl overflow-hidden rounded-2xl border border-border/80 bg-card shadow-sm"
            data-aipp-slot="chat-agent-plan-card"
        >
            <div className="flex items-center justify-between border-b bg-muted/20 px-5 py-3.5">
                <div className="flex items-center gap-2.5">
                    <div className="rounded-md bg-primary/10 p-1.5 text-primary">
                        <FileText className="h-4 w-4" />
                    </div>
                    <span className="font-semibold tracking-tight">Plan</span>
                </div>
                {isStreaming ? (
                    <span className="flex items-center gap-1 text-xs text-muted-foreground">
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        生成中
                    </span>
                ) : null}
            </div>

            <div className="px-5 py-5">
                <div className="prose prose-sm max-w-none break-words text-foreground">
                    <UnifiedMarkdown noProseWrapper>{content}</UnifiedMarkdown>
                </div>
            </div>

            {canAct ? (
                <div className="flex flex-wrap items-center justify-between gap-3 border-t bg-muted/10 px-5 py-3.5">
                    <span className="text-xs text-muted-foreground">
                        确认后退出 Plan 模式，并发送“通过”
                    </span>
                    <div className="flex items-center gap-2">
                        <Button type="button" variant="outline" size="sm" onClick={onContinuePlanning}>
                            <RotateCcw className="mr-1.5 h-3.5 w-3.5" />
                            继续完善
                        </Button>
                        <Button type="button" size="sm" disabled={modeSwitching} onClick={onApprove}>
                            <Check className="mr-1.5 h-3.5 w-3.5" />
                            {modeSwitching ? "切换中" : "通过"}
                        </Button>
                    </div>
                </div>
            ) : null}
        </div>
    );
}
