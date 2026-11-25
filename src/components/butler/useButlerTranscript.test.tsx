import { act, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { AgentActivityEvent, MCPToolCallUpdateEvent, Message } from "@/data/Conversation";
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
const activities = new Map<string, AgentActivityEvent>();

function Transcript({ running, calls, messages = history }: {
    running: boolean; calls: Map<number, MCPToolCallUpdateEvent>; messages?: Message[];
}) {
    const rows = useButlerTranscript({ conversationId, compact: true, running, messages, calls, activities });
    return rows.map(row => {
        if (row.message_type === "butler_report") return <ReportBubble key={row.id} text={row.content} />;
        if (row.message_type === "butler_processing") return <div key={row.id} role="status">
            <LiveReportBubbles reports={[text]} published={row.butler_published_reports} />
        </div>;
        return null;
    });
}

afterEach(() => vi.useRealTimers());

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
    render(<Transcript running calls={calls} messages={[...history, hiddenTrigger, message(4, "response")]} />);
    expect(screen.getAllByText(text)).toHaveLength(2);
});
