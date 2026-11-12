import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AgentActivityEvent } from "@/data/Conversation";
import { AgentActivityList } from "./AgentActivityList";

vi.mock("@/components/magicui/shine-border", () => ({ ShineBorder: () => null }));
vi.mock("@/hooks/useTheme", () => ({ useTheme: () => ({ resolvedTheme: "light" }) }));

const activity = (overrides: Partial<AgentActivityEvent> = {}): AgentActivityEvent => ({
  conversation_id: 1, response_message_id: 2, agent_kind: "pi", session_id: "pi-session",
  item_id: "2:tool", sequence: 1, kind: "file_read", title: "读取 README.md", status: "executing",
  input: { path: "README.md" }, metadata: { tool: "read" }, ...overrides,
});

describe("Pi operation cards", () => {
  it.each([
    ["command", "echo hi", "terminal"], ["file_read", "读取 README.md", "file-text"],
    ["file_search", "搜索 hello src", "search"], ["file_write", "写入 a.ts", "file-pen-line"],
    ["patch", "编辑 a.ts", "file-diff"], ["tool", "mcp_custom", "blocks"],
  ])("should render the operation with its own icon when kind is %s", (kind, title, icon) => {
    const { container } = render(<AgentActivityList activities={[activity({ kind, title, output: "details", metadata: { tool: title } })]} />);
    expect(screen.getByText(title)).toBeInTheDocument();
    expect(screen.getByText("Pi")).toBeInTheDocument();
    expect(container.querySelector(`.lucide-${icon}`)).toBeInTheDocument();
    expect(screen.queryByText("details")).not.toBeInTheDocument();
  });

  it("should exclude thoughts when rendering tool cards", () => {
    const { container } = render(<AgentActivityList activities={[activity({ kind: "reasoning", title: "思考", output: "private thought" })]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("should update one card and expose readable results when progress completes", async () => {
    const user = userEvent.setup();
    const { rerender, container } = render(<AgentActivityList activities={[activity()]} />);
    const completed = activity({ sequence: 3, status: "success", output: "file text", metadata: {
      tool: "read", rawResult: { content: [{ type: "text", text: "file text" }] },
    } });
    rerender(<AgentActivityList activities={[completed]} />);
    expect(container.querySelectorAll(".lucide-file-text")).toHaveLength(1);
    expect(screen.queryByText("file text")).not.toBeInTheDocument();
    await user.click(screen.getByTitle("展开详情"));
    expect(screen.getByText("file text")).toBeInTheDocument();
    expect(screen.getByText("参数与原始结果")).toBeInTheDocument();
  });

  it("should show failure details automatically when a file operation fails", () => {
    render(<AgentActivityList activities={[activity({ status: "failed", output: "ENOENT: missing", error: "ENOENT: missing" })]} />);
    expect(screen.getByText("ENOENT: missing")).toBeInTheDocument();
  });

  it("should preserve stopped operations when replaying saved activities", async () => {
    const user = userEvent.setup();
    const saved = JSON.stringify([activity({ kind: "command", title: "sleep 10", status: "cancelled", error: "操作已停止", metadata: { tool: "bash" } })]);
    render(<AgentActivityList activities={JSON.parse(saved)} />);
    expect(screen.getByText("Pi")).toBeInTheDocument();
    expect(screen.getByText("sleep 10")).toBeInTheDocument();
    await user.click(screen.getByTitle("展开详情"));
    expect(screen.getByText("操作已停止")).toBeInTheDocument();
  });

  it("should adapt old Pi tool records when replaying history", async () => {
    const user = userEvent.setup();
    const { container } = render(<AgentActivityList activities={[activity({
      kind: "tool", title: "read", status: "success", metadata: {},
      output: JSON.stringify({ content: [{ type: "text", text: "old file text" }] }),
    })]} />);
    expect(screen.getByText("读取 README.md")).toBeInTheDocument();
    expect(container.querySelector(".lucide-file-text")).toBeInTheDocument();
    await user.click(screen.getByTitle("展开详情"));
    expect(screen.getByText("old file text")).toBeInTheDocument();
  });

  it("should show a real diff when Pi returns an edit result", async () => {
    const user = userEvent.setup();
    render(<AgentActivityList activities={[activity({
      kind: "patch", title: "编辑 a.ts", status: "success", output: "Edited a.ts",
      metadata: { tool: "edit", changes: [{ path: "a.ts", kind: "update", diff: "+hello" }] },
    })]} />);
    await user.click(screen.getByTitle("展开详情"));
    await user.click(screen.getByText("a.ts"));
    expect(screen.getByText("hello")).toBeInTheDocument();
    expect(screen.getByText("Edited a.ts")).toBeInTheDocument();
  });
});
