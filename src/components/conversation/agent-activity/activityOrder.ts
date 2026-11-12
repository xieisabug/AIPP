import type { AgentActivityEvent } from "@/data/Conversation";

const NATIVE_DRIVERS = new Set(["pi", "cursor", "opencode", "kiro", "antigravity"]);

/** Native sequence counts updates, never use it to move an existing operation. */
export function orderAgentActivities(activities: AgentActivityEvent[]): AgentActivityEvent[] {
  return activities.map((activity, index) => ({
    activity,
    order: activity.order ?? (NATIVE_DRIVERS.has(activity.agent_kind) ? index : activity.sequence),
  })).sort((a, b) => a.order - b.order).map(({ activity }) => activity);
}
