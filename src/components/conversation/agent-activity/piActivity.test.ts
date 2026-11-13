import { describe, expect, it } from "vitest";
import type { AgentActivityEvent } from "@/data/Conversation";
import { normalizePiActivity } from "./piActivity";

const oldActivity = (overrides: Partial<AgentActivityEvent> = {}): AgentActivityEvent => ({
  conversation_id: 1, response_message_id: 2, agent_kind: "pi", item_id: "read", sequence: 3,
  kind: "tool", status: "success", title: "read", input: { path: "a.txt" }, metadata: {},
  ...overrides,
});

describe("normalizePiActivity", () => {
  it.each([
    ["bash", "command", "echo hi"], ["powershell", "command", "echo hi"],
    ["read", "file_read", "读取 a.txt"], ["grep", "file_search", "搜索 needle a.txt"],
    ["find", "file_search", "搜索 needle a.txt"], ["ls", "file_search", "列出目录 needle a.txt"],
    ["edit", "patch", "编辑 a.txt"], ["write", "file_write", "写入 a.txt"], ["custom", "tool", "custom"],
  ])("should classify old records when the native tool is %s", (tool, kind, title) => {
    const original = oldActivity({ title: tool, input: { path: "a.txt", pattern: "needle", command: "echo hi" } });
    const converted = normalizePiActivity(original);
    expect(converted).toMatchObject({ kind, title, item_id: original.item_id, sequence: original.sequence, status: "success" });
    expect(original.kind).toBe("tool");
    expect(original.metadata).toEqual({});
  });

  it("should keep original results and extract actual diffs when replaying old edits", () => {
    const result = { content: [{ type: "text", text: "done" }], details: { diff: "+new" } };
    const converted = normalizePiActivity(oldActivity({ title: "edit", output: JSON.stringify(result) }));
    expect(converted.output).toBe("done");
    expect(converted.metadata).toMatchObject({ rawResult: result, changes: [{ path: "a.txt", kind: "update", diff: "+new" }] });
  });

  it("should preserve verbatim non-JSON output and remove fake null output when reading old records", () => {
    expect(normalizePiActivity(oldActivity({ output: "plain text" })).output).toBe("plain text");
    expect(normalizePiActivity(oldActivity({ output: "null" })).output).toBeUndefined();
    expect(normalizePiActivity(oldActivity({ title: "write" })).metadata).not.toHaveProperty("changes");
  });

  it("should leave existing types and other agents alone when no legacy conversion is needed", () => {
    for (const original of [oldActivity({ agent_kind: "cursor" }), oldActivity({ kind: "command" }), oldActivity({ metadata: { tool: "custom" } })]) {
      expect(normalizePiActivity(original)).toBe(original);
    }
  });
});
