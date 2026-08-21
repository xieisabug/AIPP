import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { AgentActivityEvent } from "@/data/Conversation";
import { CommandActivityCard } from "./CommandActivityCard";
import { PatchActivityCard } from "./PatchActivityCard";
import { ToolCallActivityCard } from "./ToolCallActivityCard";
import {
  GenericActivityCard,
  SubAgentActivityCard,
} from "./SubAgentActivityCard";

vi.mock("@/components/magicui/shine-border", () => ({
  ShineBorder: () => <div data-testid="shine-border" />,
}));

vi.mock("@/hooks/useTheme", () => ({
  useTheme: () => ({ resolvedTheme: "light" }),
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

describe("CommandActivityCard", () => {
  it("should render command in header and auto-expand output while executing", () => {
    render(
      <CommandActivityCard
        activity={buildActivity({
          title: "cargo check",
          metadata: { command: "cargo check", cwd: "/repo" },
          output: "compiling...",
        })}
      />,
    );

    expect(screen.getByText("Codex")).toBeInTheDocument();
    expect(screen.getByText("cargo check")).toBeInTheDocument();
    expect(screen.getByText("cwd: /repo")).toBeInTheDocument();
    expect(screen.getByText("compiling...")).toBeInTheDocument();
  });

  it("should show exit code badge after completion", () => {
    render(
      <CommandActivityCard
        activity={buildActivity({
          status: "success",
          metadata: { command: "ls", exitCode: 0 },
        })}
      />,
    );

    expect(screen.getByText("exit code: 0")).toBeInTheDocument();
  });

  it("should mark non-zero exit code as destructive", () => {
    render(
      <CommandActivityCard
        activity={buildActivity({
          status: "failed",
          metadata: { command: "cargo build", exitCode: 101 },
        })}
      />,
    );

    const badge = screen.getByText("exit code: 101");
    expect(badge.className).toContain("destructive");
  });

  it("should collapse output after success and expand on toggle click", async () => {
    const user = userEvent.setup();
    render(
      <CommandActivityCard
        activity={buildActivity({
          status: "success",
          metadata: { command: "ls", exitCode: 0 },
          output: "file-a\nfile-b",
        })}
      />,
    );

    expect(screen.queryByText("file-a\nfile-b")).not.toBeInTheDocument();

    await user.click(screen.getByTitle("展开详情"));
    expect(screen.getByText(/file-a/)).toBeInTheDocument();
  });
});

describe("PatchActivityCard", () => {
  it("should list changed files with kind badges", () => {
    render(
      <PatchActivityCard
        activity={buildActivity({
          kind: "patch",
          title: "文件变更",
          metadata: {
            changes: [
              { path: "src/a.ts", kind: "add" },
              { path: "src/b.ts", kind: "delete" },
              { path: "src/c.ts", kind: { type: "update" } },
            ],
          },
        })}
      />,
    );

    expect(screen.getByText("文件变更（3 个文件）")).toBeInTheDocument();
    expect(screen.getByText("新增")).toBeInTheDocument();
    expect(screen.getByText("删除")).toBeInTheDocument();
    expect(screen.getByText("修改")).toBeInTheDocument();
    expect(screen.getByText("src/a.ts")).toBeInTheDocument();
  });

  it("should reveal diff after clicking the file row", async () => {
    const user = userEvent.setup();
    render(
      <PatchActivityCard
        activity={buildActivity({
          kind: "patch",
          status: "success",
          metadata: {
            changes: [{ path: "src/a.ts", kind: "update", diff: "+hello" }],
          },
        })}
      />,
    );

    // success 状态默认收起，先展开卡片
    await user.click(screen.getByTitle("展开详情"));
    expect(screen.queryByText("hello")).not.toBeInTheDocument();

    // 语法高亮会把 "+hello" 拆成多个 token，按 token 文本断言
    await user.click(screen.getByText("src/a.ts"));
    expect(screen.getByText("hello")).toBeInTheDocument();
  });

  it("should fall back to raw output when no structured changes exist", () => {
    render(
      <PatchActivityCard
        activity={buildActivity({
          kind: "patch",
          metadata: {},
          output: "raw patch output",
        })}
      />,
    );

    expect(screen.getByText("文件变更（0 个文件）")).toBeInTheDocument();
    expect(screen.getByText("raw patch output")).toBeInTheDocument();
  });
});

describe("ToolCallActivityCard", () => {
  it("should render server-tool header and parameter section", () => {
    render(
      <ToolCallActivityCard
        activity={buildActivity({
          kind: "tool",
          title: "read_file",
          metadata: { server: "aipp:operation", tool: "read_file" },
          input: { path: "/tmp/a.txt" },
          output: "file content",
        })}
      />,
    );

    expect(screen.getByText("aipp:operation")).toBeInTheDocument();
    expect(screen.getByText("read_file")).toBeInTheDocument();
    expect(screen.getByText("参数:")).toBeInTheDocument();
    expect(screen.getByText(/\/tmp\/a\.txt/)).toBeInTheDocument();
    expect(screen.getByText("结果:")).toBeInTheDocument();
  });
});

describe("SubAgentActivityCard 与 GenericActivityCard", () => {
  it("should render sub agent title and output", () => {
    render(
      <SubAgentActivityCard
        activity={buildActivity({
          kind: "sub_agent",
          title: "collabAgentToolCall",
          output: "sub agent result",
        })}
      />,
    );

    expect(screen.getByText("collabAgentToolCall")).toBeInTheDocument();
    expect(screen.getByText("sub agent result")).toBeInTheDocument();
  });

  it("should render unknown kind with generic card", () => {
    render(
      <GenericActivityCard
        activity={buildActivity({
          kind: "approval",
          title: "approval",
          output: "waiting approval",
        })}
      />,
    );

    expect(screen.getByText("approval")).toBeInTheDocument();
    expect(screen.getByText("waiting approval")).toBeInTheDocument();
  });
});
