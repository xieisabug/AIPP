import { useState } from "react";
import type { AgentActivityEvent, Message } from "@/data/Conversation";
import { maskContent } from "@/utils/antiLeakage";
import { ReasoningDisplay } from "../ReasoningDisplay";
import type { ReasoningMessageProps } from "../ReasoningMessage";
import { AgentActivityList, isHiddenAgentActivity } from "./AgentActivityList";
import { orderAgentActivities } from "./agent-activity/activityOrder";

interface Props extends Pick<ReasoningMessageProps, "conversationId" | "mcpToolCallStates" | "shiningMcpCallId" | "inlineInteractionItems" | "useRawTextRenderer"> {
  activities: AgentActivityEvent[];
  message: Message;
  responseComplete: boolean;
}

function AgentReasoning({ activity, message, responseComplete, ...context }: Omit<Props, "activities"> & { activity: AgentActivityEvent }) {
  const [expanded, setExpanded] = useState(false);
  const done = responseComplete || ["success", "failed", "cancelled"].includes(activity.status);
  const start = activity.started_at ? new Date(activity.started_at) : null;
  const finish = activity.finished_at ? new Date(activity.finished_at) : null;
  const content = activity.output ?? "";
  const reasoning: Message = {
    ...message, message_type: "reasoning", content,
    start_time: start, finish_time: done ? finish : null,
  };
  return (
    <ReasoningDisplay
      {...context}
      embedded
      message={reasoning}
      streamEvent={{ message_id: message.id, message_type: "reasoning", content, is_done: done,
        duration_ms: start && finish ? Math.max(0, finish.getTime() - start.getTime()) : undefined }}
      displayedContent={context.useRawTextRenderer ? maskContent(content) : content}
      isReasoningExpanded={expanded}
      onToggleReasoningExpand={() => setExpanded(value => !value)}
    />
  );
}

/** Preserve one timeline while keeping thoughts outside tool-card containers. */
export function AgentActivityTranscript({ activities, ...context }: Props) {
  const groups: Array<{ first: AgentActivityEvent; tools: AgentActivityEvent[] }> = [];
  for (const activity of orderAgentActivities(activities)) {
    if (isHiddenAgentActivity(activity)) continue;
    const last = groups.at(-1);
    if (last && activity.kind !== "reasoning" && last.first.kind !== "reasoning") last.tools.push(activity);
    else groups.push({ first: activity, tools: activity.kind === "reasoning" ? [] : [activity] });
  }
  if (!groups.length) return null;
  return (
    <div className="flex w-full flex-col gap-2" data-agent-transcript>
      {groups.map(({ first, tools }) => first.kind === "reasoning"
        ? <AgentReasoning key={`${first.agent_kind}:${first.session_id}:${first.item_id}`} activity={first} {...context} />
        : <AgentActivityList key={`${first.agent_kind}:${first.session_id}:${first.item_id}`} activities={tools} />)}
    </div>
  );
}
