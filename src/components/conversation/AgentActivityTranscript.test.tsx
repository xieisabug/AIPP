import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentActivityEvent, Message } from "@/data/Conversation";
import { AgentActivityTranscript } from "./AgentActivityTranscript";

const displayConfig = vi.hoisted(() => ({ isShowThinking: true }));
vi.mock("@/hooks/useDisplayConfig", () => ({ useDisplayConfig: () => displayConfig }));
vi.mock("@/hooks/useCustomTagParser", () => ({ useCustomTagParser: () => ({ parseCustomTags: (content: string) => content }) }));
vi.mock("@/hooks/useMarkdownConfig", async () => {
  const { createContext } = await import("react");
  return { MarkdownStreamingContext: createContext(false), useMarkdownConfig: () => ({ remarkPlugins: [], rehypePlugins: [], markdownComponents: {} }) };
});
vi.mock("@/hooks/useMcpToolCallProcessor", () => ({ useMcpToolCallProcessor: () => ({ processContent: (_content: string, element: React.ReactNode) => element }) }));
vi.mock("@/components/magicui/shine-border", () => ({ ShineBorder: () => null }));
vi.mock("@/hooks/useTheme", () => ({ useTheme: () => ({ resolvedTheme: "light" }) }));

const message: Message = {
  id: 2, conversation_id: 1, message_type: "response", content: "", llm_model_id: null,
  created_time: new Date("2026-11-12T10:00:00Z"), start_time: new Date("2026-11-12T10:00:00Z"),
  finish_time: null, token_count: 0, input_token_count: 0, output_token_count: 0, regenerate: null,
};
const item = (overrides: Partial<AgentActivityEvent>): AgentActivityEvent => ({
  conversation_id: 1, response_message_id: 2, agent_kind: "pi", item_id: "thought", order: 0,
  sequence: 50, kind: "reasoning", status: "success", output: "actual reasoning",
  started_at: "2026-11-12T10:00:00Z", finished_at: "2026-11-12T10:00:01Z", ...overrides,
});
const command = item({ item_id: "command", kind: "command", order: 1, sequence: 2, title: "echo hi", output: "hi" });

describe("AgentActivityTranscript", () => {
  beforeEach(() => { displayConfig.isShowThinking = true; });

  it.each([
    ["claude_sdk", "Claude Code"], ["cursor", "Cursor"], ["opencode", "OpenCode"],
    ["kiro", "Kiro"], ["antigravity", "Antigravity"],
  ])("should render readable operation details and ordinary configurable thoughts when the driver is %s", async (agent_kind, label) => {
    const user = userEvent.setup();
    const activities = JSON.parse(JSON.stringify([
      item({ agent_kind }),
      { ...command, agent_kind, metadata: { presentation: "operation", tool: "shell", rawResult: { stdout: "hi" } } },
    ]));
    const { rerender, container } = render(<AgentActivityTranscript activities={activities} message={message} responseComplete />);
    expect(screen.getByText(label)).toBeInTheDocument();
    expect(screen.getByText(/^思考完成/)).toBeInTheDocument();
    expect(screen.queryByText("hi")).not.toBeInTheDocument();
    await user.click(screen.getByTitle("展开详情"));
    expect(screen.getByText("hi")).toBeInTheDocument();
    expect(screen.getByText("参数与原始结果")).toBeInTheDocument();
    expect(container.querySelector("[data-agent-activity-list] [data-agent-reasoning]")).toBeNull();
    displayConfig.isShowThinking = false;
    rerender(<AgentActivityTranscript activities={activities} message={message} responseComplete />);
    expect(screen.queryByText(/^思考完成/)).not.toBeInTheDocument();
  });

  it("should suppress historical empty Codex thought activities while keeping actual commands", () => {
    const { container } = render(<AgentActivityTranscript activities={[
      item({ agent_kind: "codex_app_server", output: undefined }),
      item({ agent_kind: "codex_app_server", item_id: "part", kind: "reasoning/summaryPartAdded", output: undefined }),
      { ...command, agent_kind: "codex_app_server" },
    ]} message={message} responseComplete />);
    expect(container.querySelector("[data-agent-reasoning]")).toBeNull();
    expect(screen.getByText("echo hi")).toBeInTheDocument();
    expect(screen.queryByText("reasoning/summaryPartAdded")).not.toBeInTheDocument();
  });

  it("should keep Codex command order when the earlier command receives a later update", () => {
    const a = { ...command, agent_kind: "codex_app_server", item_id: "a", order: 0, sequence: 1, title: "command A" };
    const b = { ...a, item_id: "b", order: 1, sequence: 2, title: "command B" };
    const { container, rerender } = render(<AgentActivityTranscript activities={[a, b]} message={message} responseComplete={false} />);
    const order = () => Array.from(container.querySelectorAll("[data-mcp-motion='card']")).map(el => el.textContent);
    expect(order()[0]).toContain("command A");
    rerender(<AgentActivityTranscript activities={[{ ...a, sequence: 100 }, b]} message={message} responseComplete={false} />);
    expect(order()[0]).toContain("command A");
    expect(order()[1]).toContain("command B");
  });

  it("should use ordinary reasoning styling outside tool cards and preserve first-seen order during updates and replay", async () => {
    const user = userEvent.setup();
    const thought = item({});
    const { container, rerender } = render(<AgentActivityTranscript activities={[command, thought]} message={message} responseComplete={false} />);
    const rows = () => Array.from(container.querySelectorAll("[data-agent-reasoning], [data-mcp-motion='card']"));
    expect(rows()[0]).toHaveAttribute("data-agent-reasoning");
    expect(rows()[0].className).toContain("border-l-4");
    expect(rows()[1]).toHaveTextContent("echo hi");
    expect(container.querySelector("[data-agent-activity-list] [data-agent-reasoning]")).toBeNull();
    expect(container.querySelector(".lucide-brain")).toBeNull();
    expect(container.querySelector("[data-message-item]")).toBeNull();
    await user.click(screen.getByText(/^思考完成/));
    expect(screen.getByText("actual reasoning")).toBeInTheDocument();
    const saved = JSON.parse(JSON.stringify([command, { ...thought, sequence: 200 }]));
    rerender(<AgentActivityTranscript activities={saved} message={message} responseComplete />);
    expect(rows()[0]).toHaveAttribute("data-agent-reasoning");
    expect(rows()[1]).toHaveTextContent("echo hi");
    expect(screen.getByText("actual reasoning")).toBeInTheDocument();
  });

  it("should hide completed thoughts and show only a loading indicator during thinking when the display setting is off", () => {
    displayConfig.isShowThinking = false;
    const running = item({ status: "executing", finished_at: undefined });
    const { container, rerender } = render(<AgentActivityTranscript activities={[running, command]} message={message} responseComplete={false} />);
    expect(screen.getByText("思考中...")).toBeInTheDocument();
    expect(screen.queryByText("actual reasoning")).not.toBeInTheDocument();
    expect(screen.queryByText(/^思考完成/)).not.toBeInTheDocument();
    rerender(<AgentActivityTranscript activities={[item({}), command]} message={message} responseComplete />);
    expect(container.querySelector("[data-agent-reasoning]")).toBeNull();
    expect(screen.getByText("echo hi")).toBeInTheDocument();
  });

  it("should respond to changes in the thinking setting when the same transcript stays open", () => {
    const activities = [item({}), command];
    const { rerender } = render(<AgentActivityTranscript activities={activities} message={message} responseComplete />);
    expect(screen.getByText(/^思考完成/)).toBeInTheDocument();
    displayConfig.isShowThinking = false;
    rerender(<AgentActivityTranscript activities={activities} message={message} responseComplete />);
    expect(screen.queryByText(/^思考完成/)).not.toBeInTheDocument();
    displayConfig.isShowThinking = true;
    rerender(<AgentActivityTranscript activities={activities} message={message} responseComplete />);
    expect(screen.getByText(/^思考完成/)).toBeInTheDocument();
  });

  it("should stop displaying the hidden-thinking loader when the enclosing response ends", () => {
    displayConfig.isShowThinking = false;
    const { container } = render(<AgentActivityTranscript activities={[item({ status: "executing", finished_at: undefined })]} message={message} responseComplete />);
    expect(container.querySelector("[data-agent-reasoning]")).toBeNull();
    expect(screen.queryByText("思考中...")).not.toBeInTheDocument();
  });

  it("should reserve the full source label when a command is long", () => {
    render(<AgentActivityTranscript activities={[{ ...command, title: "echo hello ".repeat(100) }]} message={message} responseComplete />);
    const source = screen.getByText("Pi");
    expect(source.className).toContain("shrink-0");
    expect(source.className).toContain("whitespace-nowrap");
    expect(source.className).not.toContain("truncate");
  });
});
