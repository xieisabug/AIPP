import { describe, expect, it } from "vitest";

import type { AgentActivityEvent } from "@/data/Conversation";
import { buildActivitySegments } from "./activitySegments";

const buildActivity = (
  overrides: Partial<AgentActivityEvent>,
): AgentActivityEvent => ({
  conversation_id: 1,
  response_message_id: 2,
  agent_kind: "codex_app_server",
  session_id: "thread-1",
  item_id: "item-1",
  sequence: 1,
  kind: "command",
  status: "executing",
  ...overrides,
});

describe("buildActivitySegments", () => {
  it("should return null when there are no activities", () => {
    expect(buildActivitySegments("hello", [])).toBeNull();
  });

  it("should return null when no activity has a valid offset", () => {
    const activities = [
      buildActivity({ content_offset: null }),
      buildActivity({ item_id: "item-2", content_offset: 999 }),
    ];
    expect(buildActivitySegments("hello", activities)).toBeNull();
  });

  it("should interleave activities between text segments in offset order", () => {
    const content = "先做甲再做乙最后收尾";
    const activities = [
      buildActivity({ item_id: "item-b", sequence: 2, content_offset: 6 }),
      buildActivity({ item_id: "item-a", sequence: 1, content_offset: 3 }),
    ];

    const segments = buildActivitySegments(content, activities);

    expect(segments).not.toBeNull();
    expect(segments!.map((segment) => segment.text)).toEqual([
      "先做甲",
      "再做乙",
      "最后收尾",
    ]);
    expect(segments![0].activities.map((a) => a.item_id)).toEqual(["item-a"]);
    expect(segments![1].activities.map((a) => a.item_id)).toEqual(["item-b"]);
    expect(segments![2].activities).toEqual([]);
  });

  it("should group activities that share the same offset", () => {
    const activities = [
      buildActivity({ item_id: "item-b", sequence: 2, content_offset: 2 }),
      buildActivity({ item_id: "item-a", sequence: 1, content_offset: 2 }),
    ];

    const segments = buildActivitySegments("正文内容", activities);

    expect(segments).toHaveLength(2);
    expect(segments![0].text).toBe("正文");
    expect(segments![0].activities.map((a) => a.item_id)).toEqual(["item-a", "item-b"]);
    expect(segments![1].text).toBe("内容");
  });

  it("should append activities without valid offset after all text", () => {
    const activities = [
      buildActivity({ item_id: "item-new", sequence: 1, content_offset: 2 }),
      buildActivity({ item_id: "item-legacy", sequence: 2, content_offset: null }),
    ];

    const segments = buildActivitySegments("正文内容", activities);

    expect(segments).toHaveLength(2);
    expect(segments![1].text).toBe("内容");
    expect(segments![1].activities.map((a) => a.item_id)).toEqual(["item-legacy"]);
  });

  it("should count offsets by unicode code point", () => {
    // "🙂" 是一个 code point，与 Rust chars().count() 口径一致
    const segments = buildActivitySegments("🙂好", [
      buildActivity({ content_offset: 1 }),
    ]);

    expect(segments).not.toBeNull();
    expect(segments![0].text).toBe("🙂");
    expect(segments![1].text).toBe("好");
  });

  it("should not mutate the input activities array", () => {
    const activities = [
      buildActivity({ item_id: "item-b", sequence: 2, content_offset: 2 }),
      buildActivity({ item_id: "item-a", sequence: 1, content_offset: 1 }),
    ];

    buildActivitySegments("abc", activities);

    expect(activities.map((a) => a.item_id)).toEqual(["item-b", "item-a"]);
  });
});
