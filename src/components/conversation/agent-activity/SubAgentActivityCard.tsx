import { Bot, Wrench } from "lucide-react";

import { JsonDisplay } from "@/components/McpToolCall";
import type { AgentActivityEvent } from "@/data/Conversation";
import { AgentActivityShell } from "./AgentActivityShell";

interface ActivityCardProps {
  activity: AgentActivityEvent;
}

function ActivitySections({ activity }: ActivityCardProps) {
  return (
    <>
      {activity.output && (
        <div className="max-w-full overflow-hidden">
          <span className="text-xs font-medium mb-1 text-muted-foreground">
            输出:
          </span>
          <JsonDisplay content={activity.output} maxHeight="288px" className="mt-1" />
        </div>
      )}
      {activity.error && (
        <div className="max-w-full overflow-hidden">
          <span className="text-xs font-medium mb-1 text-muted-foreground">
            错误:
          </span>
          <JsonDisplay content={activity.error} maxHeight="200px" className="mt-1" />
        </div>
      )}
    </>
  );
}

function hasDetails(activity: AgentActivityEvent): boolean {
  return Boolean(activity.output) || Boolean(activity.error);
}

/**
 * 子 Agent 活动卡片：Bot 图标标识，展开后可查看事件原始输出。
 */
export function SubAgentActivityCard({ activity }: ActivityCardProps) {
  return (
    <AgentActivityShell
      icon={<Bot className="h-4 w-4 flex-shrink-0" />}
      sourceLabel="Codex"
      title={activity.title ?? "子 Agent"}
      status={activity.status}
    >
      {hasDetails(activity) ? <ActivitySections activity={activity} /> : undefined}
    </AgentActivityShell>
  );
}

/**
 * 通用兜底卡片：用于未知 kind 或专用卡片渲染失败时的降级展示。
 */
export function GenericActivityCard({ activity }: ActivityCardProps) {
  return (
    <AgentActivityShell
      icon={<Wrench className="h-4 w-4 flex-shrink-0" />}
      sourceLabel="Codex"
      title={activity.title ?? activity.kind}
      status={activity.status}
    >
      {hasDetails(activity) ? <ActivitySections activity={activity} /> : undefined}
    </AgentActivityShell>
  );
}
