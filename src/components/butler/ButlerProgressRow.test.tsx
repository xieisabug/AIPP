import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ButlerProgressRow from "./ButlerProgressRow";
import { BUTLER_PROGRESS_EVENT } from "./butlerProgress";

const summary = (conversation_id: number, epoch: number, text: string) => ({
    conversation_id, epoch, text, steps: [{ label: "web_search 竞品定价", status: "success" }, { label: "read_file 报价.xlsx", status: "executing" }],
});

describe("ButlerProgressRow", () => {
    afterEach(() => clearAllMockHandlers());

    it("should follow Pi activities and resume the remaining tool when concurrent work finishes", async () => {
        mockInvokeHandler("get_butler_progress_summary", () => null);
        mockInvokeHandler("get_conversation_runtime_state", () => ({ conversation_id: 9, is_running: true, phase: "assistant_streaming", epoch: 1, revision: 2 }));
        render(<ButlerProgressRow conversationId={9} />);
        expect(await screen.findByText("回复中")).toBeTruthy();
        const activity = async (item_id: string, kind: string, status: string, path?: string) =>
            act(async () => { await emit("conversation_event_9", { type: "agent_activity", data: { item_id, kind, status, input: { path } } }); });
        await activity("thought", "reasoning", "executing");
        expect(screen.getByText("思考中")).toBeTruthy();
        await activity("thought", "reasoning", "success");
        await activity("read", "file_read", "executing");
        expect(screen.getByText("阅读中")).toBeTruthy();
        await activity("edit", "patch", "executing", "src/app.ts");
        expect(screen.getByText("编程中")).toBeTruthy();
        await activity("edit", "patch", "failed");
        expect(screen.getByText("阅读中")).toBeTruthy();
        await activity("read", "file_read", "success");
        await activity("find", "file_search", "executing");
        expect(screen.getByText("搜索文件")).toBeTruthy();
        await activity("find", "file_search", "success");
        await activity("bash", "command", "executing");
        expect(screen.getByText("运行命令")).toBeTruthy();
        await activity("bash", "command", "cancelled");
        expect(screen.getByText("回复中")).toBeTruthy();
    });

    it("uses the same short status as the contact list when there is no helper summary", async () => {
        mockInvokeHandler("get_butler_progress_summary", () => null);
        mockInvokeHandler("get_conversation_runtime_state", () => ({ conversation_id: 3, is_running: true, phase: "user_pending", epoch: 1, revision: 1 }));
        render(<ButlerProgressRow conversationId={3} />);
        expect(await screen.findByText("投递中")).toBeTruthy();
        await act(async () => {
            await emit("conversation_event_3", { type: "message_add", data: { message_type: "reasoning" } });
            await emit("conversation_event_3", { type: "runtime_state_snapshot", data: { state: { conversation_id: 3, is_running: true, phase: "assistant_streaming", epoch: 1, revision: 2 } } });
        });
        await waitFor(() => expect(screen.getByText("思考中")).toBeTruthy());
        await act(async () => {
            await emit("conversation_event_3", { type: "runtime_state_snapshot", data: { state: { conversation_id: 3, is_running: true, phase: "mcp_executing", epoch: 1, revision: 3 } } });
            await emit("conversation_event_3", { type: "mcp_tool_call_update", data: { call_id: 1, status: "executing", tool_name: "read_file", parameters: "{\"file_path\":\"a.md\"}" } });
        });
        await waitFor(() => expect(screen.getByText("阅读中")).toBeTruthy());
    });

    it("should display the assistant's own words when a report arrives without helper steps", async () => {
        mockInvokeHandler("get_butler_progress_summary", () => null);
        mockInvokeHandler("get_conversation_runtime_state", () => null);
        render(<ButlerProgressRow conversationId={7} />);
        await act(async () => {
            await emit(BUTLER_PROGRESS_EVENT, { conversation_id: 7, epoch: 3, text: "我先核对两份配置。", steps: [] });
        });
        expect(await screen.findByText("我先核对两份配置。")).toBeTruthy();
        expect(screen.queryByRole("button")).toBeNull();
    });

    it("shows the latest summary for its conversation and expands the steps", async () => {
        mockInvokeHandler("get_butler_progress_summary", () => null);
        mockInvokeHandler("get_conversation_runtime_state", () => null);
        render(<ButlerProgressRow conversationId={3} />);
        await act(async () => {
            await emit(BUTLER_PROGRESS_EVENT, summary(4, 1, "别的会话"));
            await emit(BUTLER_PROGRESS_EVENT, summary(3, 2, "正在对比三家报价"));
            await emit(BUTLER_PROGRESS_EVENT, summary(3, 1, "上一轮遗留"));
        });
        await waitFor(() => expect(screen.getByText("正在对比三家报价")).toBeTruthy());
        expect(screen.queryByText("上一轮遗留")).toBeNull();
        fireEvent.click(screen.getByRole("button"));
        expect(screen.getByText("read_file 报价.xlsx")).toBeTruthy();
    });
});
