import { act, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { AgentActivityEvent, ButlerReport, MCPToolCallUpdateEvent, Message } from "@/data/Conversation";
import type { InteractionRow } from "./butlerInteractions";
import { LiveReportBubbles, ReportBubble } from "./ButlerProgressRow";
import { useButlerTranscript } from "./useButlerTranscript";

const conversationId = 31;
const text = "我先核对数据。";
const message = (id: number, message_type: string, content = "") =>
    ({ id, conversation_id: conversationId, message_type, content, created_time: new Date(id) }) as Message;
const history = [message(1, "user", "核对数据"), message(2, "response")];
const report: MCPToolCallUpdateEvent = {
    conversation_id: conversationId, message_id: 2, call_id: 10, tool_name: "report_progress",
    status: "success", parameters: JSON.stringify({ text }),
};
const sibling: MCPToolCallUpdateEvent = {
    conversation_id: conversationId, message_id: 2, call_id: 11, tool_name: "read_file", status: "pending",
};
const liveReport = { text, anchor: 2, request_id: "first" };
const activities = new Map<string, AgentActivityEvent>();

function Transcript({ running, calls, messages = history, live = [liveReport], interactions, activities: observed = activities }: {
    running: boolean; calls: Map<number, MCPToolCallUpdateEvent>; messages?: Message[];
    live?: ButlerReport[]; interactions?: InteractionRow[]; activities?: Map<string, AgentActivityEvent>;
}) {
    const rows = useButlerTranscript({ conversationId, compact: true, running, messages, calls, activities: observed, interactions });
    return rows.map(row => {
        if (row.message_type === "user") return <div key={row.id}>{row.content}</div>;
        if (row.message_type === "butler_report") return <ReportBubble key={row.id} text={row.content} />;
        if (row.message_type === "butler_processing") return <div key={row.id} role="status">
            <LiveReportBubbles reports={live} published={row.butler_published_reports} />
        </div>;
        return null;
    });
}

afterEach(() => vi.useRealTimers());

it("should keep a peer turn's saved reports on either side of a user interjection during execution and after reopening", () => {
    const trigger = { ...message(1, "user", "内部委托"), metadata_json: '{"origin":"butler_peer","audience":"peer"}' };
    const messages = [trigger, message(2, "response", "内部回复"), message(5, "user", "请留意生产库")];
    const live = [liveReport, { ...liveReport, request_id: "second", text: "已核对" }];
    const interactions: InteractionRow[] = live.map((report, index) => ({
        id: index + 1, kind: "report", anchor: 2, request_id: report.request_id,
        occurred_ms: index === 0 ? 3 : 6, request: { text: report.text },
    }));
    const props = { messages, calls: new Map<number, MCPToolCallUpdateEvent>(), interactions };
    const expected = [text, "请留意生产库", "已核对"];
    const active = render(<Transcript {...props} running live={live} />);
    expect(Array.from(active.container.children, row => row.textContent)).toEqual([...expected, ""]);
    active.unmount();

    const reopened = render(<Transcript {...props} running={false} live={[]} />);
    expect(Array.from(reopened.container.children, row => row.textContent)).toEqual(expected);
});

it("should show one report when a sibling tool resumes after waiting for approval", () => {
    vi.useFakeTimers();
    const calls = new Map([report, sibling].map(call => [call.call_id, call]));
    const view = render(<Transcript running calls={calls} />);
    expect(screen.getAllByText(text)).toHaveLength(1);

    view.rerender(<Transcript running={false} calls={calls} />);
    act(() => vi.advanceTimersByTime(2000));
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.getAllByText(text)).toHaveLength(1);

    const approved = new Map(calls).set(sibling.call_id, { ...sibling, status: "executing" });
    view.rerender(<Transcript running calls={approved} />);
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(screen.getAllByText(text)).toHaveLength(1);

    const completed = new Map(approved).set(sibling.call_id, { ...sibling, status: "success" });
    view.rerender(<Transcript running={false} calls={completed} />);
    act(() => vi.advanceTimersByTime(2000));
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.getAllByText(text)).toHaveLength(1);
});

it("should show a new live report when an earlier turn reported the same text", () => {
    const calls = new Map([[report.call_id, report]]);
    const hiddenTrigger = { ...message(3, "user"), metadata_json: '{"origin":"internal"}' };
    render(<Transcript running calls={calls} messages={[...history, hiddenTrigger, message(4, "response")]}
        live={[{ ...liveReport, anchor: 4, request_id: "next-turn" }]} />);
    expect(screen.getAllByText(text)).toHaveLength(2);
});

it("should show one progress bubble when the running response reports after a user interjection", () => {
    const messages = [...history, message(3, "user", "你让他继续吧")];
    const props = { running: true, messages, calls: new Map<number, MCPToolCallUpdateEvent>() };
    const view = render(<Transcript {...props} />);
    expect(screen.getAllByText(text)).toHaveLength(1);

    const activity = { conversation_id: conversationId, response_message_id: 2, kind: "tool", status: "success",
        title: "aipp_t331_report_progress", input: { text }, started_at: new Date(4).toISOString() } as AgentActivityEvent;
    const activities = new Map([["tool-1", activity]]);
    view.rerender(<Transcript {...props} activities={activities} />);
    expect(screen.getAllByText(text)).toHaveLength(1);

    const saved: InteractionRow = { id: 1, anchor: 2, occurred_ms: 4, request_id: "first", kind: "report", request: { text } };
    view.rerender(<Transcript {...props} activities={activities} interactions={[saved]} />);
    expect(screen.getAllByText(text)).toHaveLength(1);

    const second = { ...liveReport, request_id: "second" };
    view.rerender(<Transcript {...props} activities={activities} interactions={[saved]} live={[liveReport, second]} />);
    expect(screen.getAllByText(text)).toHaveLength(2);
    const bothSaved = [saved, { ...saved, id: 2, occurred_ms: 5, request_id: second.request_id }];
    view.rerender(<Transcript {...props} interactions={bothSaved} live={[liveReport, second]} />);
    expect(screen.getAllByText(text)).toHaveLength(2);
    view.rerender(<Transcript {...props} running={false} interactions={bothSaved} live={[]} />);
    expect(screen.getAllByText(text)).toHaveLength(2);
});

it("should keep the previous turn reply published when a sending optimistic user row is active while held", () => {
    const prevReply = { ...message(2, "response", "上一轮完整回复"), finish_time: new Date(2000) } as Message;
    const sendingUser = {
        ...message(-7_000_000_000_000, "user", "下一轮提问"),
        sendStatus: "sending" as const,
        clientRequestId: "22222222-2222-4222-8222-222222222222",
    } as Message;
    let latestRows: Message[] = [];
    function Capture({ running, messages }: { running: boolean; messages: Message[] }) {
        latestRows = useButlerTranscript({
            conversationId,
            compact: true,
            running,
            messages,
            calls: new Map(),
            activities: new Map(),
        });
        return null;
    }
    const view = render(<Capture running messages={[message(1, "user", "第一轮"), prevReply]} />);
    view.rerender(<Capture running={false} messages={[message(1, "user", "第一轮"), prevReply]} />);
    // Still within the 1200ms linger window (held = true) when the user sends the next turn:
    view.rerender(<Capture running messages={[message(1, "user", "第一轮"), prevReply, sendingUser]} />);
    expect(latestRows.map(row => row.id)).toContain(prevReply.id);
    expect(latestRows.map(row => row.id)).toContain(sendingUser.id);
});
