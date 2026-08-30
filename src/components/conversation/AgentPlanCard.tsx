import { CheckCircle2, Circle, ListChecks, Loader2 } from "lucide-react";
import type { AcpPlanEntry } from "@/data/Conversation";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
    DialogTrigger,
} from "@/components/ui/dialog";
import { cn } from "@/utils/utils";

export type AgentPlanStatus =
    | "planning"
    | "awaiting_confirmation"
    | "executing"
    | "completed";

interface AgentPlanCardProps {
    plan: AcpPlanEntry[];
    explanation?: string | null;
    hasActivePrompt: boolean;
    isPlanMode: boolean;
    modeSwitching: boolean;
    onContinuePlanning: () => void;
    onStartExecution: () => void;
}

export function getAgentPlanStatus(
    plan: AcpPlanEntry[],
    hasActivePrompt: boolean,
    isPlanMode: boolean,
): AgentPlanStatus {
    if (plan.length > 0 && plan.every((entry) => entry.status === "completed")) {
        return "completed";
    }
    if (hasActivePrompt) {
        return isPlanMode ? "planning" : "executing";
    }
    return isPlanMode ? "awaiting_confirmation" : "executing";
}

const STATUS_LABELS: Record<AgentPlanStatus, string> = {
    planning: "规划中",
    awaiting_confirmation: "待确认",
    executing: "执行中",
    completed: "已完成",
};

function PlanStepIcon({ status }: { status: AcpPlanEntry["status"] }) {
    if (status === "completed") {
        return <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-green-500" />;
    }
    if (status === "in_progress") {
        return <Loader2 className="mt-0.5 h-4 w-4 shrink-0 animate-spin text-blue-500" />;
    }
    return <Circle className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />;
}

function PlanSteps({ plan, compact = false }: { plan: AcpPlanEntry[]; compact?: boolean }) {
    const visiblePlan = compact ? plan.slice(0, 3) : plan;
    return (
        <div className="space-y-2">
            {visiblePlan.map((entry, index) => (
                <div key={`${entry.content}:${index}`} className="flex items-start gap-2 text-sm">
                    <PlanStepIcon status={entry.status} />
                    <span className={cn(entry.status === "completed" && "text-muted-foreground line-through") }>
                        {entry.content}
                    </span>
                </div>
            ))}
            {compact && plan.length > visiblePlan.length ? (
                <div className="text-xs text-muted-foreground">还有 {plan.length - visiblePlan.length} 个步骤</div>
            ) : null}
        </div>
    );
}

export function AgentPlanCard({
    plan,
    explanation,
    hasActivePrompt,
    isPlanMode,
    modeSwitching,
    onContinuePlanning,
    onStartExecution,
}: AgentPlanCardProps) {
    if (plan.length === 0) return null;

    const status = getAgentPlanStatus(plan, hasActivePrompt, isPlanMode);
    const completed = plan.filter((entry) => entry.status === "completed").length;
    const canAct = !hasActivePrompt && status !== "completed" && !modeSwitching;

    return (
        <div className="mx-auto w-full max-w-3xl rounded-xl border bg-card p-4 shadow-sm" data-aipp-slot="chat-agent-plan-card">
            <div className="flex items-start justify-between gap-3">
                <div className="flex min-w-0 items-start gap-3">
                    <div className="mt-0.5 rounded-lg bg-primary/10 p-2 text-primary">
                        <ListChecks className="h-4 w-4" />
                    </div>
                    <div className="min-w-0 space-y-1">
                        <div className="flex items-center gap-2">
                            <span className="font-medium">Plan</span>
                            <Badge variant="secondary">{STATUS_LABELS[status]}</Badge>
                        </div>
                        <div className="text-xs text-muted-foreground">
                            {completed}/{plan.length} 个步骤已完成
                        </div>
                    </div>
                </div>
            </div>

            {explanation ? <p className="mt-3 text-sm text-muted-foreground">{explanation}</p> : null}
            <div className="mt-3">
                <PlanSteps plan={plan} compact />
            </div>

            <div className="mt-4 flex flex-wrap items-center gap-2">
                <Dialog>
                    <DialogTrigger asChild>
                        <Button type="button" variant="outline" size="sm">查看完整 Plan</Button>
                    </DialogTrigger>
                    <DialogContent className="max-h-[80vh] max-w-2xl overflow-y-auto">
                        <DialogHeader>
                            <DialogTitle className="flex items-center gap-2">
                                Plan
                                <Badge variant="secondary">{STATUS_LABELS[status]}</Badge>
                            </DialogTitle>
                            <DialogDescription>
                                {explanation || `${completed}/${plan.length} 个步骤已完成`}
                            </DialogDescription>
                        </DialogHeader>
                        <PlanSteps plan={plan} />
                    </DialogContent>
                </Dialog>
                {status !== "completed" ? (
                    <>
                        <Button type="button" variant="ghost" size="sm" disabled={!canAct} onClick={onContinuePlanning}>
                            继续完善
                        </Button>
                        <Button type="button" size="sm" disabled={!canAct} onClick={onStartExecution}>
                            {modeSwitching ? "切换中" : "开始执行"}
                        </Button>
                    </>
                ) : null}
            </div>
        </div>
    );
}
