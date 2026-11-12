import { describe, expect, it } from "vitest";
import type { AgentActivityEvent } from "@/data/Conversation";
import { orderAgentActivities } from "./activityOrder";

const item = (item_id: string, overrides: Partial<AgentActivityEvent> = {}): AgentActivityEvent => ({
  conversation_id: 1, response_message_id: 2, agent_kind: "pi", item_id, sequence: 1,
  kind: "command", status: "executing", ...overrides,
});

describe("orderAgentActivities", () => {
  it("should keep first-seen order when one item receives many updates", () => {
    const thought = item("think", { order: 0, kind: "reasoning", sequence: 100 });
    const tool = item("tool", { order: 1, sequence: 2 });
    const input = [tool, thought];
    expect(orderAgentActivities(input).map(a => a.item_id)).toEqual(["think", "tool"]);
    expect(input).toEqual([tool, thought]);
  });

  it.each(["pi", "cursor", "opencode", "kiro", "antigravity"])("should preserve recorded order for old %s items without position data", agent_kind => {
    const input = [item("think", { agent_kind, sequence: 100 }), item("tool", { agent_kind, sequence: 2 })];
    expect(orderAgentActivities(input)).toEqual(input);
  });

  it("should retain existing sequence ordering when the provider has no native per-item revisions", () => {
    const input = [item("second", { agent_kind: "codex_app_server", sequence: 2 }), item("first", { agent_kind: "codex_app_server", sequence: 1 })];
    expect(orderAgentActivities(input).map(a => a.item_id)).toEqual(["first", "second"]);
  });
});
