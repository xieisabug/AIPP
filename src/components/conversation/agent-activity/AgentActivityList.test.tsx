import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentActivityEvent } from "@/data/Conversation";
import { AgentActivityList } from "../AgentActivityList";

vi.mock("@/components/magicui/shine-border", () => ({
  ShineBorder: () => <div data-testid="shine-border" />,
}));

vi.mock("@/hooks/useTheme", () => ({
  useTheme: () => ({ resolvedTheme: "light" }),
}));

let commandShouldThrow = false;
vi.mock("./CommandActivityCard", () => ({
  CommandActivityCard: ({ activity }: { activity: AgentActivityEvent }) => {
    if (commandShouldThrow) {
      throw new Error("boom");
    }
    return <div data-testid="command-card">{activity.title}</div>;
  },
}));

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

describe("AgentActivityList", () => {
  beforeEach(() => {
    commandShouldThrow = false;
  });

  it("should dispatch activities to dedicated cards by kind", () => {
    render(
      <AgentActivityList
        activities={[
          buildActivity({ kind: "command", title: "cargo check" }),
          buildActivity({
            kind: "patch",
            item_id: "item-2",
            sequence: 2,
            metadata: { changes: [{ path: "a.ts", kind: "add" }] },
          }),
          buildActivity({
            kind: "tool",
            item_id: "item-3",
            sequence: 3,
            metadata: { server: "aipp:operation", tool: "read_file" },
          }),
          buildActivity({
            kind: "mystery",
            item_id: "item-4",
            sequence: 4,
            title: "mystery",
          }),
        ]}
      />,
    );

    expect(screen.getByTestId("command-card")).toBeInTheDocument();
    expect(screen.getByText("文件变更（1 个文件）")).toBeInTheDocument();
    expect(screen.getByText("aipp:operation")).toBeInTheDocument();
    // 未知 kind 落到通用兜底卡片
    expect(screen.getByText("mystery")).toBeInTheDocument();
  });

  it("should order activities by sequence", () => {
    const { container } = render(
      <AgentActivityList
        activities={[
          buildActivity({ item_id: "item-b", sequence: 2, title: "second" }),
          buildActivity({ item_id: "item-a", sequence: 1, title: "first" }),
        ]}
      />,
    );

    const cards = container.querySelectorAll("[data-testid='command-card']");
    expect(cards[0].textContent).toBe("first");
    expect(cards[1].textContent).toBe("second");
  });

  it("should render nothing when there are no activities", () => {
    const { container } = render(<AgentActivityList activities={[]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("should hide userMessage and agentMessage activities", () => {
    const { container } = render(
      <AgentActivityList
        activities={[
          buildActivity({ kind: "userMessage", title: "userMessage" }),
          buildActivity({ kind: "agentMessage", item_id: "item-2", sequence: 2, title: "agentMessage" }),
        ]}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("should fall back to generic card when a dedicated card crashes", () => {
    commandShouldThrow = true;
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    render(
      <AgentActivityList
        activities={[buildActivity({ title: "cargo check", output: "out" })]}
      />,
    );

    // 崩溃后回退到 GenericActivityCard，仍能看到标题与输出
    expect(screen.getByText("cargo check")).toBeInTheDocument();
    expect(screen.getByText("out")).toBeInTheDocument();

    warnSpy.mockRestore();
    errorSpy.mockRestore();
  });
});
