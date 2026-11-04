import React, { useMemo } from "react";
import type { AgentActivityEvent } from "@/data/Conversation";
import { CommandActivityCard } from "./agent-activity/CommandActivityCard";
import { PatchActivityCard } from "./agent-activity/PatchActivityCard";
import { ToolCallActivityCard } from "./agent-activity/ToolCallActivityCard";
import { GenericActivityCard, SubAgentActivityCard } from "./agent-activity/SubAgentActivityCard";

/** 按活动类型分发到专用卡片组件，与 McpToolCallRenderer 的分发模式一致 */
const renderActivityCard = (activity: AgentActivityEvent): React.ReactNode => {
    switch (activity.kind) {
        case "command":
            return <CommandActivityCard activity={activity} />;
        case "patch":
            return <PatchActivityCard activity={activity} />;
        case "tool":
            return <ToolCallActivityCard activity={activity} />;
        case "sub_agent":
            return <SubAgentActivityCard activity={activity} />;
        default:
            return <GenericActivityCard activity={activity} />;
    }
};

/** 这些 item 已在气泡里直接展示（用户输入、agent 正文），不生成活动卡片；后端已跳过发送，这里兜底历史数据 */
const HIDDEN_ACTIVITY_KINDS = new Set(["userMessage", "agentMessage"]);

export function isHiddenAgentActivity(activity: AgentActivityEvent): boolean {
    return HIDDEN_ACTIVITY_KINDS.has(activity.kind);
}

class AgentActivityErrorBoundary extends React.Component<
    { fallback: React.ReactNode; children: React.ReactNode },
    { hasError: boolean }
> {
    state = { hasError: false };

    static getDerivedStateFromError() {
        return { hasError: true };
    }

    componentDidCatch(error: unknown) {
        console.warn("[AgentActivityList] activity card failed, falling back to generic", error);
    }

    render() {
        return this.state.hasError ? this.props.fallback : this.props.children;
    }
}

export function AgentActivityList({ activities }: { activities: AgentActivityEvent[] }) {
    const ordered = useMemo(
        () => activities
            .filter((activity) => !isHiddenAgentActivity(activity))
            .sort((a, b) => a.sequence - b.sequence),
        [activities],
    );
    if (ordered.length === 0) return null;
    return (
        <div className="flex w-full flex-col gap-2" data-agent-activity-list>
            {ordered.map((activity) => (
                <AgentActivityErrorBoundary
                    key={`${activity.agent_kind}:${activity.session_id ?? ""}:${activity.item_id}`}
                    fallback={<GenericActivityCard activity={activity} />}
                >
                    {renderActivityCard(activity)}
                </AgentActivityErrorBoundary>
            ))}
        </div>
    );
}
