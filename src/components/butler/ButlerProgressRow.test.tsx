import { act, render, renderHook, screen, waitFor } from "@testing-library/react";
import { useContactAvatarActivity } from "@/components/avatar/useContactAvatarActivity";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ButlerProgressRow, { LiveReportBubbles } from "./ButlerProgressRow";
import { BUTLER_PROGRESS_EVENT } from "./butlerProgress";

const snapshot = (latest: { source: string; conversation_id: number; epoch: number; text: string; steps: unknown[] }, reports: string[] = []) => ({
    ...latest, reports: (latest.source === "report_progress" ? [...reports, latest.text] : reports)
        .map((text, i) => ({ text, request_id: String(i), anchor: 2 })),
    helper: latest.source === "helper_summary" ? latest : null,
});

const summary = (conversation_id: number, epoch: number, text: string) => snapshot({
    source: "helper_summary", conversation_id, epoch, text, steps: [{ label: "web_search 竞品定价", status: "success" }, { label: "read_file 报价.xlsx", status: "executing" }],
});

describe("ButlerProgressRow", () => {
    afterEach(() => clearAllMockHandlers());

    it("should show the same cached and live progress in chat and contacts when a turn is running", async () => {
        const runtime = { conversation_id: 3, is_running: true, phase: "assistant_streaming", epoch: 2, revision: 1 };
        const progress = summary(3, 2, "正在核对供应商报价");
        mockInvokeHandler("get_butler_progress_summary", () => progress);
        mockInvokeHandler("get_conversation_runtime_state", () => runtime);
        mockInvokeHandler("list_butler_contact_activity", () => [{ conversation_id: 3, assistant_id: 7, runtime, progress, status: null }]);
        const contacts = renderHook(() => useContactAvatarActivity("1", []));
        render(<ButlerProgressRow conversationId={3} />);
        const checkBoth = async (text: string) => {
            expect(await screen.findByText(text)).toBeTruthy();
            await waitFor(() => expect(contacts.result.current.visuals.get(7)?.label).toBe(text));
        };
        await checkBoth(progress.text);
        expect(screen.getByRole("status")).toHaveClass("justify-center");
        await act(async () => {
            await emit(BUTLER_PROGRESS_EVENT, summary(3, 2, "已核对三份报价"));
            await emit(BUTLER_PROGRESS_EVENT, summary(3, 1, "上一轮进展"));
        });
        await checkBoth("已核对三份报价");
        await act(async () => {
            await emit("conversation_event_3", { type: "runtime_state_snapshot", data: { state: { ...runtime, epoch: 3, revision: 2 } } });
            await emit(BUTLER_PROGRESS_EVENT, summary(3, 2, "上一轮迟到的结果"));
        });
        expect(screen.queryByText("上一轮迟到的结果")).toBeNull();
        expect(contacts.result.current.visuals.get(7)?.label).not.toContain("报价");
    });

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
            await emit(BUTLER_PROGRESS_EVENT, snapshot({ source: "report_progress", conversation_id: 7, epoch: 3, text: "我先核对两份配置。", steps: [] }));
        });
        expect(await screen.findByText("我先核对两份配置。")).toHaveClass("butler-peer-bubble");
        expect(screen.queryByRole("button")).toBeNull();
    });

    it("should center a helper summary without tool steps alongside the report bubble", async () => {
        mockInvokeHandler("get_butler_progress_summary", () => snapshot({ source: "report_progress", conversation_id: 7, epoch: 3, text: "我先核对。", steps: [] }));
        mockInvokeHandler("get_conversation_runtime_state", () => null);
        render(<ButlerProgressRow conversationId={7} />);
        expect(await screen.findByText("我先核对。")).toHaveClass("butler-peer-bubble");
        await act(async () => {
            await emit(BUTLER_PROGRESS_EVENT, snapshot({ source: "helper_summary", conversation_id: 7, epoch: 3, text: "已核对两份配置", steps: [] }, ["我先核对。"]));
        });
        expect(screen.getByText("我先核对。")).toHaveClass("butler-peer-bubble");
        expect(screen.getByRole("status")).toHaveClass("items-center");
        expect(screen.getByText("已核对两份配置").closest(".butler-peer-bubble")).toBeNull();
        expect(screen.queryByRole("button")).toBeNull();
    });

    it("should present the latest helper summary without exposing ordinary tool calls", async () => {
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
        expect(screen.queryByRole("button")).toBeNull();
        expect(screen.queryByText("read_file 报价.xlsx")).toBeNull();
    });

    it("should keep a repeated live report when only its first occurrence is already on the timeline", () => {
        const reports = ["核对中", "核对中", "即将完成"].map((text, i) => ({ text, request_id: String(i), anchor: 2 }));
        render(<LiveReportBubbles reports={reports} published={[{ ...reports[0], currentTurn: true }]} />);
        expect(screen.getAllByText("核对中")).toHaveLength(1);
        expect(screen.getByText("即将完成")).toBeInTheDocument();
    });

    it("should show a new report when another saved report has the same owner and text", () => {
        const report = { text: "核对中", anchor: 2, request_id: "new" };
        render(<LiveReportBubbles reports={[report]} published={[{ ...report, request_id: "earlier", currentTurn: true }]} />);
        expect(screen.getByText(report.text)).toBeInTheDocument();
    });
});
