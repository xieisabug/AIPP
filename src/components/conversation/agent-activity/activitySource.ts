import type { AgentActivityEvent } from "@/data/Conversation";

const AGENT_NAMES: Record<string, string> = {
  codex_app_server: "Codex",
  claude_sdk: "Claude Code",
  acp: "ACP",
  pi: "Pi",
  cursor: "Cursor",
  opencode: "OpenCode",
  kiro: "Kiro",
  antigravity: "Antigravity",
};

export function activitySource(activity: AgentActivityEvent): string {
  return Object.hasOwn(AGENT_NAMES, activity.agent_kind)
    ? AGENT_NAMES[activity.agent_kind]
    : activity.agent_kind?.trim() || "Agent";
}
